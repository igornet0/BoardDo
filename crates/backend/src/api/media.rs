//! Serve locally stored AI media blobs.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;

use crate::state::SharedState;

pub async fn get_media(
    State(state): State<SharedState>,
    Path(name): Path<String>,
) -> Result<Response, StatusCode> {
    let (path, mime) = state
        .media
        .resolve_public(&name)
        .ok_or(StatusCode::NOT_FOUND)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
