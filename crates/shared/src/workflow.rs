use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{Edge, Node, WorkflowDefinition, WorkflowStatus};

/// Request body for creating a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkflowRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

/// Request body for updating a workflow (creates a new version).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWorkflowRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: WorkflowStatus,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Lightweight workflow list item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSummary {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub version: u32,
    pub status: WorkflowStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Validation API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateResponse {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// Run workflow request (optional trigger payload).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunWorkflowRequest {
    #[serde(default)]
    pub trigger: Value,
}

/// Immediate response after starting an execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunWorkflowResponse {
    pub execution_id: Uuid,
    pub status: crate::ExecutionStatus,
}

/// Health check payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
}

/// Stored workflow with timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRecord {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub version: u32,
    pub status: WorkflowStatus,
    pub definition: WorkflowDefinition,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
