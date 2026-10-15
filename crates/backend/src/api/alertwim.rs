//! AlertWim extension API (`/api/extensions/alertwim/*`).

use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::alertwim::{self, AlertMessage};
use crate::state::SharedState;

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/extensions/alertwim", get(status))
        .route("/extensions/alertwim/install", post(install))
        .route("/extensions/alertwim/detect", post(status))
        .route("/extensions/alertwim/launch", post(launch))
        .route("/extensions/alertwim/test", post(test))
}

fn ext() -> Result<std::sync::Arc<alertwim::AlertWim>, (StatusCode, String)> {
    alertwim::get().ok_or((StatusCode::SERVICE_UNAVAILABLE, "AlertWim extension is not initialised".into()))
}

fn bad(err: String) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err)
}

async fn status() -> ApiResult<Value> {
    Ok(Json(ext()?.status().await))
}

async fn install() -> ApiResult<Value> {
    let ext = ext()?;
    ext.start_install().map_err(|e| (StatusCode::CONFLICT, e))?;
    Ok(Json(ext.status().await))
}

async fn launch() -> ApiResult<Value> {
    let ext = ext()?;
    let ep = ext.launch().await.map_err(bad)?;
    let _ = alertwim::send(
        &ep.url,
        &ep.token,
        &AlertMessage { level: "info".into(), message: String::new(), title: None, show: true, focus: true, ..Default::default() },
    )
    .await;
    Ok(Json(ext.status().await))
}

#[derive(Deserialize, Default)]
struct TestBody {
    message: Option<String>,
}

async fn test(body: Option<Json<TestBody>>) -> ApiResult<Value> {
    let ext = ext()?;
    let ep = ext.launch().await.map_err(bad)?;
    let message = body
        .and_then(|Json(b)| b.message)
        .unwrap_or_else(|| "Связь с BoardDo работает ✓".into());
    let resp = alertwim::send(
        &ep.url,
        &ep.token,
        &AlertMessage {
            level: "success".into(),
            title: Some("Тест подключения".into()),
            message,
            source: Some("BoardDo".into()),
            show: true,
            focus: true,
            ..Default::default()
        },
    )
    .await
    .map_err(bad)?;
    Ok(Json(json!({ "delivered": true, "response": resp, "status": ext.status().await })))
}
