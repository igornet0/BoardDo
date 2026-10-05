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
        Ok(NodeOutput::data(result))
    }
}
