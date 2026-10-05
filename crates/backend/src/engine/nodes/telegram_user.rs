//! Telegram user-account (TDLib) nodes — separate from Bot API telegram.* nodes.

use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use tglib::{DeleteMessagesRequest, EditMessageRequest, ForwardMessagesRequest};
use tglib::{TelegramAccountId, TelegramChatId, TelegramMessageId};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

fn resolve_string(
    ctx: &ExecutionContext,
    raw: Option<&Value>,
    field: &str,
) -> Result<String, String> {
    let value = raw.cloned().ok_or_else(|| format!("missing `{field}`"))?;
    match ctx.resolve_value(value)? {
        Value::String(s) => {
            if s.is_empty() || s == "null" {
                return Err(format!(
                    "telegram.user: `{field}` is empty — for Telegram scenarios use a live message \
                     (trigger.{field}), not Run with an empty trigger"
                ));
            }
            Ok(s)
        }
        Value::Null => Err(format!(
            "telegram.user: `{field}` is null — trigger payload missing `{field}` \
             (write in Telegram, or pass it in the run body)"
        )),
        Value::Number(n) => Ok(n.to_string()),
        other => {
            let s = other.to_string().trim_matches('"').to_string();
            if s.is_empty() || s == "null" {
                return Err(format!("telegram.user: invalid `{field}` `{s}`"));
            }
            Ok(s)
        }
    }
}

fn sandbox_text(ctx: &ExecutionContext, raw: Option<&Value>) -> Result<String, String> {
    match raw {
        Some(v) => match ctx.resolve_value(v.clone())? {
            Value::String(s) => Ok(s),
            Value::Null => Ok(String::new()),
            other => Ok(other.to_string().trim_matches('"').to_string()),
        },
        None => Ok(String::new()),
    }
}

fn sandbox_skip(_ctx: &ExecutionContext, text: impl Into<String>) -> NodeOutput {
    NodeOutput::data(json!({
        "ok": true,
        "sandbox": true,
        "skipped": true,
        "text": text.into(),
    }))
}

fn parse_account_id(raw: &str) -> Result<TelegramAccountId, String> {
    Uuid::parse_str(raw)
        .map(TelegramAccountId)
        .map_err(|_| "telegram.user: invalid `account_id`".into())
}

fn required_account_id(ctx: &ExecutionContext, node: &Node) -> Result<TelegramAccountId, String> {
    let raw = resolve_string(ctx, node.config.get("account_id"), "account_id")?;
    if raw.is_empty() || raw == "any" {
        return Err("telegram.user actions require an explicit account_id".into());
    }
    parse_account_id(&raw)
}

fn parse_chat_id(raw: &str) -> Result<TelegramChatId, String> {
    raw.parse::<i64>()
        .map(TelegramChatId)
        .map_err(|_| format!("telegram.user: invalid chat_id `{raw}`"))
}

fn parse_message_id(raw: &str) -> Result<TelegramMessageId, String> {
    raw.parse::<i64>()
        .map(TelegramMessageId)
        .map_err(|_| format!("telegram.user: invalid message_id `{raw}`"))
}

pub struct TelegramUserMessageReceived;

#[async_trait]
impl NodeHandler for TelegramUserMessageReceived {
    fn type_id(&self) -> &'static str {
        type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        Ok(NodeOutput::data(json!({
            "triggered": true,
            "kind": "telegram.user",
            "config": node.config.clone(),
            "trigger": ctx.trigger.clone(),
            "account_id": ctx.trigger.get("account_id").cloned(),
        })))
    }
}

pub struct TelegramUserSendMessage;

#[async_trait]
impl NodeHandler for TelegramUserSendMessage {
    fn type_id(&self) -> &'static str {
        type_ids::TELEGRAM_USER_SEND_MESSAGE
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        if ctx.is_sandbox() {
            let text = sandbox_text(ctx, node.config.get("text"))?;
            return Ok(sandbox_skip(ctx, text));
        }
        let account_id = required_account_id(ctx, node)?;
        let chat_id = parse_chat_id(&resolve_string(ctx, node.config.get("chat_id"), "chat_id")?)?;
        let text = resolve_string(ctx, node.config.get("text"), "text")?;
        let parse_mode = node
            .config
            .get("parse_mode")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| Some("markdown".into()));
        tracing::info!(
            execution_id = %ctx.execution_id,
            scenario_id = %ctx.workflow_id,
            account_id = %account_id,
            "telegram.action.started"
        );
        let message_id = ctx
            .telegram_user
            .send_message(
                account_id,
                chat_id,
                text,
                parse_mode,
                Some(ctx.execution_id),
                Some(ctx.workflow_id),
            )
            .await
            .map_err(|e| format!("telegram.user.send_message [{}]: {e}", e.kind()))?;
        Ok(NodeOutput::data(json!({
            "ok": true,
            "account_id": account_id.to_string(),
            "chat_id": chat_id.0,
            "message_id": message_id.0,
        })))
    }
}

pub struct TelegramUserForwardMessage;

#[async_trait]
impl NodeHandler for TelegramUserForwardMessage {
    fn type_id(&self) -> &'static str {
        type_ids::TELEGRAM_USER_FORWARD_MESSAGE
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        if ctx.is_sandbox() {
            return Ok(sandbox_skip(ctx, String::new()));
        }
        let account_id = required_account_id(ctx, node)?;
        let from_chat_id = parse_chat_id(&resolve_string(
            ctx,
            node.config.get("from_chat_id"),
            "from_chat_id",
        )?)?;
        let to_chat_id = parse_chat_id(&resolve_string(
            ctx,
            node.config.get("to_chat_id"),
            "to_chat_id",
        )?)?;
        let message_id = parse_message_id(&resolve_string(
            ctx,
            node.config.get("message_id"),
            "message_id",
        )?)?;
        ctx.telegram_user
            .forward_messages(
                account_id,
                ForwardMessagesRequest {
                    from_chat_id,
                    to_chat_id,
                    message_ids: vec![message_id],
                },
                Some(ctx.execution_id),
                Some(ctx.workflow_id),
            )
            .await
            .map_err(|e| format!("telegram.user.forward_message [{}]: {e}", e.kind()))?;
        Ok(NodeOutput::data(json!({ "ok": true })))
    }
}

pub struct TelegramUserEditMessage;

#[async_trait]
impl NodeHandler for TelegramUserEditMessage {
    fn type_id(&self) -> &'static str {
        type_ids::TELEGRAM_USER_EDIT_MESSAGE
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        if ctx.is_sandbox() {
            let text = sandbox_text(ctx, node.config.get("text"))?;
            return Ok(sandbox_skip(ctx, text));
        }
        let account_id = required_account_id(ctx, node)?;
        let chat_id = parse_chat_id(&resolve_string(ctx, node.config.get("chat_id"), "chat_id")?)?;
        let message_id = parse_message_id(&resolve_string(
            ctx,
            node.config.get("message_id"),
            "message_id",
        )?)?;
        let text = resolve_string(ctx, node.config.get("text"), "text")?;
        ctx.telegram_user
            .edit_message(
                account_id,
                EditMessageRequest {
                    chat_id,
                    message_id,
                    text,
                    parse_mode: tglib::ParseMode::parse(
                        node.config
                            .get("parse_mode")
                            .and_then(|v| v.as_str())
                            .or(Some("markdown")),
                    ),
                },
                Some(ctx.execution_id),
                Some(ctx.workflow_id),
            )
            .await
            .map_err(|e| format!("telegram.user.edit_message [{}]: {e}", e.kind()))?;
        Ok(NodeOutput::data(json!({ "ok": true })))
    }
}

pub struct TelegramUserDeleteMessages;

#[async_trait]
impl NodeHandler for TelegramUserDeleteMessages {
    fn type_id(&self) -> &'static str {
        type_ids::TELEGRAM_USER_DELETE_MESSAGES
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        if ctx.is_sandbox() {
            return Ok(sandbox_skip(ctx, String::new()));
        }
        let account_id = required_account_id(ctx, node)?;
        let chat_id = parse_chat_id(&resolve_string(ctx, node.config.get("chat_id"), "chat_id")?)?;
        let message_id = parse_message_id(&resolve_string(
            ctx,
            node.config.get("message_id"),
            "message_id",
        )?)?;
        ctx.telegram_user
            .delete_messages(
                account_id,
                DeleteMessagesRequest {
                    chat_id,
                    message_ids: vec![message_id],
                },
                Some(ctx.execution_id),
                Some(ctx.workflow_id),
            )
            .await
            .map_err(|e| format!("telegram.user.delete_messages [{}]: {e}", e.kind()))?;
        Ok(NodeOutput::data(json!({ "ok": true })))
    }
}
