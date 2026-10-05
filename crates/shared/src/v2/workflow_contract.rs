//! Workflow contract — executable graph, optionally publishable as a node.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::capability::CapabilityBudget;
use super::node_contract::NodeInstance;
use super::ports::IoContract;
use crate::{Edge, WorkflowStatus};

/// Metadata when a workflow is published as a reusable composite node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositePublish {
    /// New palette type_id, e.g. `composite.telegram_content_pipeline`.
    pub type_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// Exposed I/O of the composite (mapped from inner trigger/outputs).
    pub io: IoContract,
    /// Max capabilities the composite may use when invoked.
    #[serde(default)]
    pub budget: CapabilityBudget,
}

/// Full workflow contract (Phase 3+).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowContract {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub version: u32,
    #[serde(default)]
    pub status: WorkflowStatus,
    /// Optional goal this workflow was generated / designed for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Typed I/O when used as composite / sub-workflow.
    #[serde(default)]
    pub io: IoContract,
    /// Default capability budget for runs of this workflow.
    #[serde(default)]
    pub budget: CapabilityBudget,
    pub nodes: Vec<NodeInstance>,
    pub edges: Vec<Edge>,
    /// If set, this workflow appears on the palette as one node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_as: Option<CompositePublish>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkflowContract {
    /// Phase 1 wire format projection (lossy — drops v2-only fields).
    pub fn to_phase1_definition(&self) -> crate::WorkflowDefinition {
        crate::WorkflowDefinition {
            nodes: self
                .nodes
                .iter()
                .map(|n| crate::Node {
                    id: n.id.clone(),
                    type_id: n.type_id.clone(),
                    category: None,
                    position: n.position,
                    config: n.config.clone(),
                })
                .collect(),
            edges: self.edges.clone(),
        }
    }
}
