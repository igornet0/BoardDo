//! Execution ledger API — raw model-call rows and spend summaries.

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use boarddo_shared::v2::{LedgerEntry, LedgerQuery, LedgerSummary, compute_ledger_summary};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::state::SharedState;

type ApiError = (StatusCode, String);
type ApiResult<T> = Result<Json<T>, ApiError>;

const DEFAULT_LIMIT: i64 = 200;
const MAX_LIMIT: i64 = 5000;
const DEFAULT_SUMMARY_DAYS: i64 = 30;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/ledger", get(list_entries))
        .route("/ledger/summary", get(summary))
}

/// Query-string form of [`LedgerQuery`]; spelled out because
/// `#[serde(flatten)]` breaks non-string fields in url-encoded queries.
#[derive(Debug, Default, Deserialize)]
struct LedgerParams {
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    objective_id: Option<Uuid>,
    goal_run_id: Option<Uuid>,
    workflow_id: Option<Uuid>,
    execution_id: Option<Uuid>,
    model: Option<String>,
    limit: Option<i64>,
}

impl LedgerParams {
    fn query(&self) -> LedgerQuery {
        LedgerQuery {
            since: self.since,
            until: self.until,
            objective_id: self.objective_id,
            goal_run_id: self.goal_run_id,
            workflow_id: self.workflow_id,
            execution_id: self.execution_id,
            model: self.model.clone(),
        }
    }
}

fn internal(e: anyhow::Error) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn list_entries(
    State(state): State<SharedState>,
    Query(p): Query<LedgerParams>,
) -> ApiResult<Vec<LedgerEntry>> {
    let limit = p.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let rows = state
        .storage
        .list_ledger(&p.query(), Some(limit))
        .await
        .map_err(internal)?;
    Ok(Json(rows))
}

/// Totals and breakdowns; without `since` covers the last 30 days.
async fn summary(
    State(state): State<SharedState>,
    Query(p): Query<LedgerParams>,
) -> ApiResult<LedgerSummary> {
    let mut q = p.query();
    if q.since.is_none() {
        q.since = Some(Utc::now() - Duration::days(DEFAULT_SUMMARY_DAYS));
    }
    let rows = state.storage.list_ledger(&q, None).await.map_err(internal)?;
    Ok(Json(compute_ledger_summary(&rows)))
}

#[cfg(test)]
mod tests {
    use boarddo_shared::v2::LedgerScope;
    use reqwest::Method;
    use serde_json::json;

    use super::*;
    use crate::objectives::tests::{call, serve_api, test_state};

    fn entry(model: &str, inp: u64, out: u64, days_ago: i64, scope: LedgerScope) -> LedgerEntry {
        let mut e = crate::ledger::chat_entry(
            "openai",
            model,
            Some(&json!({"prompt_tokens": inp, "completion_tokens": out})),
            std::time::Duration::from_millis(10),
            None,
        );
        e.at = Utc::now() - Duration::days(days_ago);
        e.scope = scope;
        e
    }

    #[tokio::test]
    async fn list_and_summary() {
        let state = test_state().await;
        let obj = Uuid::new_v4();
        let scoped = LedgerScope {
            objective_id: Some(obj),
            ..Default::default()
        };
        for e in [
            entry("gpt-4o", 1_000_000, 0, 0, scoped.clone()),
            entry("gpt-4o-mini", 1_000_000, 0, 1, LedgerScope::default()),
            entry("gpt-4o", 1_000_000, 0, 60, scoped),
        ] {
            state.storage.insert_ledger_entry(&e).await.unwrap();
        }
        let api = serve_api(state).await;
        let c = reqwest::Client::new();

        let rows = call(&c, Method::GET, format!("{api}/ledger?limit=2"), json!(null)).await;
        assert_eq!(rows.as_array().unwrap().len(), 2);
        let rows = call(
            &c,
            Method::GET,
            format!("{api}/ledger?objective_id={obj}"),
            json!(null),
        )
        .await;
        assert_eq!(rows.as_array().unwrap().len(), 2);

        // default window is 30 days: the 60-day-old call is excluded
        let s = call(&c, Method::GET, format!("{api}/ledger/summary"), json!(null)).await;
        assert_eq!(s["totals"]["calls"], 2);
        assert!((s["totals"]["cost_usd"].as_f64().unwrap() - 2.65).abs() < 1e-9);
        assert_eq!(s["by_model"][0]["key"], "gpt-4o");
        assert_eq!(s["by_objective"][0]["key"], obj.to_string());
    }
}
