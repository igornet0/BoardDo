//! Agent = specialized workflow with tools, memory rules, and a capability budget.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::capability::CapabilityBudget;
use super::node_contract::NestedWorkflowRef;
use super::ports::IoContract;

/// A tool the agent is allowed to call (maps to action / AI node types).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTool {
    /// Node type_id or composite type_id.
    pub type_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional fixed config overrides when the agent invokes this tool.
    #[serde(default)]
    pub config: Value,
}

/// Memory policy for an agent across / within runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMemoryPolicy {
    /// Keep short-term scratch in RunContext.memory[agent_id].
    #[serde(default = "default_true")]
    pub short_term: bool,
    /// Persist summaries keyed by agent + workflow between runs.
    #[serde(default)]
    pub long_term: bool,
    #[serde(default = "default_memory_limit")]
    pub max_entries: u32,
}

fn default_true() -> bool {
    true
}
fn default_memory_limit() -> u32 {
    50
}

impl Default for AgentMemoryPolicy {
    fn default() -> Self {
        Self {
            short_term: true,
            long_term: false,
            max_entries: default_memory_limit(),
        }
    }
}

/// Product-level agent definition. Runtime = nested workflow + this metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub id: Uuid,
    /// Palette type_id, e.g. `agent.marketing`.
    pub type_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Role prompt / operating rules (not executable itself).
    #[serde(default)]
    pub instructions: String,
    pub io: IoContract,
    /// Hard ceiling on what this agent may do.
    pub budget: CapabilityBudget,
    #[serde(default)]
    pub tools: Vec<AgentTool>,
    #[serde(default)]
    pub memory: AgentMemoryPolicy,
    /// Inner BoardDo workflow that implements the agent.
    pub workflow: NestedWorkflowRef,
    /// Whether human approval is required before side-effects outside allowlist.
    #[serde(default)]
    pub require_approval_for_side_effects: bool,
}
