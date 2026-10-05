use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use boarddo_shared::RuntimeSnapshot;
use uuid::Uuid;

use crate::state::SharedState;

pub async fn list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<RuntimeSnapshot>>, (StatusCode, String)> {
    Ok(Json(state.runtime.list()))
}

pub async fn get(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RuntimeSnapshot>, (StatusCode, String)> {
    let wf = state
        .storage
        .get_workflow(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "workflow not found".into()))?;
    Ok(Json(state.runtime.snapshot(id, &wf.name)))
}

pub async fn start(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RuntimeSnapshot>, (StatusCode, String)> {
    state
        .storage
        .get_workflow(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "workflow not found".into()))?;
    state
        .runtime
        .start(&state, id)
        .await
        .map(Json)
        .map_err(|err| {
            let msg = err.to_string();
            if msg.contains("not found") {
                (StatusCode::NOT_FOUND, msg)
            } else if msg.contains("requires a Telegram") || msg.contains("schedule sync") {
                (StatusCode::BAD_REQUEST, msg)
            } else {
                internal(err)
            }
        })
}

pub async fn stop(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RuntimeSnapshot>, (StatusCode, String)> {
    state
        .storage
        .get_workflow(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "workflow not found".into()))?;
    state
        .runtime
        .stop(&state, id)
        .await
        .map(Json)
        .map_err(internal)
}

pub async fn reload(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RuntimeSnapshot>, (StatusCode, String)> {
    state
        .storage
        .get_workflow(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "workflow not found".into()))?;
    state
        .runtime
        .reload(&state, id)
        .await
        .map(Json)
        .map_err(|err| {
            let msg = err.to_string();
            if msg.contains("not found") {
                (StatusCode::NOT_FOUND, msg)
            } else if msg.contains("requires a Telegram")
                || msg.contains("not running")
                || msg.contains("schedule sync")
            {
                (StatusCode::BAD_REQUEST, msg)
            } else {
                internal(err)
            }
        })
}

fn internal(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = %err, "internal error");
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
