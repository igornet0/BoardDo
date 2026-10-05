//! Telegram action nodes — credentials only via ConnectionProvider.

use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::{Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};
use crate::integrations::telegram::TelegramClient;
use crate::secrets::redact_value;

fn resolve_string(
    ctx: &ExecutionContext,
    raw: Option<&Value>,
    field: &str,
) -> Result<String, String> {
    let value = raw.cloned().ok_or_else(|| format!("missing `{field}`"))?;
    match ctx.resolve_value(value)? {
        Value::String(s) => Ok(s),
        other => Ok(other.to_string().trim_matches('"').to_string()),
    }
}

async fn telegram_client(ctx: &ExecutionContext, node: &Node) -> Result<TelegramClient, String> {
    let cid = node
        .config
        .get("connection_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "telegram: missing `connection_id`".to_string())?;

    let resolved = ctx.connections.resolve(cid).await?;
    if resolved.connection_type != "telegram" {
        return Err(format!(
            "telegram: connection `{}` has type `{}`, expected `telegram`",
            resolved.name, resolved.connection_type
        ));
    }

    TelegramClient::from_resolved(&resolved.config, &resolved.credentials).map_err(|e| {
        // Never include token in error messages.
        format!("telegram.{}: {}", e.kind(), e)
    })
}

fn safe_output(method: &str, result: Value) -> Value {
    // Ensure we never echo credentials; Telegram result is message metadata only.
    json!({
        "ok": true,
        "method": method,
        "result": redact_value(&result),
    })
}

pub struct TelegramSendMessage;

#[async_trait]
impl NodeHandler for TelegramSendMessage {
    fn type_id(&self) -> &'static str {
        type_ids::TELEGRAM_SEND_MESSAGE
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        if ctx.is_sandbox() {
            let text = resolve_string(ctx, node.config.get("text"), "text").unwrap_or_default();
            return Ok(NodeOutput::data(json!({
                "ok": true,
                "sandbox": true,
                "skipped": true,
                "text": text,
            })));
        }
        let client = telegram_client(ctx, node).await?;
        let chat_id = resolve_string(ctx, node.config.get("chat_id"), "chat_id")?;
        let text = resolve_string(ctx, node.config.get("text"), "text")?;
        let parse_mode = node
            .config
            .get("parse_mode")
            .and_then(|v| v.as_str())
            .or(Some("markdown"));

        tracing::info!(
            execution_id = %ctx.execution_id,
            node_id = %node.id,
            chat_id = %chat_id,
            "telegram.send_message"
        );

        let result = client
            .send_message(&chat_id, &text, parse_mode)
            .await
            .map_err(|e| format!("telegram.send_message [{}]: {e}", e.kind()))?;

        Ok(NodeOutput::data(safe_output("sendMessage", result)))
    }
}

pub struct TelegramSendPhoto;

#[async_trait]
impl NodeHandler for TelegramSendPhoto {
    fn type_id(&self) -> &'static str {
        type_ids::TELEGRAM_SEND_PHOTO
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let client = telegram_client(ctx, node).await?;
        let chat_id = resolve_string(ctx, node.config.get("chat_id"), "chat_id")?;
        let photo = resolve_string(ctx, node.config.get("photo"), "photo")?;
        let caption = match node.config.get("caption") {
            None | Some(Value::Null) => None,
            Some(v) => Some(resolve_string(ctx, Some(v), "caption")?),
        };

        tracing::info!(
            execution_id = %ctx.execution_id,
            node_id = %node.id,
            chat_id = %chat_id,
            "telegram.send_photo"
        );

        let result = client
            .send_photo(&chat_id, &photo, caption.as_deref())
            .await
            .map_err(|e| format!("telegram.send_photo [{}]: {e}", e.kind()))?;

        Ok(NodeOutput::data(safe_output("sendPhoto", result)))
    }
}

pub struct TelegramSendDocument;

#[async_trait]
impl NodeHandler for TelegramSendDocument {
    fn type_id(&self) -> &'static str {
        type_ids::TELEGRAM_SEND_DOCUMENT
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let client = telegram_client(ctx, node).await?;
        let chat_id = resolve_string(ctx, node.config.get("chat_id"), "chat_id")?;
        let document = resolve_string(ctx, node.config.get("document"), "document")?;
        let caption = match node.config.get("caption") {
            None | Some(Value::Null) => None,
            Some(v) => Some(resolve_string(ctx, Some(v), "caption")?),
        };

        tracing::info!(
            execution_id = %ctx.execution_id,
            node_id = %node.id,
            chat_id = %chat_id,
            "telegram.send_document"
        );

        let result = client
            .send_document(&chat_id, &document, caption.as_deref())
            .await
            .map_err(|e| format!("telegram.send_document [{}]: {e}", e.kind()))?;

        Ok(NodeOutput::data(safe_output("sendDocument", result)))
    }
}
