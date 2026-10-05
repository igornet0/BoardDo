use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use boarddo_shared::{
    Connection, ConnectionTestResponse, CreateConnectionRequest, UpdateConnectionRequest,
};
use uuid::Uuid;

use crate::state::SharedState;

pub async fn list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<Connection>>, (StatusCode, String)> {
    state.connections.list().await.map(Json).map_err(internal)
}

pub async fn create(
    State(state): State<SharedState>,
    Json(req): Json<CreateConnectionRequest>,
) -> Result<(StatusCode, Json<Connection>), (StatusCode, String)> {
    if req.name.trim().is_empty() || req.connection_type.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name and type are required".into()));
    }
    let record = state.connections.create(req).await.map_err(internal)?;
    // record never contains secret — only has_secret flag
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn get(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Connection>, (StatusCode, String)> {
    match state.connections.get(id).await.map_err(internal)? {
        Some(c) => Ok(Json(c)),
        None => Err((StatusCode::NOT_FOUND, "connection not found".into())),
    }
}

pub async fn update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateConnectionRequest>,
) -> Result<Json<Connection>, (StatusCode, String)> {
    match state.connections.update(id, req).await.map_err(internal)? {
        Some(c) => Ok(Json(c)),
        None => Err((StatusCode::NOT_FOUND, "connection not found".into())),
    }
}

pub async fn delete(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let deleted = state.connections.delete(id).await.map_err(internal)?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "connection not found".into()))
    }
}

pub async fn test(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ConnectionTestResponse>, (StatusCode, String)> {
    let (ok, message) = state.connections.test(id).await.map_err(internal)?;
    Ok(Json(ConnectionTestResponse { ok, message }))
}

fn internal(err: anyhow::Error) -> (StatusCode, String) {
    // Never include secret material in error strings from crypto — map generically.
    tracing::error!(error = %err, "connection api error");
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
