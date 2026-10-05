//! Goal → Plan → Workflow (Phase 4 surface; SmartDo does not invent graphs).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::capability::CapabilityBudget;
use super::workflow_contract::WorkflowContract;

/// User-stated objective in natural language (or structured form).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub id: Uuid,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Value>,
    /// Capability ceiling the planner must respect when proposing a graph.
    #[serde(default)]
    pub budget: CapabilityBudget,
    pub created_at: DateTime<Utc>,
}

/// One step in a proposed plan (before / as it becomes canvas nodes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// Suggested node type_id (`ai.analyze`, `composite.…`, `agent.…`, …).
    pub type_id: String,
    #[serde(default)]
    pub config: Value,
    /// Ids of steps that must complete before this one.
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// AI Planner output — never executed directly; user reviews on canvas first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub steps: Vec<PlanStep>,
    /// Materialized contract ready to load into SBDO (optional until accepted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowContract>,
    pub created_at: DateTime<Utc>,
}

/// Status of a human approval gate during execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Pending,
    Approved,
    Rejected,
    TimedOut,
    Edited,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub node_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Snapshot of artifacts / fields under review.
    #[serde(default)]
    pub payload: Value,
    pub status: ApprovalDecision,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
    /// Optional edited payload when status = Edited / Approved with changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_payload: Option<Value>,
}
