use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

/// Async delay for testing the execution pipeline.
///
/// Config: `{ "ms": 1000 }`
pub struct DelayNode;

#[async_trait]
impl NodeHandler for DelayNode {
    fn type_id(&self) -> &'static str {
        type_ids::LOGIC_DELAY
    }

    async fn execute(
        &self,
        node: &Node,
        _ctx: &mut ExecutionContext,
    ) -> Result<NodeOutput, String> {
        let ms = node
            .config
            .get("ms")
            .and_then(Value::as_u64)
            .ok_or_else(|| "logic.delay: missing or invalid `ms`".to_string())?;

        sleep(Duration::from_millis(ms)).await;

        Ok(NodeOutput::data(json!({ "delayed_ms": ms })))
    }
}
