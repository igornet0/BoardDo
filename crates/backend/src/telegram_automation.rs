//! Route Telegram user events into armed scenario runtimes.

use boarddo_shared::{WorkflowRecord, type_ids};
use boarddo_telegram::{MessageTriggerFilter, matches_message_trigger};
use serde_json::json;
use tglib::TelegramEvent;
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
