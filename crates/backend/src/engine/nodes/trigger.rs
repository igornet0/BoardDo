use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::json;

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

pub struct ManualTrigger;

#[async_trait]
impl NodeHandler for ManualTrigger {
    fn type_id(&self) -> &'static str {
        type_ids::TRIGGER_MANUAL
    }

    async fn execute(
        &self,
        _node: &Node,
        ctx: &mut ExecutionContext,
    ) -> Result<NodeOutput, String> {
        Ok(NodeOutput::data(json!({
            "triggered": true,
            "kind": "manual",
            "trigger": ctx.trigger.clone(),
        })))
    }
}

/// External HTTP webhook entrypoint node.
pub struct WebhookTrigger;

#[async_trait]
impl NodeHandler for WebhookTrigger {
    fn type_id(&self) -> &'static str {
        type_ids::TRIGGER_WEBHOOK
    }

    async fn execute(
        &self,
        _node: &Node,
        ctx: &mut ExecutionContext,
    ) -> Result<NodeOutput, String> {
        Ok(NodeOutput::data(json!({
            "triggered": true,
            "kind": "webhook",
            "body": ctx.trigger.clone(),
        })))
    }
}

/// Cron / interval schedule entrypoint node.
///
/// Config examples:
/// `{ "cron": "*/5 * * * *", "timezone": "UTC" }`
/// `{ "every": { "minutes": 5 } }`
/// `{ "daily_at": "09:00", "timezone": "Europe/Moscow" }`
pub struct ScheduleTrigger;

#[async_trait]
impl NodeHandler for ScheduleTrigger {
    fn type_id(&self) -> &'static str {
        type_ids::TRIGGER_SCHEDULE
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        Ok(NodeOutput::data(json!({
            "triggered": true,
            "kind": "schedule",
            "node_id": node.id,
            "config": node.config.clone(),
            "trigger": ctx.trigger.clone(),
        })))
    }
}
