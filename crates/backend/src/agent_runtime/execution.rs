//! Execution guard for tool calls: single execution of sends / irreversible
//! actions through the call journal, bounded retries of transient failures
//! for idempotent tools.

use std::future::Future;
use std::time::Duration;

use boarddo_shared::v2::{
    ToolDescriptor, VerificationFailure, classify_failure, failure_had_no_effect,
    requires_single_execution,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::storage::{JournalClaim, Storage};

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(250),
        }
    }
}

fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for k in keys {
                out.insert(k.clone(), canonical(&map[k]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// Same run + tool + config ⇒ same key, whatever the plan step id or the
/// order of keys in the config.
pub fn idempotency_key(run_id: Uuid, type_id: &str, config: &Value) -> String {
    let mut h = Sha256::new();
    h.update(run_id.as_bytes());
    h.update([0]);
    h.update(type_id.as_bytes());
    h.update([0]);
    h.update(canonical(config).to_string().as_bytes());
    hex::encode(h.finalize())
}

/// Runs `attempt` under the guard. Single-execution tools are claimed in the
/// journal first and never run when the claim can not be established or a
/// previous attempt may have taken effect.
pub async fn run_guarded<F, Fut>(
    storage: &Storage,
    d: Option<&ToolDescriptor>,
    run_id: Uuid,
    type_id: &str,
    config: &Value,
    policy: RetryPolicy,
    mut attempt: F,
) -> Result<Value, String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Value, String>>,
{
    let Some(d) = d else {
        return attempt().await;
    };

    if requires_single_execution(d) {
        let key = idempotency_key(run_id, type_id, config);
        match storage.tool_journal_claim(&key, run_id, type_id).await {
            Err(e) => {
                return Err(format!(
                    "call journal unavailable, `{type_id}` not executed: {e}"
                ));
            }
            Ok(JournalClaim::Done(result)) => {
                tracing::info!(%run_id, type_id, "duplicate call skipped, reusing recorded result");
                return Ok(result);
            }
            Ok(JournalClaim::InFlight) => {
                return Err(format!(
                    "`{type_id}` with the same input is already in progress or its outcome is unknown; not repeating"
                ));
            }
            Ok(JournalClaim::FailedUncertain(prev)) => {
                return Err(format!(
                    "`{type_id}` with the same input failed earlier with an uncertain outcome ({prev}); not repeating"
                ));
            }
            Ok(JournalClaim::Fresh | JournalClaim::Reclaimed) => {}
        }

        let outcome = attempt().await;
        let retry_safe = match &outcome {
            Ok(_) => false,
            Err(e) => failure_had_no_effect(classify_failure(e)),
        };
        if let Err(e) = storage.tool_journal_finish(&key, &outcome, retry_safe).await {
            // The row stays `started`, so any repeat is refused.
            tracing::error!(%run_id, type_id, error = %e, "failed to record tool call outcome");
        }
        return outcome;
    }

    let max = policy.max_attempts.max(1);
    let mut n = 1;
    loop {
        match attempt().await {
            Ok(v) => return Ok(v),
            Err(e) if n < max && VerificationFailure::new(d, e.as_str()).retryable => {
                let delay = policy.base_delay * 2u32.pow(n - 1);
                tracing::warn!(%run_id, type_id, attempt = n, error = %e, "transient failure, retrying");
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                n += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::lookup_tool;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    async fn storage() -> Storage {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let s = Storage::from_pool(pool);
        s.migrate_tool_journal().await.unwrap();
        s
    }

    fn fast() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::ZERO,
        }
    }

    #[test]
    fn key_ignores_key_order_and_differs_by_input() {
        let run = Uuid::now_v7();
        let a = idempotency_key(run, "telegram.send_message", &json!({"chat_id": 1, "text": {"a": 1, "b": 2}}));
        let b = idempotency_key(run, "telegram.send_message", &json!({"text": {"b": 2, "a": 1}, "chat_id": 1}));
        let c = idempotency_key(run, "telegram.send_message", &json!({"chat_id": 2, "text": {"a": 1, "b": 2}}));
        let d = idempotency_key(Uuid::now_v7(), "telegram.send_message", &json!({"chat_id": 1, "text": {"a": 1, "b": 2}}));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[tokio::test]
    async fn send_runs_once_and_repeat_reuses_result() {
        let s = storage().await;
        let d = lookup_tool("telegram.send_message");
        let run = Uuid::now_v7();
        let cfg = json!({"chat_id": 1, "text": "hi"});
        let calls = AtomicU32::new(0);
        let attempt = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"ok": true, "message_id": 7}))
        };
        let first = run_guarded(&s, d, run, "telegram.send_message", &cfg, fast(), attempt).await;
        let second = run_guarded(&s, d, run, "telegram.send_message", &cfg, fast(), attempt).await;
        assert_eq!(first, Ok(json!({"ok": true, "message_id": 7})));
        assert_eq!(second, first);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unfinished_claim_blocks_repeat() {
        let s = storage().await;
        let run = Uuid::now_v7();
        let cfg = json!({"chat_id": 1, "text": "hi"});
        let key = idempotency_key(run, "telegram.send_message", &cfg);
        assert_eq!(s.tool_journal_claim(&key, run, "telegram.send_message").await.unwrap(), JournalClaim::Fresh);
        let calls = AtomicU32::new(0);
        let r = run_guarded(&s, lookup_tool("telegram.send_message"), run, "telegram.send_message", &cfg, fast(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"ok": true}))
        })
        .await;
        assert!(r.unwrap_err().contains("not repeating"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn uncertain_failure_is_not_repeated_but_refusal_is() {
        let s = storage().await;
        let d = lookup_tool("telegram.send_message");
        let run = Uuid::now_v7();
        let calls = AtomicU32::new(0);

        // Timeout: the message may have been delivered.
        let cfg = json!({"chat_id": 1, "text": "a"});
        let r = run_guarded(&s, d, run, "telegram.send_message", &cfg, fast(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("request timed out".to_string())
        })
        .await;
        assert!(r.is_err());
        let r = run_guarded(&s, d, run, "telegram.send_message", &cfg, fast(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"ok": true}))
        })
        .await;
        assert!(r.unwrap_err().contains("uncertain outcome"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // 429: refused upstream, nothing was sent; a later call may go out.
        let cfg = json!({"chat_id": 1, "text": "b"});
        let r = run_guarded(&s, d, run, "telegram.send_message", &cfg, fast(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("HTTP 429 Too Many Requests".to_string())
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2, "single-execution tool is not retried in-call");
        let r = run_guarded(&s, d, run, "telegram.send_message", &cfg, fast(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"ok": true}))
        })
        .await;
        assert_eq!(r, Ok(json!({"ok": true})));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn idempotent_tool_retries_transient_failures_only() {
        let s = storage().await;
        let d = lookup_tool("web.search");
        let run = Uuid::now_v7();
        let cfg = json!({"query": "x"});

        let calls = AtomicU32::new(0);
        let r = run_guarded(&s, d, run, "web.search", &cfg, fast(), || async {
            if calls.fetch_add(1, Ordering::SeqCst) < 2 {
                Err("connection timed out".to_string())
            } else {
                Ok(json!({"results": []}))
            }
        })
        .await;
        assert_eq!(r, Ok(json!({"results": []})));
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        let calls = AtomicU32::new(0);
        let r = run_guarded(&s, d, run, "web.search", &cfg, fast(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("timed out".to_string())
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3, "bounded by max_attempts");

        let calls = AtomicU32::new(0);
        let r = run_guarded(&s, d, run, "web.search", &cfg, fast(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("401 unauthorized".to_string())
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "permanent failure is not retried");
    }

    #[tokio::test]
    async fn non_idempotent_internal_tool_is_not_retried() {
        let s = storage().await;
        let calls = AtomicU32::new(0);
        let r = run_guarded(&s, lookup_tool("lead.create"), Uuid::now_v7(), "lead.create", &json!({}), fast(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("timed out".to_string())
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
