use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use boarddo_shared::v2::{CreateToolRequest, ToolOrigin};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::SharedState;
use crate::tools::generate::{self, GenerateToolRequest};
use crate::tools::runtime;
use crate::tools::service::{ToolError, ToolService};

/// Tool runs may carry large datasets.
pub const BODY_LIMIT: usize = 32 * 1024 * 1024;

/// JSON error body `{ "error", "kind", "details"? }` with a matching status.
pub struct ApiError(ToolError);

impl From<ToolError> for ApiError {
    fn from(e: ToolError) -> Self {
        Self(e)
    }
}

impl From<String> for ApiError {
    fn from(e: String) -> Self {
        Self(ToolError::Internal(e))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind) = match &self.0 {
            ToolError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ToolError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            ToolError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid"),
            ToolError::BuildFailed { .. } => (StatusCode::UNPROCESSABLE_ENTITY, "build_failed"),
            ToolError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        let mut body = json!({ "error": self.0.message(), "kind": kind });
        if let ToolError::BuildFailed { details, .. } = &self.0 {
            body["details"] = details.clone();
        }
        (status, Json(body)).into_response()
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

#[derive(Deserialize)]
pub struct FindQuery {
    pub q: Option<String>,
    pub limit: Option<u32>,
}

/// Unified tool registry: every tool id with its contract.
pub async fn catalog() -> Json<Vec<boarddo_shared::v2::ToolDescriptor>> {
    Json(boarddo_shared::v2::all_tools().to_vec())
}

pub async fn list(State(state): State<SharedState>) -> ApiResult {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(json!({ "tools": svc.list().await? })))
}

pub async fn find(State(state): State<SharedState>, Query(q): Query<FindQuery>) -> ApiResult {
    let svc = ToolService::new(state.storage.clone());
    let query = q.q.unwrap_or_default();
    let limit = q.limit.unwrap_or(20);
    Ok(Json(json!({ "tools": svc.find(&query, limit).await? })))
}

pub async fn get(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult {
    let svc = ToolService::new(state.storage.clone());
    let tool = svc.resolve_tool(&id).await?;
    Ok(Json(svc.get_detail(tool.id).await?))
}

pub async fn create(State(state): State<SharedState>, Json(body): Json<CreateToolRequest>) -> ApiResult {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.create_tool(body, "user", ToolOrigin::User).await?))
}

/// New version of an existing tool; a failing build keeps the active version.
pub async fn update(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<CreateToolRequest>,
) -> ApiResult {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.update_tool(&id, body, "user").await?))
}

/// AI writes, tests and repairs a tool from a description.
pub async fn generate(State(state): State<SharedState>, Json(body): Json<GenerateToolRequest>) -> ApiResult {
    Ok(Json(generate::generate(&state, body).await?))
}

/// Interpreter in use, the alternatives found and their packages.
pub async fn runtime() -> ApiResult {
    Ok(Json(tokio::task::spawn_blocking(runtime::status).await.map_err(|e| e.to_string())?))
}

#[derive(Deserialize)]
pub struct RuntimeBody {
    /// Interpreter path; `null` = automatic (newest found).
    pub python: Option<std::path::PathBuf>,
}

pub async fn set_runtime(State(state): State<SharedState>, Json(body): Json<RuntimeBody>) -> ApiResult {
    runtime::select(&state.storage, body.python).await.map_err(ToolError::Invalid)?;
    runtime().await
}

#[derive(Deserialize)]
pub struct VenvBody {
    /// Base interpreter; default = the current one.
    pub python: Option<std::path::PathBuf>,
    /// Allowlisted packages to install (`numpy`, `pandas`, `requests`, `beautifulsoup4`).
    #[serde(default)]
    pub packages: Vec<String>,
}

/// Create the BoardDo tools venv and install packages into it (downloads from PyPI).
pub async fn create_venv(State(state): State<SharedState>, Json(body): Json<VenvBody>) -> ApiResult {
    let result = runtime::create_venv(&state.storage, body.python, body.packages)
        .await
        .map_err(ToolError::Invalid)?;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct RunBody {
    #[serde(default)]
    pub input: Value,
}

pub async fn run(State(state): State<SharedState>, Path(id): Path<String>, Json(body): Json<RunBody>) -> ApiResult {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.run_tool(&id, body.input).await?))
}

pub async fn test_run(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<RunBody>,
) -> ApiResult {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.test_tool(&id, body.input).await?))
}

pub async fn delete(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult {
    let svc = ToolService::new(state.storage.clone());
    Ok(Json(svc.delete_tool(&id).await?))
}
