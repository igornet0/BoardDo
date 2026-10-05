use boarddo_shared::{RuntimeTriggerInfo, WorkflowDefinition, WorkflowRecord, type_ids};
use serde_json::Value;

use crate::triggers::schedule::{OverlapPolicy, parse_after_completion_secs, parse_overlap};

pub fn runtime_triggers(definition: &WorkflowDefinition) -> Vec<RuntimeTriggerInfo> {
    let mut out = Vec::new();
    for node in &definition.nodes {
        match node.type_id.as_str() {
            type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED => {
                let account = node
                    .config
                    .get("account_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty() && *s != "any")
                    .map(str::to_string);
                let contains = node
                    .config
                    .get("text_contains")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                let ignore_n = chat_id_list_len(
                    node.config
                        .get("ignore_chat_ids")
                        .or_else(|| node.config.get("ignore_chats")),
                );
                let only_n = chat_id_list_len(
                    node.config
                        .get("only_chat_ids")
                        .or_else(|| node.config.get("chat_ids")),
                );
                let mut label = match contains {
                    Some(s) => format!("Telegram message contains `{s}`"),
                    None => "Telegram message".into(),
                };
                if only_n > 0 {
                    label.push_str(&format!(" · only {only_n} chat(s)"));
                }
                if ignore_n > 0 {
                    label.push_str(&format!(" · ignore {ignore_n} chat(s)"));
                }
                out.push(RuntimeTriggerInfo {
                    kind: "telegram.user".into(),
                    label,
                    account_hint: account,
                });
            }
            type_ids::TRIGGER_SCHEDULE => {
                out.push(RuntimeTriggerInfo {
                    kind: "schedule".into(),
                    label: schedule_label(&node.config),
                    account_hint: None,
                });
            }
            type_ids::TRIGGER_WEBHOOK => {
                out.push(RuntimeTriggerInfo {
                    kind: "webhook".into(),
                    label: "Webhook".into(),
                    account_hint: None,
                });
            }
            _ => {}
        }
    }
    out
}

fn chat_id_list_len(raw: Option<&Value>) -> usize {
    match raw {
        Some(Value::Array(items)) => items
            .iter()
            .filter(|v| {
                v.as_i64().is_some()
                    || v.as_u64().is_some()
                    || v.as_str()
                        .is_some_and(|s| !s.trim().is_empty() && s.trim().parse::<i64>().is_ok())
            })
            .count(),
        Some(Value::String(s)) => s
            .split(|c: char| c == ',' || c == ';' || c == '\n' || c.is_whitespace())
            .filter(|p| !p.trim().is_empty() && p.trim().parse::<i64>().is_ok())
            .count(),
        Some(Value::Number(_)) => 1,
        _ => 0,
    }
}

pub fn has_runtime_trigger(wf: &WorkflowRecord) -> bool {
    wf.definition.nodes.iter().any(|n| {
        matches!(
            n.type_id.as_str(),
            type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED
                | type_ids::TRIGGER_SCHEDULE
                | type_ids::TRIGGER_WEBHOOK
        )
    })
}

fn schedule_label(config: &Value) -> String {
    if let Some(secs) = parse_after_completion_secs(config) {
        return format!("After completion ({secs}s)");
    }
    if let Some(every) = config.get("every") {
        if let Some(s) = every.get("seconds").and_then(Value::as_u64) {
            return format!("Every {s}s");
        }
        if let Some(m) = every.get("minutes").and_then(Value::as_u64) {
            return format!("Every {m}m");
        }
        if let Some(h) = every.get("hours").and_then(Value::as_u64) {
            return format!("Every {h}h");
        }
    }
    if let Some(daily) = config.get("daily_at").and_then(Value::as_str) {
        return format!("Daily at {daily}");
    }
    if let Some(cron) = config
        .get("cron")
        .or_else(|| config.get("schedule"))
        .and_then(Value::as_str)
    {
        return format!("Cron {cron}");
    }
    "Schedule".into()
}

pub fn overlap_for_definition(definition: &WorkflowDefinition) -> OverlapPolicy {
    for node in &definition.nodes {
        if node.type_id == type_ids::TRIGGER_SCHEDULE {
            return parse_overlap(&node.config);
        }
    }
    OverlapPolicy::Skip
}

pub fn after_completion_for_definition(definition: &WorkflowDefinition) -> Option<(String, u64)> {
    for node in &definition.nodes {
        if node.type_id != type_ids::TRIGGER_SCHEDULE {
            continue;
        }
        if let Some(secs) = parse_after_completion_secs(&node.config) {
            return Some((node.id.clone(), secs));
        }
    }
    None
}
