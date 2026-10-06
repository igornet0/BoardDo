//! Report failed Telegram-related executions to the account's Saved Messages.
//!
//! The failure is always persisted in BoardDo first; this is a best-effort
//! extra copy so the owner sees it in Telegram. Sends go through the gated
//! gateway, so they are audited and marked BoardDo-originated (no re-trigger).

use boarddo_shared::{ExecutionStatus, NodeExecution, WorkflowDefinition};
use boarddo_telegram::{TelegramEngine, TelegramUserGateway};
use serde_json::Value;
use tglib::{TelegramAccountId, TelegramChatId};
use uuid::Uuid;

pub const ERROR_TAG: &str = "[ERROR BRD]";

/// Telegram caps a text message at 4096 chars; leave room for the header.
const MAX_ERROR_CHARS: usize = 3000;

pub struct FailedExecution<'a> {
    pub workflow_id: Uuid,
    pub workflow_name: &'a str,
    pub execution_id: Uuid,
    pub source: &'a str,
    pub definition: &'a WorkflowDefinition,
    pub trigger: &'a Value,
    pub error: Option<&'a str>,
    pub node_executions: &'a [NodeExecution],
}

struct FailedNode<'a> {
    id: &'a str,
    type_id: &'a str,
    error: Option<&'a str>,
}

pub async fn report_failure(telegram: &TelegramEngine, failed: FailedExecution<'_>) {
    let node = failed_node(&failed);
    if !is_telegram_related(&failed, node.as_ref()) {
        return;
    }
    let Some(account_id) = resolve_account_id(&failed, node.as_ref()) else {
        tracing::debug!(
            execution_id = %failed.execution_id,
            "telegram.error_report.skipped_no_account"
        );
        return;
    };

    let self_chat_id = match telegram.get_account_info(account_id).await {
        Ok(profile) => match profile.telegram_user_id {
            Some(id) => TelegramChatId(id),
            None => {
                tracing::warn!(
                    execution_id = %failed.execution_id,
                    account_id = %account_id,
                    "telegram.error_report.failed: account has no telegram_user_id"
                );
                return;
            }
        },
        Err(err) => {
            tracing::warn!(
                execution_id = %failed.execution_id,
                account_id = %account_id,
                error = %err,
                "telegram.error_report.failed"
            );
            return;
        }
    };

    let text = format_report(&failed, node.as_ref());
    match telegram
        .send_message(
            account_id,
            self_chat_id,
            text,
            None,
            None,
            Some(failed.execution_id),
            Some(failed.workflow_id),
        )
        .await
    {
        Ok(_) => tracing::info!(
            execution_id = %failed.execution_id,
            account_id = %account_id,
            "telegram.error_report.sent"
        ),
        Err(err) => tracing::warn!(
            execution_id = %failed.execution_id,
            account_id = %account_id,
            error = %err,
            "telegram.error_report.failed"
        ),
    }
}

fn failed_node<'a>(failed: &FailedExecution<'a>) -> Option<FailedNode<'a>> {
    let exec = failed
        .node_executions
        .iter()
        .rev()
        .find(|n| n.status == ExecutionStatus::Failed)?;
    let type_id = failed
        .definition
        .nodes
        .iter()
        .find(|n| n.id == exec.node_id)
        .map(|n| n.type_id.as_str())
        .unwrap_or("");
    Some(FailedNode {
        id: &exec.node_id,
        type_id,
        error: exec.error.as_deref(),
    })
}

fn is_telegram_related(failed: &FailedExecution<'_>, node: Option<&FailedNode<'_>>) -> bool {
    failed.source == crate::triggers::TriggerSource::TelegramUser.as_str()
        || node.is_some_and(|n| n.type_id.starts_with("telegram.user."))
}

/// Trigger account first (live Telegram message), then a literal UUID on the
/// failed node. Templated configs like `{{trigger.account_id}}` without a
/// trigger cannot be resolved here and are skipped.
fn resolve_account_id(
    failed: &FailedExecution<'_>,
    node: Option<&FailedNode<'_>>,
) -> Option<TelegramAccountId> {
    let from_trigger = failed.trigger.get("account_id").and_then(Value::as_str);
    let from_node = node.and_then(|n| {
        failed
            .definition
            .nodes
            .iter()
            .find(|d| d.id == n.id)
            .and_then(|d| d.config.get("account_id"))
            .and_then(Value::as_str)
    });
    [from_trigger, from_node]
        .into_iter()
        .flatten()
        .find_map(|raw| Uuid::parse_str(raw.trim()).ok())
        .map(TelegramAccountId)
}

fn format_report(failed: &FailedExecution<'_>, node: Option<&FailedNode<'_>>) -> String {
    let error = node
        .and_then(|n| n.error)
        .or(failed.error)
        .unwrap_or("unknown error");
    let mut out = format!("{ERROR_TAG}\nСценарий: {}\n", failed.workflow_name);
    if let Some(n) = node {
        out.push_str(&format!("Нода: {} ({})\n", n.id, n.type_id));
    }
    out.push_str(&format!("Ошибка: {}\n", truncate(error, MAX_ERROR_CHARS)));
    out.push_str(&format!("Execution: {}", failed.execution_id));
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::Node;
    use serde_json::json;

    fn node(id: &str, type_id: &str, config: Value) -> Node {
        serde_json::from_value(json!({
            "id": id,
            "type_id": type_id,
            "config": config,
            "position": { "x": 0.0, "y": 0.0 },
        }))
        .unwrap()
    }

    fn node_exec(node_id: &str, status: ExecutionStatus, error: Option<&str>) -> NodeExecution {
        NodeExecution {
            id: Uuid::now_v7(),
            execution_id: Uuid::nil(),
            node_id: node_id.into(),
            status,
            input: Value::Null,
            output: None,
            error: error.map(str::to_string),
            started_at: chrono::Utc::now(),
            finished_at: None,
            duration_ms: None,
        }
    }

    fn definition(nodes: Vec<Node>) -> WorkflowDefinition {
        serde_json::from_value(json!({ "nodes": nodes, "edges": [] })).unwrap()
    }

    #[test]
    fn telegram_trigger_failure_reports_trigger_account() {
        let account = Uuid::now_v7();
        let def = definition(vec![
            node("answer", "ai.chat", json!({})),
            node(
                "reply",
                "telegram.user.send_message",
                json!({ "account_id": "{{trigger.account_id}}" }),
            ),
        ]);
        let trigger = json!({ "account_id": account.to_string() });
        let execs = vec![
            node_exec("answer", ExecutionStatus::Completed, None),
            node_exec("reply", ExecutionStatus::Failed, Some("telegram.user: `text` is empty")),
        ];
        let failed = FailedExecution {
            workflow_id: Uuid::nil(),
            workflow_name: "Telegram: классификация → поиск",
            execution_id: Uuid::nil(),
            source: "telegram.user",
            definition: &def,
            trigger: &trigger,
            error: None,
            node_executions: &execs,
        };
        let node = failed_node(&failed);
        assert!(is_telegram_related(&failed, node.as_ref()));
        assert_eq!(
            resolve_account_id(&failed, node.as_ref()).map(|a| a.0),
            Some(account)
        );
        let text = format_report(&failed, node.as_ref());
        assert!(text.starts_with(ERROR_TAG));
        assert!(text.contains("Нода: reply (telegram.user.send_message)"));
        assert!(text.contains("`text` is empty"));
    }

    #[test]
    fn manual_run_uses_literal_node_account() {
        let account = Uuid::now_v7();
        let def = definition(vec![node(
            "send",
            "telegram.user.send_message",
            json!({ "account_id": account.to_string() }),
        )]);
        let trigger = json!({});
        let execs = vec![node_exec("send", ExecutionStatus::Failed, Some("boom"))];
        let failed = FailedExecution {
            workflow_id: Uuid::nil(),
            workflow_name: "wf",
            execution_id: Uuid::nil(),
            source: "manual",
            definition: &def,
            trigger: &trigger,
            error: None,
            node_executions: &execs,
        };
        let node = failed_node(&failed);
        assert!(is_telegram_related(&failed, node.as_ref()));
        assert_eq!(
            resolve_account_id(&failed, node.as_ref()).map(|a| a.0),
            Some(account)
        );
    }

    #[test]
    fn non_telegram_failure_is_ignored() {
        let def = definition(vec![node("http", "http.request", json!({}))]);
        let trigger = json!({});
        let execs = vec![node_exec("http", ExecutionStatus::Failed, Some("500"))];
        let failed = FailedExecution {
            workflow_id: Uuid::nil(),
            workflow_name: "wf",
            execution_id: Uuid::nil(),
            source: "manual",
            definition: &def,
            trigger: &trigger,
            error: None,
            node_executions: &execs,
        };
        let node = failed_node(&failed);
        assert!(!is_telegram_related(&failed, node.as_ref()));
    }

    #[test]
    fn long_errors_are_truncated() {
        let long = "x".repeat(MAX_ERROR_CHARS + 50);
        let out = truncate(&long, MAX_ERROR_CHARS);
        assert_eq!(out.chars().count(), MAX_ERROR_CHARS + 1);
    }
}
