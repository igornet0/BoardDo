use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Lifecycle of a long-lived scenario runtime (not a single execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    Starting,
    Running,
    Waiting,
    Executing,
    Stopping,
}

impl RuntimeState {
    pub fn is_accepting(self) -> bool {
        matches!(self, Self::Running | Self::Waiting | Self::Executing)
    }
}

/// One armed trigger on a runtime (Telegram / schedule / webhook).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeTriggerInfo {
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
}

/// UI snapshot of a scenario runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub state: RuntimeState,
    pub triggers: Vec<RuntimeTriggerInfo>,
    pub executions_total: u64,
    pub in_flight: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_execution_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_execution_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
}

impl RuntimeSnapshot {
    pub fn stopped(workflow_id: Uuid, workflow_name: impl Into<String>) -> Self {
        Self {
            workflow_id,
            workflow_name: workflow_name.into(),
            state: RuntimeState::Stopped,
            triggers: vec![],
            executions_total: 0,
            in_flight: 0,
            last_event_at: None,
            last_event_source: None,
            last_execution_id: None,
            last_execution_status: None,
            next_run_at: None,
            started_at: None,
        }
    }
}
