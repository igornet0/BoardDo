use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use boarddo_shared::{
    CreateWorkflowRequest, RunWorkflowRequest, RunWorkflowResponse, UpdateWorkflowRequest,
    ValidateResponse, WorkflowRecord, WorkflowSchedule, WorkflowSummary,
};
use uuid::Uuid;

use super::run::spawn_execution;
use crate::state::SharedState;

pub async fn list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<WorkflowSummary>>, (StatusCode, String)> {
    state
        .storage
        .list_workflows()
        .await
        .map(Json)
        .map_err(internal)
}

pub async fn create(
    State(state): State<SharedState>,
    Json(req): Json<CreateWorkflowRequest>,
) -> Result<(StatusCode, Json<WorkflowRecord>), (StatusCode, String)> {
    let record = state.storage.create_workflow(req).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn get(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkflowRecord>, (StatusCode, String)> {
    match state.storage.get_workflow(id).await.map_err(internal)? {
        Some(wf) => Ok(Json(wf)),
        None => Err((StatusCode::NOT_FOUND, "workflow not found".into())),
    }
}

pub async fn update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateWorkflowRequest>,
) -> Result<Json<WorkflowRecord>, (StatusCode, String)> {
    match state
        .storage
        .update_workflow(id, req)
        .await
        .map_err(internal)?
    {
        Some(wf) => {
            if let Err(err) = state.runtime.sync_workflow(&state, &wf).await {
                tracing::warn!(error = %err, workflow_id = %wf.id, "runtime.sync_after_update");
            }
            Ok(Json(wf))
        }
        None => Err((StatusCode::NOT_FOUND, "workflow not found".into())),
    }
}

pub async fn delete(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let _ = state.runtime.stop(&state, id).await;
    let deleted = state.storage.delete_workflow(id).await.map_err(internal)?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "workflow not found".into()))
    }
}

pub async fn validate(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ValidateResponse>, (StatusCode, String)> {
    let wf = state
        .storage
        .get_workflow(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "workflow not found".into()))?;

    match state.engine.validate(&wf.definition) {
        Ok(()) => Ok(Json(ValidateResponse {
            valid: true,
            errors: vec![],
        })),
        Err(errors) => Ok(Json(ValidateResponse {
            valid: false,
            errors,
        })),
    }
}

pub async fn run(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<RunWorkflowRequest>,
) -> Result<Json<RunWorkflowResponse>, (StatusCode, String)> {
    let wf = state
        .storage
        .get_workflow(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "workflow not found".into()))?;

    spawn_execution(&state, &wf, req.trigger, "manual")
        .await
        .map(Json)
        .map_err(|err| {
            let msg = err.to_string();
            if msg.starts_with("Workflow validation failed") || msg.contains("has no `") {
                (StatusCode::BAD_REQUEST, msg)
            } else {
                internal(err)
            }
        })
}

pub async fn list_schedules(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<WorkflowSchedule>>, (StatusCode, String)> {
    state
        .storage
        .get_workflow(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "workflow not found".into()))?;

    state
        .storage
        .list_schedules_for_workflow(id)
        .await
        .map(Json)
        .map_err(internal)
}

fn internal(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = %err, "internal error");
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
