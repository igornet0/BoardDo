//! Execution ledger runtime: records every model call (tokens, $ and latency)
//! with the objective / item / goal run / workflow execution that caused it.
//!
//! Attribution travels as a task-local [`LedgerScope`]: callers wrap work in
//! [`scoped`], nested scopes merge, and code that crosses a `tokio::spawn`
//! captures [`current`] before spawning and re-enters it inside the task.
//! Writes go through a background task so a slow database never delays a
//! model reply; before [`init`] (e.g. in unit tests) entries are dropped.

use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

use boarddo_shared::v2::{LedgerEntry, LedgerScope, estimate_cost_usd};
use chrono::Utc;
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use uuid::Uuid;

use crate::storage::Storage;

static SINK: OnceLock<UnboundedSender<LedgerEntry>> = OnceLock::new();

tokio::task_local! {
    static SCOPE: LedgerScope;
}

/// Starts the writer task. Call once after migrations.
pub fn init(storage: Storage) {
    let (tx, mut rx) = unbounded_channel::<LedgerEntry>();
    if SINK.set(tx).is_err() {
        return;
    }
    tokio::spawn(async move {
        while let Some(entry) = rx.recv().await {
            if let Err(err) = storage.insert_ledger_entry(&entry).await {
                tracing::warn!(error = %err, model = %entry.model, "ledger.write_failed");
            }
        }
    });
}

/// The attribution of the current task, if any scope is active.
pub fn current() -> Option<LedgerScope> {
    SCOPE.try_with(|s| s.clone()).ok()
}

/// Runs `fut` with `scope` merged over the current one.
pub async fn scoped<F: Future>(scope: LedgerScope, fut: F) -> F::Output {
    let merged = current().unwrap_or_default().merged(&scope);
    SCOPE.scope(merged, fut).await
}

/// Re-enters a scope captured with [`current`] (e.g. inside `tokio::spawn`).
pub async fn within<F: Future>(scope: Option<LedgerScope>, fut: F) -> F::Output {
    match scope {
        Some(s) => scoped(s, fut).await,
        None => fut.await,
    }
}

/// Builds the ledger row for one chat call from its OpenAI-shaped `usage`.
pub fn chat_entry(
    provider: &str,
    model: &str,
    usage: Option<&Value>,
    duration: Duration,
    error: Option<&str>,
) -> LedgerEntry {
    let tokens = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let input_tokens = tokens("prompt_tokens");
    let output_tokens = tokens("completion_tokens");
    LedgerEntry {
        id: Uuid::new_v4(),
        at: Utc::now(),
        provider: provider.to_string(),
        model: model.to_string(),
        kind: "chat".into(),
        input_tokens,
        output_tokens,
        cost_usd: estimate_cost_usd(model, input_tokens, output_tokens),
        duration_ms: duration.as_millis() as u64,
        ok: error.is_none(),
        error: error.map(|e| e.chars().take(500).collect()),
        scope: current().unwrap_or_default(),
    }
}

pub fn record(entry: LedgerEntry) {
    if let Some(tx) = SINK.get() {
        let _ = tx.send(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn scopes_nest_and_cross_spawn() {
        assert!(current().is_none());
        let obj = Uuid::new_v4();
        let wf = Uuid::new_v4();
        let entry = scoped(
            LedgerScope {
                objective_id: Some(obj),
                ..Default::default()
            },
            async move {
                let captured = current();
                tokio::spawn(within(captured, async move {
                    scoped(
                        LedgerScope {
                            workflow_id: Some(wf),
                            ..Default::default()
                        },
                        async {
                            chat_entry(
                                "openai",
                                "gpt-4o",
                                Some(&json!({"prompt_tokens": 1000, "completion_tokens": 100})),
                                Duration::from_millis(42),
                                None,
                            )
                        },
                    )
                    .await
                }))
                .await
                .unwrap()
            },
        )
        .await;
        assert_eq!(entry.scope.objective_id, Some(obj));
        assert_eq!(entry.scope.workflow_id, Some(wf));
        assert_eq!(entry.input_tokens, 1000);
        assert_eq!(entry.output_tokens, 100);
        assert_eq!(entry.duration_ms, 42);
        assert!(entry.ok);
        assert!((entry.cost_usd.unwrap() - 0.0035).abs() < 1e-9);
    }

    #[test]
    fn failed_call_without_usage() {
        let e = chat_entry("anthropic", "claude-sonnet-4-5", None, Duration::ZERO, Some("boom"));
        assert!(!e.ok);
        assert_eq!(e.error.as_deref(), Some("boom"));
        assert_eq!(e.cost_usd, Some(0.0));
        assert_eq!(e.scope, LedgerScope::default());
    }
}
