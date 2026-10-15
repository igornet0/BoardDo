use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::{Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

pub struct CustomToolRunNode;

#[async_trait]
impl NodeHandler for CustomToolRunNode {
    fn type_id(&self) -> &'static str {
        type_ids::TOOL_RUN
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let name = node
            .config
            .get("tool_name")
            .or_else(|| node.config.get("name"))
            .and_then(Value::as_str)
            .ok_or("missing tool_name")?;

        let raw_input = node
            .config
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let input = ctx.resolve_value(raw_input)?;

        let result = ctx.custom_tools.run_by_name(name, input).await?;
        // A failed run fails the node (and the scenario) unless the scenario handles it.
        let fail_on_error = node.config.get("fail_on_error").and_then(Value::as_bool).unwrap_or(true);
        if fail_on_error && result.get("status").and_then(Value::as_str) == Some("failed") {
            return Err(format!(
                "tool `{name}` failed: {}: {}",
                result.get("error_type").and_then(Value::as_str).unwrap_or("Error"),
                result.get("message").and_then(Value::as_str).unwrap_or("no message")
            ));
        }
        Ok(NodeOutput::data(result))
    }
}
