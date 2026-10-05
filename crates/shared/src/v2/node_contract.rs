//! Node contract — intention-level unit on the canvas.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::capability::CapabilityBudget;
use super::ports::IoContract;

/// What kind of unit this node is at the product level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Starts a run (manual, webhook, schedule, goal, …).
    Trigger,
    /// Pure / deterministic data transform.
    Data,
    /// Branching / routing.
    Decision,
    /// Side-effecting tool call (HTTP, Telegram, deploy, …).
    Action,
    /// Model-backed reasoning (generate, analyze, plan, …).
    Ai,
    /// Blocks until a human approves / rejects / edits.
    Approval,
    /// Wait / retry / compensation primitives.
    Control,
    /// Nested workflow exposed as a single node.
    Composite,
    /// Agent = specialized workflow + tools + memory + budget.
    Agent,
    /// Observability / debug.
    Debug,
}

/// How the node fails and recovers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodePolicy {
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub retry: RetryPolicy,
    /// If true, failure does not fail the whole run (edge cases / best-effort).
    #[serde(default)]
    pub continue_on_error: bool,
    /// Optional compensation workflow / node type to run on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensate_with: Option<String>,
}

fn default_timeout_ms() -> u64 {
    60_000
}

impl Default for NodePolicy {
    fn default() -> Self {
        Self {
            timeout_ms: default_timeout_ms(),
            retry: RetryPolicy::default(),
            continue_on_error: false,
            compensate_with: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryPolicy {
    #[serde(default)]
    pub max: u32,
    #[serde(default = "default_backoff")]
    pub backoff_ms: u64,
}

fn default_backoff() -> u64 {
    300
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max: 0,
            backoff_ms: default_backoff(),
        }
    }
}

/// Human-approval specific configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalConfig {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Artifact / variable names shown to the reviewer.
    #[serde(default)]
    pub review_fields: Vec<String>,
    /// Auto-reject after this many ms (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default = "default_approve_port")]
    pub approve_port: String,
    #[serde(default = "default_reject_port")]
    pub reject_port: String,
}

fn default_approve_port() -> String {
    "approved".into()
}
fn default_reject_port() -> String {
    "rejected".into()
}

/// How a composite / agent node binds to an inner workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NestedWorkflowRef {
    /// Inline graph (small composites).
    Inline { workflow_id: Uuid },
    /// Reference to a saved workflow version in storage.
    Stored {
        workflow_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<u32>,
    },
}

/// Full contract for a palette / canvas node type OR an instance template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeContract {
    /// Registry key, e.g. `ai.analyze`, `composite.telegram_pipeline`, `agent.marketing`.
    pub type_id: String,
    pub kind: NodeKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Intention summary shown on canvas (not the low-level op).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    pub io: IoContract,
    /// Capabilities this node *requires* to run.
    #[serde(default)]
    pub requires: CapabilityBudget,
    /// Capabilities this node *grants* to its children (composites / agents).
    #[serde(default)]
    pub grants: CapabilityBudget,
    #[serde(default)]
    pub policy: NodePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<NestedWorkflowRef>,
    /// Default config seed when dropped from palette.
    #[serde(default)]
    pub default_config: Value,
    /// UI category for palette grouping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// Instance of a node on a canvas (extends Phase 1 `Node` conceptually).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeInstance {
    pub id: String,
    pub type_id: String,
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    pub position: crate::Position,
    #[serde(default)]
    pub config: Value,
    /// Optional per-instance capability narrowing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<CapabilityBudget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<NestedWorkflowRef>,
}
