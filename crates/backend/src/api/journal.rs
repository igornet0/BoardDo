//! Tool call journal API — lets an operator see single-execution calls the
//! runtime refuses to repeat and settle their outcome by hand.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::Deserialize;
use uuid::Uuid;

use crate::state::SharedState;
use crate::storage::{JournalEntry, JournalResolution};

type ApiError = (StatusCode, String);

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/tool-journal", get(list))
        .route("/tool-journal/{key}/resolve", post(resolve))
}

fn internal(e: anyhow::Error) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[derive(Deserialize)]
struct ListQuery {
    run_id: Option<Uuid>,
}

/// With `run_id`: every call of that run. Without: unsettled calls only.
async fn list(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<JournalEntry>>, ApiError> {
    Ok(Json(state.storage.tool_journal_list(q.run_id).await.map_err(internal)?))
}

async fn resolve(
    State(state): State<SharedState>,
    Path(key): Path<String>,
    Json(resolution): Json<JournalResolution>,
) -> Result<StatusCode, ApiError> {
    if let JournalResolution::NoEffect { note } = &resolution
        && note.trim().is_empty()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "`note` must say how it was established that the call had no effect".into(),
        ));
    }
    if state
        .storage
        .tool_journal_resolve(&key, &resolution)
        .await
        .map_err(internal)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::CONFLICT,
            format!("journal entry `{key}` not found or already settled"),
        ))
    }
}
