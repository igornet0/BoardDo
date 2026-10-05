use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use boarddo_shared::RunWorkflowResponse;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::SharedState;
use crate::triggers::{Trigger, TriggerContext, WebhookTriggerIngress};

/// `POST /api/webhooks/:workflow_id` — fire workflows that contain `trigger.webhook`.
pub async fn webhook(
    State(state): State<SharedState>,
    Path(workflow_id): Path<Uuid>,
    body: Option<Json<Value>>,
) -> Result<Json<RunWorkflowResponse>, (StatusCode, String)> {
    let wf = state
        .storage
        .get_workflow(workflow_id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "workflow not found".into()))?;

    let payload = body.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    if state.runtime.is_accepting(workflow_id) {
        return state
            .runtime
            .dispatch(
                &state,
                crate::runtime::RuntimeEvent {
                    workflow_id,
                    source: crate::runtime::RuntimeEventSource::Webhook,
                    payload,
                    schedule_node_id: None,
                },
            )
            .await
            .and_then(|opt| {
                opt.ok_or_else(|| anyhow::anyhow!("runtime skipped webhook (busy or stopping)"))
            })
            .map(Json)
            .map_err(|err| {
                let msg = err.to_string();
                if msg.contains("no `trigger.webhook`")
                    || msg.starts_with("Workflow validation failed")
                    || msg.contains("skipped")
                {
                    (StatusCode::BAD_REQUEST, msg)
                } else {
                    internal(err)
                }
            });
    }

    let ingress = WebhookTriggerIngress;

    ingress
        .fire(&state, &wf, TriggerContext::webhook(payload))
        .await
        .map(Json)
        .map_err(|err| {
            let msg = err.to_string();
            if msg.contains("no `trigger.webhook`") || msg.starts_with("Workflow validation failed")
            {
                (StatusCode::BAD_REQUEST, msg)
            } else {
                internal(err)
            }
        })
}

fn internal(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = %err, "internal error");
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
