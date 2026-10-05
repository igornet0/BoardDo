use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::{Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

/// Sets a variable in the execution context.
///
/// Config: `{ "name": "amount", "value": 150 }` or `"value": "{{trigger.x}}"`
pub struct SetDataNode;

#[async_trait]
impl NodeHandler for SetDataNode {
    fn type_id(&self) -> &'static str {
        type_ids::DATA_SET
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let name = node
            .config
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "data.set: missing required parameter `name`".to_string())?;

        let raw = node
            .config
            .get("value")
            .cloned()
            .ok_or_else(|| "data.set: missing required parameter `value`".to_string())?;

        let value = ctx.resolve_value(raw)?;
        ctx.set_variable(name, value.clone());

        Ok(NodeOutput::data(json!({
            "name": name,
            "value": value,
        })))
    }
}
