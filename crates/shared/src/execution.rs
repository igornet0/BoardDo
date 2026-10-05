use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Lifecycle status of an execution or node run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl Default for ExecutionStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// A single workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version: u32,
    pub status: ExecutionStatus,
    /// How the run was started: `manual`, `webhook`, `schedule`, …
    #[serde(default = "default_trigger_type")]
    pub trigger_type: String,
    pub trigger: Value,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Ordered run timeline (empty in list views; filled on get / changelog API).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changelog: Vec<ExecutionChangelogEntry>,
}

fn default_trigger_type() -> String {
    "manual".into()
}

/// One persisted changelog document — one DB row per scenario run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionChangelog {
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
    pub trigger_type: String,
    pub trigger: Value,
    pub status: ExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub entries: Vec<ExecutionChangelogEntry>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A single step in the run changelog (trigger → nodes → finish).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionChangelogEntry {
    pub at: DateTime<Utc>,
    /// `execution.started`, `node.started`, `node.completed`, `node.failed`, `execution.completed`, …
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ExecutionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
}

impl ExecutionChangelogEntry {
    pub fn from_event(event: &ExecutionEvent) -> Self {
        let at = Utc::now();
        match event {
            ExecutionEvent::ExecutionStarted {
                execution_id,
                workflow_id,
            } => Self {
                at,
                kind: "execution.started".into(),
                node_id: None,
                message: Some(format!(
                    "execution {execution_id} started for workflow {workflow_id}"
                )),
                status: Some(ExecutionStatus::Running),
                error: None,
                duration_ms: None,
                output: None,
            },
            ExecutionEvent::NodeStarted { node_id, .. } => Self {
                at,
                kind: "node.started".into(),
                node_id: Some(node_id.clone()),
                message: Some(format!("node `{node_id}` started")),
                status: Some(ExecutionStatus::Running),
                error: None,
                duration_ms: None,
                output: None,
            },
            ExecutionEvent::NodeCompleted {
                node_id,
                output,
                duration_ms,
                ..
            } => Self {
                at,
                kind: "node.completed".into(),
                node_id: Some(node_id.clone()),
                message: Some(format!("node `{node_id}` completed")),
                status: Some(ExecutionStatus::Completed),
                error: None,
                duration_ms: Some(*duration_ms),
                output: Some(output.clone()),
            },
            ExecutionEvent::NodeFailed {
                node_id, error, ..
            } => Self {
                at,
                kind: "node.failed".into(),
                node_id: Some(node_id.clone()),
                message: Some(format!("node `{node_id}` failed")),
                status: Some(ExecutionStatus::Failed),
                error: Some(error.clone()),
                duration_ms: None,
                output: None,
            },
            ExecutionEvent::ExecutionCompleted {
                status, error, ..
            } => Self {
                at,
                kind: "execution.completed".into(),
                node_id: None,
                message: Some(format!("execution finished as {status:?}")),
                status: Some(*status),
                error: error.clone(),
                duration_ms: None,
                output: None,
            },
        }
    }

    pub fn accepted(trigger_type: &str, trigger: &Value) -> Self {
        Self {
            at: Utc::now(),
            kind: "execution.accepted".into(),
            node_id: None,
            message: Some(format!("run accepted via `{trigger_type}`")),
            status: Some(ExecutionStatus::Running),
            error: None,
            duration_ms: None,
            output: Some(serde_json::json!({ "trigger": trigger })),
        }
    }
}

/// Persisted schedule row for `trigger.schedule` nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSchedule {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub node_id: String,
    pub cron: String,
    pub timezone: String,
    pub enabled: bool,
    pub next_run_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Per-node execution record (foundation for the debugger).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecution {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub node_id: String,
    pub status: ExecutionStatus,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
}

/// Real-time events pushed over WebSocket during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionEvent {
    #[serde(rename = "execution.started")]
    ExecutionStarted {
        execution_id: Uuid,
        workflow_id: Uuid,
    },
    #[serde(rename = "node.started")]
    NodeStarted { execution_id: Uuid, node_id: String },
    #[serde(rename = "node.completed")]
    NodeCompleted {
        execution_id: Uuid,
        node_id: String,
        output: Value,
        duration_ms: u64,
    },
    #[serde(rename = "node.failed")]
    NodeFailed {
        execution_id: Uuid,
        node_id: String,
        error: String,
    },
    #[serde(rename = "execution.completed")]
    ExecutionCompleted {
        execution_id: Uuid,
        status: ExecutionStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn changelog_entry_from_events() {
        let id = Uuid::nil();
        let started = ExecutionEvent::ExecutionStarted {
            execution_id: id,
            workflow_id: id,
        };
        let entry = ExecutionChangelogEntry::from_event(&started);
        assert_eq!(entry.kind, "execution.started");

        let failed = ExecutionEvent::NodeFailed {
            execution_id: id,
            node_id: "ai".into(),
            error: "boom".into(),
        };
        let entry = ExecutionChangelogEntry::from_event(&failed);
        assert_eq!(entry.kind, "node.failed");
        assert_eq!(entry.node_id.as_deref(), Some("ai"));
        assert_eq!(entry.error.as_deref(), Some("boom"));

        let accepted = ExecutionChangelogEntry::accepted("telegram.user", &json!({"text": "hi"}));
        assert_eq!(accepted.kind, "execution.accepted");
    }
}
