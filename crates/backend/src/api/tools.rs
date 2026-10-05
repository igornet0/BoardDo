use axum::extract::{Path, Query, State};
use axum::Json;
use boarddo_shared::v2::{CreateToolRequest, ToolOrigin};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::SharedState;
use crate::tools::service::ToolService;

#[derive(Deserialize)]
pub struct FindQuery {
    pub q: Option<String>,
    pub limit: Option<u32>,
}

pub async fn list(State(state): State<SharedState>) -> Result<Json<Value>, String> {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(json!({ "tools": svc.list().await? })))
}

pub async fn find(
    State(state): State<SharedState>,
    Query(q): Query<FindQuery>,
) -> Result<Json<Value>, String> {
    let svc = ToolService::new(state.storage.clone());
    let query = q.q.unwrap_or_default();
    let limit = q.limit.unwrap_or(20);
    Ok(Json(json!({ "tools": svc.find(&query, limit).await? })))
}

pub async fn get(State(state): State<SharedState>, Path(id): Path<Uuid>) -> Result<Json<Value>, String> {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.get_detail(id).await?))
}

pub async fn create(
    State(state): State<SharedState>,
    Json(body): Json<CreateToolRequest>,
) -> Result<Json<Value>, String> {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(
        svc.create_tool(body, "user", ToolOrigin::User).await?,
    ))
}

#[derive(Deserialize)]
pub struct RunBody {
    pub input: Value,
}

pub async fn run(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<RunBody>,
) -> Result<Json<Value>, String> {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.run_tool(&id, body.input).await?))
}

pub async fn test_run(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<RunBody>,
) -> Result<Json<Value>, String> {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.test_tool(&id, body.input).await?))
}

pub async fn delete(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, String> {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.delete_tool(&id).await?))
}
