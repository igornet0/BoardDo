use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::{Value, json};
use tracing::info;

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

/// Logs a message (supports templates / expressions).
pub struct LogNode;

#[async_trait]
impl NodeHandler for LogNode {
    fn type_id(&self) -> &'static str {
        type_ids::DEBUG_LOG
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let raw = node
            .config
            .get("message")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));

        let resolved = ctx.resolve_value(raw)?;
        let message = match resolved {
            Value::String(s) => s,
            other => other.to_string(),
        };

        info!(execution_id = %ctx.execution_id, node_id = %node.id, %message, "debug.log");

        Ok(NodeOutput::data(json!({ "message": message })))
    }
}
