//! Run context — variables, artifacts, permissions, scoped memory.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::artifact::Artifact;
use super::capability::CapabilityBudget;
use super::research::ResearchContext;

/// Execution-scoped state. SmartDo mutates this deterministically;
/// AI nodes read/write artifacts through it, never own the control loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunContext {
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version: u32,
    /// Nested depth when running composites (0 = top-level).
    #[serde(default)]
    pub depth: u32,
    #[serde(default)]
    pub trigger: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_source: Option<String>,
    /// Flat variables (Phase 1 compatible).
    #[serde(default)]
    pub variables: Map<String, Value>,
    /// Named artifacts produced during the run.
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    /// Effective capability budget for this run (narrowed by parents).
    #[serde(default)]
    pub budget: CapabilityBudget,
    /// Per-agent short-term memory keyed by agent node id.
    #[serde(default)]
    pub memory: Map<String, Value>,
    /// Accumulated internet-research state for this run, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research: Option<ResearchContext>,
    pub started_at: DateTime<Utc>,
}

impl RunContext {
    pub fn new(
        execution_id: Uuid,
        workflow_id: Uuid,
        workflow_version: u32,
        trigger: Value,
        budget: CapabilityBudget,
    ) -> Self {
        Self {
            execution_id,
            workflow_id,
            workflow_version,
            depth: 0,
            trigger,
            trigger_source: None,
            variables: Map::new(),
            artifacts: Vec::new(),
            budget,
            memory: Map::new(),
            research: None,
            started_at: Utc::now(),
        }
    }

    pub fn set_var(&mut self, name: impl Into<String>, value: Value) {
        self.variables.insert(name.into(), value);
    }

    pub fn push_artifact(&mut self, artifact: Artifact) {
        if let Some(name) = &artifact.name {
            if let Some(v) = &artifact.value {
                self.variables.insert(name.clone(), v.clone());
            } else if let Some(uri) = &artifact.uri {
                self.variables
                    .insert(name.clone(), Value::String(uri.clone()));
            }
        }
        self.artifacts.push(artifact);
    }

    pub fn can(&self, capability: &str) -> bool {
        self.budget.allows(capability)
    }

    pub fn child_scope(&self, child_budget: &CapabilityBudget) -> CapabilityBudget {
        self.budget.narrow(child_budget)
    }
}
