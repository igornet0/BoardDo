//! Unified trigger entrypoints for starting workflow executions.
//!
//! Node handlers (`trigger.manual`, `trigger.webhook`, `trigger.schedule`) still
//! run inside the graph. This module is the *ingress* layer that creates
//! executions — Scheduler / Webhook / Manual Run all go through here.

pub mod schedule;

use boarddo_shared::{RunWorkflowResponse, WorkflowRecord, type_ids};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::engine::executor::EventSink;
use crate::runtime::ExecutionFinished;
use crate::state::SharedState;

/// How an execution was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSource {
    Manual,
    Webhook,
    Schedule,
    TelegramUser,
}

impl TriggerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Webhook => "webhook",
            Self::Schedule => "schedule",
            Self::TelegramUser => "telegram.user",
        }
    }

    pub fn node_type_id(self) -> &'static str {
        match self {
            Self::Manual => type_ids::TRIGGER_MANUAL,
            Self::Webhook => type_ids::TRIGGER_WEBHOOK,
            Self::Schedule => type_ids::TRIGGER_SCHEDULE,
            Self::TelegramUser => type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
        }
    }
}

/// Context passed when firing a trigger.
#[derive(Debug, Clone)]
pub struct TriggerContext {
    pub payload: Value,
    pub source: TriggerSource,
    /// Optional schedule node that fired (for schedule source).
    pub schedule_node_id: Option<String>,
}

impl TriggerContext {
    pub fn manual(payload: Value) -> Self {
        Self {
            payload,
            source: TriggerSource::Manual,
            schedule_node_id: None,
        }
    }

    pub fn webhook(payload: Value) -> Self {
        Self {
            payload,
            source: TriggerSource::Webhook,
            schedule_node_id: None,
        }
    }

    pub fn schedule(node_id: impl Into<String>) -> Self {
        Self {
            payload: json!({
                "scheduled_at": chrono::Utc::now().to_rfc3339(),
            }),
            source: TriggerSource::Schedule,
            schedule_node_id: Some(node_id.into()),
        }
    }

    pub fn telegram_user(payload: Value) -> Self {
        Self {
            payload,
            source: TriggerSource::TelegramUser,
            schedule_node_id: None,
        }
    }
}

/// Ingress trigger contract (validate + fire → execution).
#[async_trait::async_trait]
pub trait Trigger: Send + Sync {
    fn source(&self) -> TriggerSource;

    fn validate(&self, wf: &WorkflowRecord) -> Result<(), String>;

    async fn fire(
        &self,
        state: &SharedState,
        wf: &WorkflowRecord,
        ctx: TriggerContext,
    ) -> anyhow::Result<RunWorkflowResponse>;
}

/// Shared fire implementation used by Manual / Webhook / Schedule.
pub async fn fire_execution(
    state: &SharedState,
    wf: &WorkflowRecord,
    ctx: TriggerContext,
) -> anyhow::Result<RunWorkflowResponse> {
    if let Err(errors) = state.engine.validate(&wf.definition) {
        anyhow::bail!("Workflow validation failed: {}", errors.join("; "));
    }

    let expected = ctx.source.node_type_id();
    let has_exact = wf.definition.nodes.iter().any(|n| n.type_id == expected);
    let has_any_trigger = wf
        .definition
        .nodes
        .iter()
        .any(|n| n.type_id.starts_with("trigger."));

    if !has_exact {
        // User-initiated `/run` may target schedule-only graphs — executor falls back.
        if !(ctx.source == TriggerSource::Manual && has_any_trigger) {
            anyhow::bail!("workflow has no `{expected}` node");
        }
    }

    let execution_id = Uuid::now_v7();
    state
        .storage
        .create_execution(
            execution_id,
            wf.id,
            wf.version,
            ctx.source.as_str(),
            &ctx.payload,
        )
        .await?;

    let accepted = boarddo_shared::ExecutionChangelogEntry::accepted(
        ctx.source.as_str(),
        &ctx.payload,
    );
    if let Err(err) = state
        .storage
        .create_execution_changelog(
            execution_id,
            wf.id,
            ctx.source.as_str(),
            &ctx.payload,
            &[accepted.clone()],
        )
        .await
    {
        tracing::error!(error = %err, %execution_id, "failed to create execution changelog");
    }

    let storage = state.storage.clone();
    let engine = state.engine.clone();
    let events = state.events.clone();
    let runtime = state.runtime.clone();
    let definition = wf.definition.clone();
    let workflow_id = wf.id;
    let workflow_version = wf.version;
    let source = ctx.source.as_str().to_string();
    let trigger = ctx.payload;

    tokio::spawn(async move {
        let changelog = std::sync::Arc::new(std::sync::Mutex::new(vec![accepted]));
        let changelog_live = changelog.clone();
        let storage_live = storage.clone();
        let sink: EventSink = Box::new(move |event| {
            let entry = boarddo_shared::ExecutionChangelogEntry::from_event(&event);
            if let Ok(mut guard) = changelog_live.lock() {
                guard.push(entry.clone());
            }
            let storage = storage_live.clone();
            let execution_id = execution_id;
            tokio::spawn(async move {
                if let Err(err) = storage
                    .append_execution_changelog(execution_id, &[entry], None, None, false)
                    .await
                {
                    tracing::warn!(error = %err, %execution_id, "failed to append changelog entry");
                }
            });
            let _ = events.send(event);
        });

        let result = engine
            .execute_with_source(
                execution_id,
                workflow_id,
                workflow_version,
                &definition,
                trigger,
                Some(&source),
                Some(&sink),
            )
            .await;

        let status = result.status;
        let entries = changelog.lock().map(|g| g.clone()).unwrap_or_default();
        if let Err(err) = storage
            .finish_execution_changelog(
                execution_id,
                status,
                result.error.clone(),
                &entries,
            )
            .await
        {
            tracing::error!(error = %err, %execution_id, "failed to finish execution changelog");
        }

        if let Err(err) = storage
            .finish_execution(execution_id, status, result.error, &result.node_executions)
            .await
        {
            tracing::error!(error = %err, %execution_id, "failed to persist execution result");
        }

        let _ = runtime.on_execution_finished(ExecutionFinished {
            workflow_id,
            execution_id,
            status,
        });
    });

    Ok(RunWorkflowResponse {
        execution_id,
        status: boarddo_shared::ExecutionStatus::Running,
    })
}

pub struct ManualTriggerIngress;
pub struct WebhookTriggerIngress;
pub struct ScheduleTriggerIngress;

#[async_trait::async_trait]
impl Trigger for ManualTriggerIngress {
    fn source(&self) -> TriggerSource {
        TriggerSource::Manual
    }

    fn validate(&self, wf: &WorkflowRecord) -> Result<(), String> {
        if wf
            .definition
            .nodes
            .iter()
            .any(|n| n.type_id.starts_with("trigger."))
        {
            Ok(())
        } else {
            Err("workflow has no trigger node".into())
        }
    }

    async fn fire(
        &self,
        state: &SharedState,
        wf: &WorkflowRecord,
        ctx: TriggerContext,
    ) -> anyhow::Result<RunWorkflowResponse> {
        self.validate(wf).map_err(anyhow::Error::msg)?;
        fire_execution(state, wf, ctx).await
    }
}

#[async_trait::async_trait]
impl Trigger for WebhookTriggerIngress {
    fn source(&self) -> TriggerSource {
        TriggerSource::Webhook
    }

    fn validate(&self, wf: &WorkflowRecord) -> Result<(), String> {
        if wf
            .definition
            .nodes
            .iter()
            .any(|n| n.type_id == type_ids::TRIGGER_WEBHOOK)
        {
            Ok(())
        } else {
            Err("workflow has no trigger.webhook node".into())
        }
    }

    async fn fire(
        &self,
        state: &SharedState,
        wf: &WorkflowRecord,
        ctx: TriggerContext,
    ) -> anyhow::Result<RunWorkflowResponse> {
        self.validate(wf).map_err(anyhow::Error::msg)?;
        fire_execution(state, wf, ctx).await
    }
}

#[async_trait::async_trait]
impl Trigger for ScheduleTriggerIngress {
    fn source(&self) -> TriggerSource {
        TriggerSource::Schedule
    }

    fn validate(&self, wf: &WorkflowRecord) -> Result<(), String> {
        if wf
            .definition
            .nodes
            .iter()
            .any(|n| n.type_id == type_ids::TRIGGER_SCHEDULE)
        {
            Ok(())
        } else {
            Err("workflow has no trigger.schedule node".into())
        }
    }

    async fn fire(
        &self,
        state: &SharedState,
        wf: &WorkflowRecord,
        ctx: TriggerContext,
    ) -> anyhow::Result<RunWorkflowResponse> {
        self.validate(wf).map_err(anyhow::Error::msg)?;
        fire_execution(state, wf, ctx).await
    }
}
