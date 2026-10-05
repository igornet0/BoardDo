use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use boarddo_shared::{Execution, ExecutionChangelog, NodeExecution};
use serde::Deserialize;
use uuid::Uuid;

use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ChangelogListQuery {
    pub workflow_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

pub async fn list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<Execution>>, (StatusCode, String)> {
    state
        .storage
        .list_executions()
        .await
        .map(Json)
        .map_err(internal)
}

pub async fn get(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Execution>, (StatusCode, String)> {
    match state.storage.get_execution(id).await.map_err(internal)? {
        Some(ex) => Ok(Json(ex)),
        None => Err((StatusCode::NOT_FOUND, "execution not found".into())),
    }
}

pub async fn nodes(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<NodeExecution>>, (StatusCode, String)> {
    // Ensure execution exists
    state
        .storage
        .get_execution(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "execution not found".into()))?;

    state
        .storage
        .get_node_executions(id)
        .await
        .map(Json)
        .map_err(internal)
}

pub async fn changelog(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ExecutionChangelog>, (StatusCode, String)> {
    match state
        .storage
        .get_execution_changelog(id)
        .await
        .map_err(internal)?
    {
        Some(log) => Ok(Json(log)),
        None => Err((StatusCode::NOT_FOUND, "execution changelog not found".into())),
    }
}

pub async fn list_changelogs(
    State(state): State<SharedState>,
    Query(q): Query<ChangelogListQuery>,
) -> Result<Json<Vec<ExecutionChangelog>>, (StatusCode, String)> {
    state
        .storage
        .list_execution_changelogs(q.workflow_id, q.limit)
        .await
        .map(Json)
        .map_err(internal)
}

fn internal(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = %err, "internal error");
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
