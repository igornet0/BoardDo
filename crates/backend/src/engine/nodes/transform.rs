use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::{Map, Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

/// Builds an object from a mapping of field → template/literal/expression.
pub struct TransformNode;

#[async_trait]
impl NodeHandler for TransformNode {
    fn type_id(&self) -> &'static str {
        type_ids::DATA_TRANSFORM
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let mapping = node
            .config
            .get("mapping")
            .and_then(Value::as_object)
            .ok_or_else(|| "data.transform: missing required parameter `mapping`".to_string())?;

        let mut out = Map::new();
        for (key, raw) in mapping {
            let value = ctx.resolve_value(raw.clone())?;
            out.insert(key.clone(), value);
        }

        for (key, value) in &out {
            ctx.set_variable(key, value.clone());
        }

        Ok(NodeOutput::data(Value::Object(out)))
    }
}

pub fn apply_mapping(
    mapping: &Map<String, Value>,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let mut out = Map::new();
    for (key, raw) in mapping {
        out.insert(key.clone(), ctx.resolve_value(raw.clone())?);
    }
    Ok(json!(out))
}
