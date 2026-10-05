use boarddo_shared::{RunWorkflowResponse, WorkflowRecord};
use serde_json::Value;

use crate::state::SharedState;
use crate::triggers::{ManualTriggerIngress, Trigger, TriggerContext};

/// Manual run entrypoint (used by REST `/run`).
pub async fn spawn_execution(
    state: &SharedState,
    wf: &WorkflowRecord,
    trigger: Value,
    trigger_source: &str,
) -> anyhow::Result<RunWorkflowResponse> {
    let ctx = match trigger_source {
        "webhook" => TriggerContext::webhook(trigger),
        "schedule" => TriggerContext {
            payload: trigger,
            source: crate::triggers::TriggerSource::Schedule,
            schedule_node_id: None,
        },
        _ => TriggerContext::manual(trigger),
    };

    match ctx.source {
        crate::triggers::TriggerSource::Manual => ManualTriggerIngress.fire(state, wf, ctx).await,
        crate::triggers::TriggerSource::Webhook => {
            crate::triggers::WebhookTriggerIngress
                .fire(state, wf, ctx)
                .await
        }
        crate::triggers::TriggerSource::Schedule => {
            crate::triggers::ScheduleTriggerIngress
                .fire(state, wf, ctx)
                .await
        }
        crate::triggers::TriggerSource::TelegramUser => {
            crate::triggers::fire_execution(state, wf, ctx).await
        }
    }
}
