use axum::Json;
use boarddo_shared::HealthResponse;

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        service: "smartdo".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}
