//! Goal analytics API — outcome vs. operational metrics per goal.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use boarddo_shared::v2::{
    ExternalSpendEntry, GoalAnalyticsReport, GoalAnalyticsRow, NewExternalSpend, compute_goal_report,
};
use chrono::Utc;
use uuid::Uuid;

use crate::state::SharedState;

type ApiError = (StatusCode, String);
type ApiResult<T> = Result<Json<T>, ApiError>;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/analytics/overview", get(overview))
        .route("/analytics/goals/{id}", get(goal_report))
        .route(
            "/goals/{id}/external-spend",
            get(list_external_spend).post(record_external_spend),
        )
}

async fn require_goal(state: &SharedState, id: Uuid) -> Result<(), ApiError> {
    match state.storage.get_goal_spec(id).await.map_err(internal)? {
        Some(_) => Ok(()),
        None => Err((StatusCode::NOT_FOUND, format!("goal {id} not found"))),
    }
}

/// Money spent outside model calls (ads, services, contractors), counted in
/// the goal's cost report and, when in USD, against its budget.
async fn record_external_spend(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<NewExternalSpend>,
) -> Result<(StatusCode, Json<ExternalSpendEntry>), ApiError> {
    require_goal(&state, id).await?;
    let entry = body
        .into_entry(id, Utc::now())
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    state
        .storage
        .insert_external_spend(&entry)
        .await
        .map_err(internal)?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn list_external_spend(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Vec<ExternalSpendEntry>> {
    require_goal(&state, id).await?;
    Ok(Json(
        state
            .storage
            .list_external_spend(id)
            .await
            .map_err(internal)?,
    ))
}

fn internal(e: anyhow::Error) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn goal_report(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> ApiResult<GoalAnalyticsReport> {
    let facts = state
        .storage
        .goal_analytics_facts(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, format!("goal {id} not found")))?;
    Ok(Json(compute_goal_report(&facts)))
}

async fn overview(State(state): State<SharedState>) -> ApiResult<Vec<GoalAnalyticsRow>> {
    let specs = state.storage.list_goal_specs().await.map_err(internal)?;
    let mut rows = Vec::with_capacity(specs.len());
    for spec in specs {
        // A goal deleted between the listing and this read is simply skipped.
        if let Some(facts) = state
            .storage
            .goal_analytics_facts(spec.id)
            .await
            .map_err(internal)?
        {
            rows.push(GoalAnalyticsRow::from(&compute_goal_report(&facts)));
        }
    }
    Ok(Json(rows))
}
