//! Re-exports of API DTOs for a stable import path.

pub use crate::agent_editor::{
    AgentChatMessage, AgentChatRequest, AgentChatResponse, AgentGraphOp, AgentSelection,
    AgentSettings, AgentSettingsTestResponse, AgentUsage, AgentWorkflowSnapshot,
    UpdateAgentSettingsRequest,
};
pub use crate::connection::{
    Connection, ConnectionTestResponse, CreateConnectionRequest, UpdateConnectionRequest,
};
pub use crate::execution::{
    Execution, ExecutionChangelog, ExecutionChangelogEntry, ExecutionEvent, ExecutionStatus,
    NodeExecution,
};
pub use crate::runtime::{RuntimeSnapshot, RuntimeState, RuntimeTriggerInfo};
pub use crate::workflow::{
    CreateWorkflowRequest, HealthResponse, RunWorkflowRequest, RunWorkflowResponse,
    UpdateWorkflowRequest, ValidateResponse, WorkflowRecord, WorkflowSummary,
};
