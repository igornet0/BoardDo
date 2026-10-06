//! Telegram user-account (TDLib) nodes — separate from Bot API telegram.* nodes.

use std::time::Duration;

use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use tglib::{
    DeleteMessagesRequest, EditMessageRequest, ForwardMessagesRequest, GetChatMembersRequest,
    TelegramChatMember,
};
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

/// Optional message id: absent, empty or null (e.g. an unresolved template) → `None`.
fn optional_message_id(
    ctx: &ExecutionContext,
    raw: Option<&Value>,
) -> Result<Option<TelegramMessageId>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let s = match ctx.resolve_value(raw.clone())? {
        Value::Null => return Ok(None),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s,
        other => other.to_string(),
    };
    let s = s.trim();
    if s.is_empty() || s == "null" || s == "0" {
        return Ok(None);
    }
    parse_message_id(s).map(Some)
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
        let reply_to = optional_message_id(ctx, node.config.get("reply_to_message_id"))?;
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
                reply_to,
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
            "reply_to_message_id": reply_to.map(|m| m.0),
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

pub struct TelegramUserGetChatMembers;

const DEFAULT_MEMBERS_LIMIT: u64 = 200;
/// Membership changes rarely; reuse a fetched list for a day unless configured.
const DEFAULT_MEMBERS_CACHE_TTL_HOURS: f64 = 24.0;

#[async_trait]
impl NodeHandler for TelegramUserGetChatMembers {
    fn type_id(&self) -> &'static str {
        type_ids::TELEGRAM_USER_GET_CHAT_MEMBERS
    }

    /// Read-only, so it also runs in sandbox. With `continue_on_error` a failed
    /// lookup (hidden member list, no access) yields an empty list instead of
    /// failing the run.
    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let continue_on_error = node
            .config
            .get("continue_on_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match fetch_members(node, ctx).await {
            Ok(output) => Ok(NodeOutput::data(output)),
            Err(err) if continue_on_error => {
                tracing::warn!(
                    execution_id = %ctx.execution_id,
                    node_id = %node.id,
                    error = %err,
                    "telegram.user.get_chat_members.skipped"
                );
                Ok(NodeOutput::data(json!({
                    "ok": false,
                    "error": err,
                    "total_count": 0,
                    "count": 0,
                    "members": [],
                    "text": "",
                    "context": "",
                })))
            }
            Err(err) => Err(err),
        }
    }
}

async fn fetch_members(node: &Node, ctx: &ExecutionContext) -> Result<Value, String> {
    let account_id = required_account_id(ctx, node)?;
    let chat_id = parse_chat_id(&resolve_string(ctx, node.config.get("chat_id"), "chat_id")?)?;
    let limit = node
        .config
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MEMBERS_LIMIT)
        .clamp(1, 1000) as u32;
    // `cache_ttl_hours: 0` forces a fresh fetch on every run.
    let ttl_hours = node
        .config
        .get("cache_ttl_hours")
        .and_then(Value::as_f64)
        .unwrap_or(DEFAULT_MEMBERS_CACHE_TTL_HOURS);
    let max_age = (ttl_hours > 0.0).then(|| Duration::from_secs_f64(ttl_hours * 3600.0));
    let result = ctx
        .telegram_user
        .get_chat_members(
            account_id,
            GetChatMembersRequest {
                chat_id,
                limit: Some(limit),
            },
            max_age,
        )
        .await
        .map_err(|e| format!("telegram.user.get_chat_members [{}]: {e}", e.kind()))?;
    let response = result.response;
    let text = members_text(&response.members);
    let context = if text.is_empty() {
        String::new()
    } else {
        format!(
            "Chat members ({} of {}):\n{text}",
            response.members.len(),
            response.total_count
        )
    };
    let members: Vec<Value> = response
        .members
        .iter()
        .map(|m| {
            json!({
                "id": m.user.id.0,
                "name": m.user.display_name(),
                "username": m.user.username,
                "mention": m.user.mention(),
                "status": m.status,
                "is_bot": m.user.is_bot,
            })
        })
        .collect();
    Ok(json!({
        "ok": true,
        "chat_id": chat_id.0,
        "total_count": response.total_count,
        "count": members.len(),
        "members": members,
        "text": text,
        "context": context,
        "cached": result.from_cache,
        "fetched_at": result.fetched_at.to_rfc3339(),
    }))
}

/// One line per member: `- Name (@username) — status [bot]`.
fn members_text(members: &[TelegramChatMember]) -> String {
    members
        .iter()
        .map(|m| {
            let mut line = format!("- {}", m.user.display_name());
            match m.user.mention() {
                Some(mention) if !line.ends_with(&mention) => {
                    line.push_str(&format!(" ({mention})"));
                }
                Some(_) => {}
                None => line.push_str(" (no username)"),
            }
            if m.status != "member" {
                line.push_str(&format!(" — {}", m.status));
            }
            if m.user.is_bot {
                line.push_str(" [bot]");
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tglib::{TelegramUser, TelegramUserId};

    fn member(name: &str, username: Option<&str>, status: &str, is_bot: bool) -> TelegramChatMember {
        TelegramChatMember {
            user: TelegramUser {
                id: TelegramUserId(1),
                first_name: name.into(),
                last_name: None,
                username: username.map(str::to_string),
                is_bot,
            },
            status: status.into(),
        }
    }

    #[test]
    fn formats_member_lines() {
        let text = members_text(&[
            member("Igor", Some("igornet0"), "creator", false),
            member("Anna", None, "member", false),
            member("", Some("helper_bot"), "administrator", true),
        ]);
        assert_eq!(
            text,
            "- Igor (@igornet0) — creator\n- Anna (no username)\n- @helper_bot — administrator [bot]"
        );
    }
}
