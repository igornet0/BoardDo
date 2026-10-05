use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use boarddo_shared::{
    ExecutionStatus, RuntimeSnapshot, RuntimeState, RuntimeTriggerInfo, WorkflowRecord,
    WorkflowStatus,
};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::events::{ExecutionFinished, RuntimeEvent, RuntimeEventSource};
use super::lifecycle::{
    after_completion_for_definition, has_runtime_trigger, overlap_for_definition, runtime_triggers,
};
use crate::state::SharedState;
use crate::triggers::schedule::OverlapPolicy;
use crate::triggers::{TriggerContext, fire_execution};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramAccountDemand {
    None,
    /// At least one trigger listens on any account.
    Any,
    Specific(HashSet<Uuid>),
}

#[derive(Debug, Clone)]
struct ScenarioRuntime {
    workflow_id: Uuid,
    workflow_name: String,
    state: RuntimeState,
    triggers: Vec<RuntimeTriggerInfo>,
    in_flight: u32,
    executions_total: u64,
    last_event_at: Option<DateTime<Utc>>,
    last_event_source: Option<String>,
    last_execution_id: Option<Uuid>,
    last_execution_status: Option<String>,
    next_run_at: Option<DateTime<Utc>>,
    on_overlap: OverlapPolicy,
    after_completion_secs: Option<u64>,
    after_completion_node_id: Option<String>,
    pending: Option<RuntimeEvent>,
    started_at: DateTime<Utc>,
}

impl ScenarioRuntime {
    fn snapshot(&self) -> RuntimeSnapshot {
        let state = if self.state == RuntimeState::Executing && self.in_flight == 0 {
            RuntimeState::Waiting
        } else {
            self.state
        };
        RuntimeSnapshot {
            workflow_id: self.workflow_id,
            workflow_name: self.workflow_name.clone(),
            state,
            triggers: self.triggers.clone(),
            executions_total: self.executions_total,
            in_flight: self.in_flight,
            last_event_at: self.last_event_at,
            last_event_source: self.last_event_source.clone(),
            last_execution_id: self.last_execution_id,
            last_execution_status: self.last_execution_status.clone(),
            next_run_at: self.next_run_at,
            started_at: Some(self.started_at),
        }
    }

    fn is_accepting(&self) -> bool {
        self.state.is_accepting()
    }
}

/// In-memory registry of armed scenario runtimes.
#[derive(Clone, Default)]
pub struct RuntimeSupervisor {
    inner: Arc<RwLock<HashMap<Uuid, ScenarioRuntime>>>,
}

impl RuntimeSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self, workflow_id: Uuid, name: &str) -> RuntimeSnapshot {
        self.inner
            .read()
            .get(&workflow_id)
            .map(ScenarioRuntime::snapshot)
            .unwrap_or_else(|| RuntimeSnapshot::stopped(workflow_id, name))
    }

    pub fn list(&self) -> Vec<RuntimeSnapshot> {
        let mut items: Vec<_> = self
            .inner
            .read()
            .values()
            .filter(|rt| rt.state != RuntimeState::Stopped)
            .map(ScenarioRuntime::snapshot)
            .collect();
        items.sort_by(|a, b| a.workflow_name.cmp(&b.workflow_name));
        items
    }

    pub fn is_accepting(&self, workflow_id: Uuid) -> bool {
        self.inner
            .read()
            .get(&workflow_id)
            .is_some_and(ScenarioRuntime::is_accepting)
    }

    /// Which Telegram accounts armed runtimes currently need online.
    pub fn telegram_account_demand(&self) -> TelegramAccountDemand {
        let mut specific = HashSet::new();
        let mut any = false;
        let mut has_tg = false;
        for rt in self.inner.read().values() {
            if !rt.is_accepting() {
                continue;
            }
            for trigger in &rt.triggers {
                if trigger.kind != "telegram.user" {
                    continue;
                }
                has_tg = true;
                match trigger.account_hint.as_deref() {
                    Some(hint) if !hint.is_empty() && hint != "any" => {
                        if let Ok(id) = Uuid::parse_str(hint) {
                            specific.insert(id);
                        } else {
                            any = true;
                        }
                    }
                    _ => any = true,
                }
            }
        }
        if !has_tg {
            TelegramAccountDemand::None
        } else if any {
            TelegramAccountDemand::Any
        } else {
            TelegramAccountDemand::Specific(specific)
        }
    }

    /// Re-arm every `active` workflow after process start.
    pub async fn restore(&self, state: &SharedState) -> anyhow::Result<()> {
        let workflows = state.storage.list_active_workflows().await?;
        for wf in workflows {
            if let Err(err) = self.arm(state, &wf, false).await {
                warn!(workflow_id = %wf.id, error = %err, "runtime.restore_failed");
            }
        }
        info!(count = self.inner.read().len(), "runtime.restored");
        Ok(())
    }

    pub async fn start(
        &self,
        state: &SharedState,
        workflow_id: Uuid,
    ) -> anyhow::Result<RuntimeSnapshot> {
        let wf = state
            .storage
            .get_workflow(workflow_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("workflow not found"))?;
        if !has_runtime_trigger(&wf) {
            anyhow::bail!(
                "runtime requires a Telegram, schedule, or webhook trigger (not a one-shot Run)"
            );
        }
        if wf.status != WorkflowStatus::Active {
            state
                .storage
                .set_workflow_status(workflow_id, WorkflowStatus::Active)
                .await?
                .ok_or_else(|| anyhow::anyhow!("workflow not found"))?;
        }
        let wf = state
            .storage
            .get_workflow(workflow_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("workflow not found"))?;
        let snap = self.arm(state, &wf, true).await?;
        crate::telegram_demand::sync_telegram_accounts(state).await;
        Ok(snap)
    }

    pub async fn stop(
        &self,
        state: &SharedState,
        workflow_id: Uuid,
    ) -> anyhow::Result<RuntimeSnapshot> {
        {
            let mut map = self.inner.write();
            if let Some(rt) = map.get_mut(&workflow_id) {
                rt.state = RuntimeState::Stopping;
                rt.pending = None;
                rt.in_flight = 0;
            }
        }
        if let Some(wf) = state.storage.get_workflow(workflow_id).await? {
            if wf.status == WorkflowStatus::Active {
                let _ = state
                    .storage
                    .set_workflow_status(workflow_id, WorkflowStatus::Draft)
                    .await?;
            }
        }
        let name = self
            .inner
            .write()
            .remove(&workflow_id)
            .map(|rt| rt.workflow_name)
            .unwrap_or_else(|| "workflow".into());
        info!(%workflow_id, "runtime.stopped");
        crate::telegram_demand::sync_telegram_accounts(state).await;
        Ok(RuntimeSnapshot::stopped(workflow_id, name))
    }

    /// Re-arm from the latest saved workflow definition without going idle.
    pub async fn reload(
        &self,
        state: &SharedState,
        workflow_id: Uuid,
    ) -> anyhow::Result<RuntimeSnapshot> {
        let wf = state
            .storage
            .get_workflow(workflow_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("workflow not found"))?;
        if !has_runtime_trigger(&wf) {
            anyhow::bail!(
                "runtime requires a Telegram, schedule, or webhook trigger (not a one-shot Run)"
            );
        }
        let armed = self.inner.read().contains_key(&workflow_id);
        if !armed && wf.status != WorkflowStatus::Active {
            anyhow::bail!("runtime is not running — start it first");
        }
        if wf.status != WorkflowStatus::Active {
            state
                .storage
                .set_workflow_status(workflow_id, WorkflowStatus::Active)
                .await?
                .ok_or_else(|| anyhow::anyhow!("workflow not found"))?;
        }
        let wf = state
            .storage
            .get_workflow(workflow_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("workflow not found"))?;
        let snap = self.arm(state, &wf, false).await?;
        info!(%workflow_id, name = %wf.name, "runtime.reloaded");
        crate::telegram_demand::sync_telegram_accounts(state).await;
        Ok(snap)
    }

    /// Refresh an already-active workflow after graph save, or no-op if draft.
    pub async fn sync_workflow(
        &self,
        state: &SharedState,
        wf: &WorkflowRecord,
    ) -> anyhow::Result<()> {
        match wf.status {
            WorkflowStatus::Active => {
                if has_runtime_trigger(wf) {
                    self.arm(state, wf, false).await?;
                } else {
                    let _ = self.stop(state, wf.id).await?;
                }
            }
            WorkflowStatus::Draft | WorkflowStatus::Archived => {
                if self.inner.read().contains_key(&wf.id) {
                    let _ = self.stop(state, wf.id).await?;
                }
            }
        }
        // stop() already syncs; arm path needs an explicit sync.
        if wf.status == WorkflowStatus::Active && has_runtime_trigger(wf) {
            crate::telegram_demand::sync_telegram_accounts(state).await;
        }
        Ok(())
    }

    pub async fn dispatch(
        &self,
        state: &SharedState,
        event: RuntimeEvent,
    ) -> anyhow::Result<Option<boarddo_shared::RunWorkflowResponse>> {
        let workflow_id = event.workflow_id;
        let skip_or_queue = {
            let mut map = self.inner.write();
            let Some(rt) = map.get_mut(&workflow_id) else {
                debug!(%workflow_id, "runtime.dispatch.ignored_unarmed");
                return Ok(None);
            };
            if !rt.is_accepting() {
                debug!(%workflow_id, state = ?rt.state, "runtime.dispatch.ignored_state");
                return Ok(None);
            }
            let interval = !event.source.allows_concurrent();
            if interval && rt.in_flight > 0 {
                match rt.on_overlap {
                    OverlapPolicy::Skip => {
                        debug!(%workflow_id, "runtime.overlap.skipped");
                        return Ok(None);
                    }
                    OverlapPolicy::Queue => {
                        rt.pending = Some(event.clone());
                        debug!(%workflow_id, "runtime.overlap.queued");
                        return Ok(None);
                    }
                }
            }
            rt.state = RuntimeState::Executing;
            rt.in_flight = rt.in_flight.saturating_add(1);
            rt.executions_total = rt.executions_total.saturating_add(1);
            rt.last_event_at = Some(Utc::now());
            rt.last_event_source = Some(event.source.as_str().into());
            if event.source == RuntimeEventSource::Schedule {
                rt.next_run_at = None;
            }
            false
        };
        let _ = skip_or_queue;

        let wf = match state.storage.get_workflow(workflow_id).await? {
            Some(w) => w,
            None => {
                self.rollback_in_flight(workflow_id);
                anyhow::bail!("workflow not found");
            }
        };

        let ctx = match event.source {
            RuntimeEventSource::TelegramUser => {
                TriggerContext::telegram_user(event.payload.clone())
            }
            RuntimeEventSource::Webhook => TriggerContext::webhook(event.payload.clone()),
            RuntimeEventSource::Schedule => {
                if let Some(node_id) = &event.schedule_node_id {
                    TriggerContext::schedule(node_id)
                } else {
                    TriggerContext {
                        payload: event.payload.clone(),
                        source: crate::triggers::TriggerSource::Schedule,
                        schedule_node_id: None,
                    }
                }
            }
        };

        match fire_execution(state, &wf, ctx).await {
            Ok(resp) => {
                let mut map = self.inner.write();
                if let Some(rt) = map.get_mut(&workflow_id) {
                    rt.last_execution_id = Some(resp.execution_id);
                    rt.last_execution_status = Some("running".into());
                }
                Ok(Some(resp))
            }
            Err(err) => {
                self.rollback_in_flight(workflow_id);
                Err(err)
            }
        }
    }

    pub fn on_execution_finished(&self, finished: ExecutionFinished) -> Option<RuntimeEvent> {
        let mut map = self.inner.write();
        let Some(rt) = map.get_mut(&finished.workflow_id) else {
            return None;
        };
        if rt.in_flight == 0 {
            return None;
        }
        rt.in_flight = rt.in_flight.saturating_sub(1);
        rt.last_execution_id = Some(finished.execution_id);
        rt.last_execution_status = Some(
            match finished.status {
                ExecutionStatus::Completed => "completed",
                ExecutionStatus::Failed => "failed",
                ExecutionStatus::Cancelled => "cancelled",
                ExecutionStatus::Running => "running",
                ExecutionStatus::Pending => "pending",
            }
            .into(),
        );
        if rt.state == RuntimeState::Stopping {
            return None;
        }
        if rt.in_flight == 0 {
            rt.state = RuntimeState::Waiting;
        }
        if finished.status == ExecutionStatus::Completed {
            if let (Some(secs), Some(node_id)) = (
                rt.after_completion_secs,
                rt.after_completion_node_id.clone(),
            ) {
                if rt.is_accepting() {
                    rt.next_run_at = Some(Utc::now() + Duration::seconds(secs as i64));
                    debug!(
                        workflow_id = %rt.workflow_id,
                        node_id,
                        next = ?rt.next_run_at,
                        "runtime.after_completion.scheduled"
                    );
                }
            }
        }
        None
    }

    /// Queued interval events waiting because a previous execution was in flight.
    pub fn take_queued(&self) -> Vec<RuntimeEvent> {
        let mut map = self.inner.write();
        let mut out = Vec::new();
        for rt in map.values_mut() {
            if !rt.is_accepting() || rt.in_flight > 0 {
                continue;
            }
            if let Some(event) = rt.pending.take() {
                out.push(event);
            }
        }
        out
    }

    /// After-completion / interval timers due now.
    pub fn due_after_completion(&self, now: DateTime<Utc>) -> Vec<(Uuid, String)> {
        let mut map = self.inner.write();
        let mut due = Vec::new();
        for rt in map.values_mut() {
            if !rt.is_accepting() {
                continue;
            }
            let Some(at) = rt.next_run_at else {
                continue;
            };
            if at > now {
                continue;
            }
            if rt.after_completion_secs.is_none() {
                continue;
            }
            if rt.in_flight > 0 && rt.on_overlap == OverlapPolicy::Skip {
                continue;
            }
            let Some(node_id) = rt.after_completion_node_id.clone() else {
                continue;
            };
            rt.next_run_at = None;
            due.push((rt.workflow_id, node_id));
        }
        due
    }

    async fn arm(
        &self,
        state: &SharedState,
        wf: &WorkflowRecord,
        log_start: bool,
    ) -> anyhow::Result<RuntimeSnapshot> {
        let executions_total = state.storage.count_executions(wf.id).await.unwrap_or(0);
        let next_run_at = next_run_hint(state, wf).await;
        let (after_node, after_secs) = match after_completion_for_definition(&wf.definition) {
            Some((id, secs)) => (Some(id), Some(secs)),
            None => (None, None),
        };
        let now = Utc::now();
        let next_run_at = if after_secs.is_some() && next_run_at.is_none() {
            Some(now + Duration::seconds(after_secs.unwrap_or(1) as i64))
        } else {
            next_run_at
        };

        let mut map = self.inner.write();
        let existing = map.get(&wf.id);
        let started_at = existing.map(|r| r.started_at).unwrap_or(now);
        let last_event_at = existing.and_then(|r| r.last_event_at);
        let last_event_source = existing.and_then(|r| r.last_event_source.clone());
        let last_execution_id = existing.and_then(|r| r.last_execution_id);
        let last_execution_status = existing.and_then(|r| r.last_execution_status.clone());
        let in_flight = existing.map(|r| r.in_flight).unwrap_or(0);
        let pending = existing.and_then(|r| r.pending.clone());
        let prev_total = existing.map(|r| r.executions_total).unwrap_or(0);

        let state_now = if in_flight > 0 {
            RuntimeState::Executing
        } else {
            RuntimeState::Waiting
        };

        let rt = ScenarioRuntime {
            workflow_id: wf.id,
            workflow_name: wf.name.clone(),
            state: state_now,
            triggers: runtime_triggers(&wf.definition),
            in_flight,
            executions_total: executions_total.max(prev_total),
            last_event_at,
            last_event_source,
            last_execution_id,
            last_execution_status,
            next_run_at,
            on_overlap: overlap_for_definition(&wf.definition),
            after_completion_secs: after_secs,
            after_completion_node_id: after_node,
            pending,
            started_at,
        };
        let snap = rt.snapshot();
        map.insert(wf.id, rt);
        drop(map);
        if log_start {
            info!(workflow_id = %wf.id, name = %wf.name, "runtime.started");
        }
        Ok(snap)
    }

    fn rollback_in_flight(&self, workflow_id: Uuid) {
        let mut map = self.inner.write();
        if let Some(rt) = map.get_mut(&workflow_id) {
            rt.in_flight = rt.in_flight.saturating_sub(1);
            rt.executions_total = rt.executions_total.saturating_sub(1);
            if rt.in_flight == 0 && rt.state == RuntimeState::Executing {
                rt.state = RuntimeState::Waiting;
            }
        }
    }
}

async fn next_run_hint(state: &SharedState, wf: &WorkflowRecord) -> Option<DateTime<Utc>> {
    let schedules = state
        .storage
        .list_schedules_for_workflow(wf.id)
        .await
        .ok()?;
    schedules
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| s.next_run_at)
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::{Edge, Node, Position, WorkflowDefinition, type_ids};
    use serde_json::json;

    fn wf_telegram() -> WorkflowRecord {
        let now = Utc::now();
        WorkflowRecord {
            id: Uuid::now_v7(),
            name: "Echo".into(),
            description: String::new(),
            version: 1,
            status: WorkflowStatus::Active,
            definition: WorkflowDefinition {
                nodes: vec![Node {
                    id: "tg_in".into(),
                    type_id: type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED.into(),
                    category: None,
                    position: Position::default(),
                    config: json!({"account_id": "any", "text_contains": ""}),
                }],
                edges: vec![Edge {
                    id: "e".into(),
                    source: "tg_in".into(),
                    target: "tg_in".into(),
                    source_port: None,
                    target_port: None,
                }],
            },
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn telegram_trigger_kind() {
        let wf = wf_telegram();
        let kinds = runtime_triggers(&wf.definition);
        assert_eq!(kinds.len(), 1);
        assert_eq!(kinds[0].kind, "telegram.user");
        assert!(has_runtime_trigger(&wf));
    }

    #[test]
    fn telegram_demand_none_when_unarmed() {
        let supervisor = RuntimeSupervisor::new();
        assert_eq!(
            supervisor.telegram_account_demand(),
            TelegramAccountDemand::None
        );
    }

    #[test]
    fn telegram_demand_any_from_armed_runtime() {
        let supervisor = RuntimeSupervisor::new();
        let wf = wf_telegram();
        let triggers = runtime_triggers(&wf.definition);
        supervisor.inner.write().insert(
            wf.id,
            ScenarioRuntime {
                workflow_id: wf.id,
                workflow_name: wf.name.clone(),
                state: RuntimeState::Waiting,
                triggers,
                in_flight: 0,
                executions_total: 0,
                last_event_at: None,
                last_event_source: None,
                last_execution_id: None,
                last_execution_status: None,
                next_run_at: None,
                on_overlap: OverlapPolicy::Skip,
                after_completion_secs: None,
                after_completion_node_id: None,
                pending: None,
                started_at: Utc::now(),
            },
        );
        assert_eq!(
            supervisor.telegram_account_demand(),
            TelegramAccountDemand::Any
        );
    }

    #[test]
    fn telegram_demand_specific_account() {
        let supervisor = RuntimeSupervisor::new();
        let account = Uuid::now_v7();
        let mut wf = wf_telegram();
        wf.definition.nodes[0].config = json!({"account_id": account.to_string()});
        let triggers = runtime_triggers(&wf.definition);
        supervisor.inner.write().insert(
            wf.id,
            ScenarioRuntime {
                workflow_id: wf.id,
                workflow_name: wf.name.clone(),
                state: RuntimeState::Waiting,
                triggers,
                in_flight: 0,
                executions_total: 0,
                last_event_at: None,
                last_event_source: None,
                last_execution_id: None,
                last_execution_status: None,
                next_run_at: None,
                on_overlap: OverlapPolicy::Skip,
                after_completion_secs: None,
                after_completion_node_id: None,
                pending: None,
                started_at: Utc::now(),
            },
        );
        match supervisor.telegram_account_demand() {
            TelegramAccountDemand::Specific(ids) => {
                assert_eq!(ids.len(), 1);
                assert!(ids.contains(&account));
            }
            other => panic!("expected Specific, got {other:?}"),
        }
    }

    #[test]
    fn overlap_skip_when_in_flight() {
        let mut rt = ScenarioRuntime {
            workflow_id: Uuid::now_v7(),
            workflow_name: "m".into(),
            state: RuntimeState::Executing,
            triggers: vec![],
            in_flight: 1,
            executions_total: 1,
            last_event_at: None,
            last_event_source: None,
            last_execution_id: None,
            last_execution_status: None,
            next_run_at: None,
            on_overlap: OverlapPolicy::Skip,
            after_completion_secs: None,
            after_completion_node_id: None,
            pending: None,
            started_at: Utc::now(),
        };
        assert!(rt.is_accepting());
        assert_eq!(rt.on_overlap, OverlapPolicy::Skip);
        assert!(rt.in_flight > 0);
        rt.in_flight = 0;
        rt.state = RuntimeState::Waiting;
        assert_eq!(rt.snapshot().state, RuntimeState::Waiting);
    }

    #[test]
    fn finished_returns_to_waiting() {
        let sup = RuntimeSupervisor::new();
        let id = Uuid::now_v7();
        {
            let mut map = sup.inner.write();
            map.insert(
                id,
                ScenarioRuntime {
                    workflow_id: id,
                    workflow_name: "Echo".into(),
                    state: RuntimeState::Executing,
                    triggers: vec![],
                    in_flight: 1,
                    executions_total: 1,
                    last_event_at: None,
                    last_event_source: None,
                    last_execution_id: None,
                    last_execution_status: None,
                    next_run_at: None,
                    on_overlap: OverlapPolicy::Skip,
                    after_completion_secs: Some(5),
                    after_completion_node_id: Some("sched".into()),
                    pending: None,
                    started_at: Utc::now(),
                },
            );
        }
        let pending = sup.on_execution_finished(ExecutionFinished {
            workflow_id: id,
            execution_id: Uuid::now_v7(),
            status: ExecutionStatus::Completed,
        });
        assert!(pending.is_none());
        let snap = sup.snapshot(id, "Echo");
        assert_eq!(snap.state, RuntimeState::Waiting);
        assert_eq!(snap.in_flight, 0);
        assert!(snap.next_run_at.is_some());
    }
}
