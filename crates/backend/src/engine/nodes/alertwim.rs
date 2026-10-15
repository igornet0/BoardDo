use async_trait::async_trait;
use boarddo_shared::Node;
use serde_json::{Value, json};

use crate::alertwim::{self, AlertMessage};
use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

pub const ALERT: &str = "alertwim.alert";

/// Writes a line to the AlertWim window, starting the app when needed.
///
/// Config: `message` (required), `level` (info | success | warn | error | debug),
/// `title`, `source`, `data`, `show` (default true), `focus`, `launch` (default
/// true), `connection_id` (default: the auto-created AlertWim connection),
/// `fail_on_error` (default false — a missing window must not break a scenario).
pub struct AlertWimAlertNode;

fn text(node: &Node, ctx: &ExecutionContext, key: &str) -> Result<Option<String>, String> {
    let Some(raw) = node.config.get(key).cloned() else {
        return Ok(None);
    };
    Ok(match ctx.resolve_value(raw)? {
        Value::Null => None,
        Value::String(s) => Some(s).filter(|s| !s.trim().is_empty()),
        other => Some(serde_json::to_string_pretty(&other).unwrap_or_default()),
    })
}

fn flag(node: &Node, key: &str, default: bool) -> bool {
    node.config.get(key).and_then(Value::as_bool).unwrap_or(default)
}

async fn deliver(node: &Node, ctx: &ExecutionContext, alert: &AlertMessage) -> Result<Value, String> {
    let launch = flag(node, "launch", true);
    // Explicit connection first (URL + token from the connection registry).
    if let Some(cid) = node.config.get("connection_id").and_then(Value::as_str).filter(|c| !c.is_empty()) {
        let conn = ctx.connections.resolve(cid).await?;
        let url = conn.config.get("base_url").and_then(Value::as_str).unwrap_or_default().to_string();
        let token = conn.credentials.get("token").and_then(Value::as_str).unwrap_or_default().to_string();
        match alertwim::send(&url, &token, alert).await {
            Ok(v) => return Ok(v),
            Err(e) if !launch => return Err(e),
            Err(_) => {}
        }
    }
    let ext = alertwim::get().ok_or("AlertWim extension is not initialised")?;
    let ep = match alertwim::running_endpoint().await {
        Some(ep) => ep,
        None if launch => ext.launch().await?,
        None => return Err("AlertWim is not running (enable `launch` to start it)".into()),
    };
    alertwim::send(&ep.url, &ep.token, alert).await
}

#[async_trait]
impl NodeHandler for AlertWimAlertNode {
    fn type_id(&self) -> &'static str {
        ALERT
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let message = text(node, ctx, "message")?.unwrap_or_default();
        let title = text(node, ctx, "title")?;
        if message.is_empty() && title.is_none() {
            return Err("alertwim.alert: `message` is required".into());
        }
        let data = match node.config.get("data").cloned() {
            None | Some(Value::Null) => None,
            Some(raw) => Some(ctx.resolve_value(raw)?).filter(|v| !v.is_null()),
        };
        let alert = AlertMessage {
            level: text(node, ctx, "level")?.unwrap_or_else(|| "info".into()),
            title,
            message,
            source: text(node, ctx, "source")?.or_else(|| Some("BoardDo".into())),
            data,
            show: flag(node, "show", true),
            focus: flag(node, "focus", false),
        };
        match deliver(node, ctx, &alert).await {
            Ok(resp) => Ok(NodeOutput::data(json!({
                "delivered": true,
                "id": resp.get("id"),
                "level": alert.level,
                "message": alert.message,
            }))),
            Err(err) if !flag(node, "fail_on_error", false) => {
                tracing::warn!(execution_id = %ctx.execution_id, node_id = %node.id, error = %err, "alertwim.alert");
                Ok(NodeOutput::data(json!({ "delivered": false, "error": err, "message": alert.message })))
            }
            Err(err) => Err(format!("alertwim.alert: {err}")),
        }
    }
}
