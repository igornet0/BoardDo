//! Headless browser (RustBrowser) status and control.

use axum::Json;
use serde_json::{Value, json};

use crate::integrations::browser;

/// Where the browser runs, whether it is built / up, pid, idle time.
pub async fn status() -> Json<Value> {
    Json(browser::shared().status().await)
}

/// Stop a BoardDo-managed browser now (it restarts on the next render).
pub async fn stop() -> Json<Value> {
    browser::shared().shutdown().await;
    Json(json!({ "ok": true, "status": browser::shared().status().await }))
}
