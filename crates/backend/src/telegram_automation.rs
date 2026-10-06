//! Route Telegram user events into armed scenario runtimes.

use boarddo_shared::{WorkflowRecord, type_ids};
use boarddo_telegram::{MessageTriggerFilter, TelegramUserGateway, matches_message_trigger};
use serde_json::{Value, json};
use tglib::{TelegramEvent, TelegramMessageReceived, TelegramUser, TelegramUserId};
use uuid::Uuid;

use crate::runtime::{RuntimeEvent, RuntimeEventSource};
use crate::state::SharedState;

pub fn spawn(state: SharedState) {
    tokio::spawn(async move {
        let mut rx = state.telegram.subscribe_events();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Err(err) = handle_event(&state, event).await {
                        tracing::error!(error = %err, "telegram.automation.failed");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "telegram.automation.lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn handle_event(state: &SharedState, event: TelegramEvent) -> anyhow::Result<()> {
    let TelegramEvent::MessageReceived(payload) = &event else {
        return Ok(());
    };

    maybe_record_goal_leads(state, payload).await;

    // No armed Telegram runtimes → do not evaluate scenario filters / log no_match.
    if matches!(
        state.runtime.telegram_account_demand(),
        crate::runtime::TelegramAccountDemand::None
    ) {
        return Ok(());
    }

    let from_boarddo = state.telegram.is_boarddo_originated(payload);
    let workflows = state.storage.list_active_workflows().await?;

    let mut targets: Vec<&WorkflowRecord> = Vec::new();
    let mut ignored_by_chat = false;
    for wf in &workflows {
        if !state.runtime.is_accepting(wf.id) {
            continue;
        }
        let mut matched = false;
        for node in &wf.definition.nodes {
            if node.type_id != type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED {
                continue;
            }
            let filter = MessageTriggerFilter::from_node_config(&node.config);
            if matches_message_trigger(&filter, payload, from_boarddo) {
                matched = true;
                break;
            }
            if filter_rejects_only_by_ignore(&filter, payload, from_boarddo) {
                ignored_by_chat = true;
            }
        }
        if matched {
            targets.push(wf);
        }
    }

    if targets.is_empty() {
        if ignored_by_chat {
            tracing::debug!(
                account_id = %payload.account_id,
                chat_id = %payload.chat_id,
                message_id = %payload.message_id,
                "telegram.scenario.ignored_chat"
            );
        } else {
            tracing::debug!(
                account_id = %payload.account_id,
                chat_id = %payload.chat_id,
                message_id = %payload.message_id,
                is_outgoing = payload.is_outgoing,
                from_boarddo,
                text = payload.text.as_deref().unwrap_or(""),
                active_workflows = workflows.len(),
                "telegram.scenario.no_match"
            );
        }
        return Ok(());
    }

    // Claim only after we know at least one armed runtime wants the event,
    // so messages that arrive while Stopped are not permanently swallowed.
    if !state.telegram.claim_event(&event).await? {
        tracing::debug!(account_id = %payload.account_id, "telegram.automation.duplicate");
        return Ok(());
    }

    let (reply_to, reply_context) = reply_payload(state, payload).await;
    let people = people_payload(state, payload).await;

    for wf in targets {
        let correlation_id = Uuid::now_v7();
        let trigger_payload = json!({
            "account_id": payload.account_id.to_string(),
            "chat_id": payload.chat_id.0,
            "message_id": payload.message_id.0,
            "sender_id": payload.sender_id.map(|s| s.0),
            "text": payload.text,
            "is_outgoing": payload.is_outgoing,
            "timestamp": payload.timestamp.to_rfc3339(),
            "correlation_id": correlation_id.to_string(),
            "event_type": "message_received",
            "reply_to": reply_to,
            "reply_context": reply_context,
            "sender": people.sender,
            "me": people.me,
            "sender_context": people.context,
        });
        tracing::info!(
            account_id = %payload.account_id,
            chat_id = %payload.chat_id,
            scenario_id = %wf.id,
            correlation_id = %correlation_id,
            is_outgoing = payload.is_outgoing,
            "telegram.scenario.triggered"
        );
        match state
            .runtime
            .dispatch(
                state,
                RuntimeEvent {
                    workflow_id: wf.id,
                    source: RuntimeEventSource::TelegramUser,
                    payload: trigger_payload,
                    schedule_node_id: None,
                },
            )
            .await
        {
            Ok(Some(resp)) => {
                let _ = state
                    .telegram
                    .record_scenario_execution(
                        resp.execution_id,
                        wf.id,
                        payload.account_id,
                        payload.event_key(),
                        "started",
                        None,
                    )
                    .await;
                let _ = state
                    .telegram
                    .audit
                    .record(
                        payload.account_id,
                        Some(wf.id),
                        Some(resp.execution_id),
                        "message_received",
                        "scenario_triggered",
                        Some(format!("chat:{}", payload.chat_id)),
                        tglib::TelegramAuditStatus::Success,
                        None,
                    )
                    .await;
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    account_id = %payload.account_id,
                    scenario_id = %wf.id,
                    error = %err,
                    "telegram.scenario.failed"
                );
            }
        }
    }
    Ok(())
}

struct People {
    sender: Value,
    me: Value,
    context: String,
}

/// Who wrote the message and who owns the account, so an agent can tell the
/// owner from other chat participants and address people by `@username`.
///
/// `trigger.sender` / `trigger.me` are null when unknown; `trigger.sender_context`
/// is a prompt-ready block (empty when nothing is known). Lookups are
/// best-effort and never block the run.
async fn people_payload(state: &SharedState, payload: &TelegramMessageReceived) -> People {
    let me = match state.telegram.get_account_info(payload.account_id).await {
        Ok(profile) => profile.telegram_user_id.map(|id| TelegramUser {
            id: TelegramUserId(id),
            first_name: profile.display_name.unwrap_or_default(),
            last_name: None,
            username: profile.username,
            is_bot: false,
        }),
        Err(err) => {
            tracing::warn!(account_id = %payload.account_id, error = %err, "telegram.me.fetch_failed");
            None
        }
    };
    let sender_is_me = match (&me, payload.sender_id) {
        (Some(me), Some(sender)) => me.id == sender,
        (Some(_), None) => payload.is_outgoing,
        _ => false,
    };
    let sender = if sender_is_me {
        me.clone()
    } else if let Some(sender_id) = payload.sender_id {
        match state.telegram.get_user(payload.account_id, sender_id).await {
            Ok(user) => Some(user),
            Err(err) => {
                tracing::warn!(
                    account_id = %payload.account_id,
                    sender_id = %sender_id.0,
                    error = %err,
                    "telegram.sender.fetch_failed"
                );
                None
            }
        }
    } else {
        None
    };
    People {
        sender: match &sender {
            Some(user) => user_json(user, Some(sender_is_me)),
            None => payload
                .sender_id
                .map(|id| json!({ "id": id.0, "is_self": sender_is_me }))
                .unwrap_or(Value::Null),
        },
        me: me.as_ref().map(|u| user_json(u, None)).unwrap_or(Value::Null),
        context: sender_context(sender.as_ref(), me.as_ref(), sender_is_me),
    }
}

fn user_json(user: &TelegramUser, is_self: Option<bool>) -> Value {
    let mut v = json!({
        "id": user.id.0,
        "name": user.display_name(),
        "first_name": user.first_name,
        "last_name": user.last_name,
        "username": user.username,
        "mention": user.mention(),
        "is_bot": user.is_bot,
    });
    if let Some(is_self) = is_self {
        v["is_self"] = json!(is_self);
    }
    v
}

fn describe_user(user: &TelegramUser) -> String {
    match user.mention() {
        Some(m) if user.display_name() != m => format!("{} ({m}, id {})", user.display_name(), user.id.0),
        Some(m) => format!("{m} (id {})", user.id.0),
        None => format!("{} (id {}, no username)", user.display_name(), user.id.0),
    }
}

fn sender_context(sender: Option<&TelegramUser>, me: Option<&TelegramUser>, sender_is_me: bool) -> String {
    let owner = me.map(describe_user);
    match (sender, owner) {
        (Some(sender), _) if sender_is_me => format!(
            "Message author: {} — the owner of this Telegram account; you act on their behalf.",
            describe_user(sender)
        ),
        (Some(sender), Some(owner)) => format!(
            "Message author: {} — a chat participant, not the account owner.\nAccount owner (you act on their behalf): {owner}.",
            describe_user(sender)
        ),
        (Some(sender), None) => format!("Message author: {}.", describe_user(sender)),
        (None, Some(owner)) => format!("Account owner (you act on their behalf): {owner}."),
        (None, None) => String::new(),
    }
}

/// Resolve the replied-to message so scenarios see what `@ai …` refers to.
///
/// Returns `trigger.reply_to` (null when the message is not a reply) and
/// `trigger.reply_context` — a prompt-ready block, empty when there is nothing
/// to add, so `{{trigger.text}}{{trigger.reply_context}}` works either way.
/// A failed fetch keeps the ids and quote; the run still starts.
async fn reply_payload(state: &SharedState, payload: &TelegramMessageReceived) -> (Value, String) {
    let Some(reply) = &payload.reply_to else {
        return (Value::Null, String::new());
    };
    let fetched = match state
        .telegram
        .get_message(payload.account_id, reply.chat_id, reply.message_id)
        .await
    {
        Ok(message) => Some(message),
        Err(err) => {
            tracing::warn!(
                account_id = %payload.account_id,
                chat_id = %reply.chat_id,
                message_id = %reply.message_id,
                error = %err,
                "telegram.reply_to.fetch_failed"
            );
            None
        }
    };
    let text = fetched
        .as_ref()
        .and_then(|m| m.text.as_deref())
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let content_kind = fetched.as_ref().map(|m| m.content_kind.as_str());
    let reply_to = json!({
        "chat_id": reply.chat_id.0,
        "message_id": reply.message_id.0,
        "sender_id": fetched.as_ref().and_then(|m| m.sender_id).map(|s| s.0),
        "text": text,
        "quote": reply.quote,
        "content_kind": content_kind,
    });
    (
        reply_to,
        reply_context(text, reply.quote.as_deref(), content_kind),
    )
}

fn reply_context(text: Option<&str>, quote: Option<&str>, content_kind: Option<&str>) -> String {
    let mut out = String::new();
    match (text, content_kind) {
        (Some(text), _) => {
            out.push_str("\n\nReplied-to message:\n");
            out.push_str(text);
        }
        (None, Some(kind)) => {
            out.push_str(&format!("\n\nReplied-to message: [{kind} without text]"));
        }
        (None, None) => {}
    }
    if let Some(quote) = quote.map(str::trim).filter(|q| !q.is_empty()) {
        out.push_str("\n\nQuoted fragment:\n");
        out.push_str(quote);
    }
    out
}

/// True when the only reason the filter rejects is the ignore-chat block-list.
fn filter_rejects_only_by_ignore(
    filter: &MessageTriggerFilter,
    event: &tglib::TelegramMessageReceived,
    from_boarddo: bool,
) -> bool {
    if filter.ignore_outgoing && from_boarddo {
        return false;
    }
    if let Some(account_id) = filter.account_id {
        if account_id != event.account_id {
            return false;
        }
    }
    if !filter.ignore_chat_ids.iter().any(|id| *id == event.chat_id) {
        return false;
    }
    // It is on the ignore list — confirm it would otherwise match.
    let mut open = filter.clone();
    open.ignore_chat_ids.clear();
    matches_message_trigger(&open, event, from_boarddo)
}

async fn maybe_record_goal_leads(
    state: &SharedState,
    payload: &tglib::TelegramMessageReceived,
) {
    if payload.is_outgoing {
        return;
    }
    let text = payload.text.as_deref().unwrap_or("");
    let text_lc = text.to_lowercase();
    const MARKERS: &[&str] = &[
        "interested",
        "хочу",
        "count me",
        "sign me",
        "+1",
        "демо",
        "demo",
        "let's try",
        "lets try",
        "price",
        "pricing",
        "how to buy",
    ];
    let looks_relevant = MARKERS.iter().any(|m| text_lc.contains(m));
    let Ok(specs) = state.storage.list_goal_specs().await else {
        return;
    };
    for spec in specs {
        let Ok(Some(run)) = state.storage.latest_run_for_goal(spec.id).await else {
            continue;
        };
        if !run.status.is_active() {
            continue;
        }

        if spec.agent_type_id == "agent.marketing" {
            if !looks_relevant && text.trim().is_empty() {
                continue;
            }
            let chat_id = payload.chat_id.to_string();
            let account_id = Some(payload.account_id.to_string());
            let experiments = state
                .storage
                .list_experiments(run.id)
                .await
                .unwrap_or_default();
            let experiment_id = experiments.first().map(|e| e.id);
            if let Err(err) = crate::agent_runtime::process_inbound(
                state,
                &spec,
                &run,
                &chat_id,
                account_id.as_deref(),
                text,
                experiment_id,
                false,
            )
            .await
            {
                tracing::warn!(error = %err, goal_id = %spec.id, "marketing.inbound.failed");
            }
            continue;
        }

        if spec.metric != "leads_interested" || !looks_relevant {
            continue;
        }
        if let Err(err) = crate::agent_runtime::record_lead(state, &spec, 1.0).await {
            tracing::warn!(error = %err, goal_id = %spec.id, "goal.lead.failed");
        } else {
            tracing::info!(goal_id = %spec.id, "goal.lead.interested");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{reply_context, sender_context};
    use tglib::{TelegramUser, TelegramUserId};

    fn user(id: i64, name: &str, username: Option<&str>) -> TelegramUser {
        TelegramUser {
            id: TelegramUserId(id),
            first_name: name.into(),
            last_name: None,
            username: username.map(str::to_string),
            is_bot: false,
        }
    }

    #[test]
    fn sender_context_owner() {
        let me = user(1, "Igor", Some("igornet0"));
        assert_eq!(
            sender_context(Some(&me), Some(&me), true),
            "Message author: Igor (@igornet0, id 1) — the owner of this Telegram account; you act on their behalf."
        );
    }

    #[test]
    fn sender_context_other_participant() {
        let me = user(1, "Igor", Some("igornet0"));
        let anna = user(2, "Anna", None);
        assert_eq!(
            sender_context(Some(&anna), Some(&me), false),
            "Message author: Anna (id 2, no username) — a chat participant, not the account owner.\nAccount owner (you act on their behalf): Igor (@igornet0, id 1)."
        );
        assert_eq!(sender_context(None, None, false), "");
    }

    #[test]
    fn reply_context_with_text_and_quote() {
        let ctx = reply_context(Some("original"), Some("frag"), Some("text"));
        assert_eq!(
            ctx,
            "\n\nReplied-to message:\noriginal\n\nQuoted fragment:\nfrag"
        );
    }

    #[test]
    fn reply_context_media_without_caption() {
        assert_eq!(
            reply_context(None, None, Some("photo")),
            "\n\nReplied-to message: [photo without text]"
        );
    }

    #[test]
    fn reply_context_empty_when_nothing_known() {
        assert_eq!(reply_context(None, None, None), "");
        assert_eq!(reply_context(None, Some("  "), None), "");
    }
}
