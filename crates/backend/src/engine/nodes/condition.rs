use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::{Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

/// Branches on `true` / `false` ports.
///
/// Either classic compare:
/// `{ "left": "{{amount}}", "operator": ">", "right": 100 }`
///
/// Or expression:
/// `{ "expression": "{{amount > 100}}" }`
pub struct ConditionNode;

#[async_trait]
impl NodeHandler for ConditionNode {
    fn type_id(&self) -> &'static str {
        type_ids::LOGIC_CONDITION
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        if let Some(expr) = node.config.get("expression") {
            let result_val = ctx.resolve_value(expr.clone())?;
            let result = match result_val {
                Value::Bool(b) => b,
                Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
                Value::String(s) => !s.is_empty() && s != "false" && s != "0",
                Value::Null => false,
                other => !other.is_null(),
            };
            let port = if result { "true" } else { "false" };
            return Ok(NodeOutput::branch(
                json!({ "result": result, "expression": expr }),
                port,
            ));
        }

        let left_raw = node
            .config
            .get("left")
            .cloned()
            .ok_or_else(|| "logic.condition: missing `left` or `expression`".to_string())?;
        let operator = node
            .config
            .get("operator")
            .and_then(Value::as_str)
            .ok_or_else(|| "logic.condition: missing `operator`".to_string())?;
        let right_raw = node
            .config
            .get("right")
            .cloned()
            .ok_or_else(|| "logic.condition: missing `right`".to_string())?;

        let left = ctx.resolve_value(left_raw)?;
        let right = ctx.resolve_value(right_raw)?;
        let result = compare(&left, operator, &right)?;
        let port = if result { "true" } else { "false" };

        Ok(NodeOutput::branch(
            json!({
                "result": result,
                "left": left,
                "operator": operator,
                "right": right,
            }),
            port,
        ))
    }
}

fn compare(left: &Value, operator: &str, right: &Value) -> Result<bool, String> {
    match operator {
        "==" | "eq" => Ok(left == right || numeric_eq(left, right)),
        "!=" | "neq" => Ok(left != right && !numeric_eq(left, right)),
        ">" | "gt" => Ok(as_f64(left)? > as_f64(right)?),
        ">=" | "gte" => Ok(as_f64(left)? >= as_f64(right)?),
        "<" | "lt" => Ok(as_f64(left)? < as_f64(right)?),
        "<=" | "lte" => Ok(as_f64(left)? <= as_f64(right)?),
        "contains" => match (left, right) {
            (Value::String(l), Value::String(r)) => Ok(l.contains(r)),
            _ => Err("contains requires string operands".into()),
        },
        other => Err(format!("unknown operator: {other}")),
    }
}

fn numeric_eq(a: &Value, b: &Value) -> bool {
    matches!((as_f64(a), as_f64(b)), (Ok(x), Ok(y)) if (x - y).abs() < f64::EPSILON)
}

fn as_f64(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(n) => n.as_f64().ok_or_else(|| "number out of range".to_string()),
        Value::String(s) => s
            .parse::<f64>()
            .map_err(|_| format!("cannot parse `{s}` as number")),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(format!("cannot compare non-numeric value: {value}")),
    }
}
