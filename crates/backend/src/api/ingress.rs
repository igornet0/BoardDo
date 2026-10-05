use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use boarddo_shared::{
    Channel, CreateChannelRequest, CreateStreamRequest, CreateTriggerRequest, Stream, Trigger,
    UpdateChannelRequest, UpdateStreamRequest, UpdateTriggerRequest,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::ingress::IngressError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct StreamListQuery {
    pub channel_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct TriggerListQuery {
    pub stream_id: Option<Uuid>,
}

fn map_err(err: IngressError) -> (StatusCode, String) {
    match &err {
        IngressError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
        IngressError::AlreadyExists(msg) => (StatusCode::CONFLICT, msg.clone()),
        IngressError::InvalidDefinition(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
        IngressError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
        IngressError::Database(e) => {
            tracing::error!(error = %e, "ingress database error");
            (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
        }
    }
}

// ── Channels ────────────────────────────────────────────────────────────

pub async fn list_channels(
    State(state): State<SharedState>,
) -> Result<Json<Vec<Channel>>, (StatusCode, String)> {
    state
        .ingress
        .list_channels()
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn create_channel(
    State(state): State<SharedState>,
    Json(req): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<Channel>), (StatusCode, String)> {
    let ch = state.ingress.create_channel(req).await.map_err(map_err)?;
    Ok((StatusCode::CREATED, Json(ch)))
}

pub async fn get_channel(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Channel>, (StatusCode, String)> {
    state
        .ingress
        .get_channel(id)
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn update_channel(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateChannelRequest>,
) -> Result<Json<Channel>, (StatusCode, String)> {
    state
        .ingress
        .update_channel(id, req)
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn delete_channel(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.ingress.delete_channel(id).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Streams ─────────────────────────────────────────────────────────────

pub async fn list_streams(
    State(state): State<SharedState>,
    Query(q): Query<StreamListQuery>,
) -> Result<Json<Vec<Stream>>, (StatusCode, String)> {
    state
        .ingress
        .list_streams(q.channel_id)
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn create_stream(
    State(state): State<SharedState>,
    Json(req): Json<CreateStreamRequest>,
) -> Result<(StatusCode, Json<Stream>), (StatusCode, String)> {
    let s = state.ingress.create_stream(req).await.map_err(map_err)?;
    Ok((StatusCode::CREATED, Json(s)))
}

pub async fn get_stream(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Stream>, (StatusCode, String)> {
    state
        .ingress
        .get_stream(id)
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn update_stream(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateStreamRequest>,
) -> Result<Json<Stream>, (StatusCode, String)> {
    state
        .ingress
        .update_stream(id, req)
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn delete_stream(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.ingress.delete_stream(id).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Triggers ────────────────────────────────────────────────────────────

pub async fn list_triggers(
    State(state): State<SharedState>,
    Query(q): Query<TriggerListQuery>,
) -> Result<Json<Vec<Trigger>>, (StatusCode, String)> {
    state
        .ingress
        .list_triggers(q.stream_id)
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn create_trigger(
    State(state): State<SharedState>,
    Json(req): Json<CreateTriggerRequest>,
) -> Result<(StatusCode, Json<Trigger>), (StatusCode, String)> {
    let t = state.ingress.create_trigger(req).await.map_err(map_err)?;
    Ok((StatusCode::CREATED, Json(t)))
}

pub async fn get_trigger(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Trigger>, (StatusCode, String)> {
    state
        .ingress
        .get_trigger(id)
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn update_trigger(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTriggerRequest>,
) -> Result<Json<Trigger>, (StatusCode, String)> {
    state
        .ingress
        .update_trigger(id, req)
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn delete_trigger(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.ingress.delete_trigger(id).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}
