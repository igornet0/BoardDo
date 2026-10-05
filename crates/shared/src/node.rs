use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Canvas position for a node.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// High-level node category (palette / UX grouping).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeCategory {
    Trigger,
    Action,
    Condition,
    Transform,
    Logic,
    Data,
}

/// A single workflow node.
///
/// `type_id` is the registry key, e.g. `"trigger.manual"`, `"logic.condition"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub type_id: String,
    #[serde(default)]
    pub category: Option<NodeCategory>,
    pub position: Position,
    #[serde(default)]
    pub config: Value,
}

/// Directed edge between two nodes, optionally via named ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_port: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_port: Option<String>,
}

/// Workflow lifecycle status in storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Draft,
    Active,
    Archived,
}

impl Default for WorkflowStatus {
    fn default() -> Self {
        Self::Draft
    }
}

/// Full workflow definition (graph + metadata).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub version: u32,
    #[serde(default)]
    pub status: WorkflowStatus,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Immutable snapshot of a workflow graph at a given version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl From<&Workflow> for WorkflowDefinition {
    fn from(wf: &Workflow) -> Self {
        Self {
            nodes: wf.nodes.clone(),
            edges: wf.edges.clone(),
        }
    }
}

/// Known BoardDo node type IDs.
pub mod type_ids {
    pub const TRIGGER_MANUAL: &str = "trigger.manual";
    pub const TRIGGER_WEBHOOK: &str = "trigger.webhook";
    pub const TRIGGER_SCHEDULE: &str = "trigger.schedule";
    pub const DATA_SET: &str = "data.set";
    pub const DATA_TRANSFORM: &str = "data.transform";
    pub const LOGIC_CONDITION: &str = "logic.condition";
    pub const LOGIC_DELAY: &str = "logic.delay";
    pub const DEBUG_LOG: &str = "debug.log";
    pub const HTTP_REQUEST: &str = "http.request";
    pub const WEB_SEARCH: &str = "web.search";
    pub const WEB_OPEN: &str = "web.open";
    pub const WEB_FETCH: &str = "web.fetch";
    pub const WEB_EXTRACT: &str = "web.extract";
    pub const TELEGRAM_SEND_MESSAGE: &str = "telegram.send_message";
    pub const TELEGRAM_SEND_PHOTO: &str = "telegram.send_photo";
    pub const TELEGRAM_SEND_DOCUMENT: &str = "telegram.send_document";
    pub const TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED: &str =
        "trigger.telegram.user.message_received";
    pub const TELEGRAM_USER_SEND_MESSAGE: &str = "telegram.user.send_message";
    pub const TELEGRAM_USER_FORWARD_MESSAGE: &str = "telegram.user.forward_message";
    pub const TELEGRAM_USER_EDIT_MESSAGE: &str = "telegram.user.edit_message";
    pub const TELEGRAM_USER_DELETE_MESSAGES: &str = "telegram.user.delete_messages";
    pub const GITHUB_GET_LATEST_RELEASE: &str = "github.get_latest_release";

    // Phase 3+ — intention / AI / control (handlers land gradually)
    pub const AI_CHAT: &str = "ai.chat";
    pub const AI_GENERATE: &str = "ai.generate";
    pub const AI_ANALYZE: &str = "ai.analyze";
    pub const AI_CLASSIFY: &str = "ai.classify";
    pub const AI_EXTRACT: &str = "ai.extract";
    pub const AI_DECIDE: &str = "ai.decide";
    pub const AI_PLAN: &str = "ai.plan";
    pub const AI_CODE: &str = "ai.code";
    pub const AI_IMAGE: &str = "ai.image";
    pub const AI_AUDIO: &str = "ai.audio";
    pub const AI_VIDEO: &str = "ai.video";
    pub const HUMAN_APPROVAL: &str = "human.approval";
    pub const WORKFLOW_COMPOSITE: &str = "workflow.composite";
    pub const AGENT_RUN: &str = "agent.run";
    pub const TOOL_RUN: &str = "tool.run";

    pub const ALL: &[&str] = &[
        TRIGGER_MANUAL,
        TRIGGER_WEBHOOK,
        TRIGGER_SCHEDULE,
        DATA_SET,
        DATA_TRANSFORM,
        LOGIC_CONDITION,
        LOGIC_DELAY,
        DEBUG_LOG,
        HTTP_REQUEST,
        WEB_SEARCH,
        WEB_OPEN,
        WEB_FETCH,
        WEB_EXTRACT,
        TELEGRAM_SEND_MESSAGE,
        TELEGRAM_SEND_PHOTO,
        TELEGRAM_SEND_DOCUMENT,
        TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
        TELEGRAM_USER_SEND_MESSAGE,
        TELEGRAM_USER_FORWARD_MESSAGE,
        TELEGRAM_USER_EDIT_MESSAGE,
        TELEGRAM_USER_DELETE_MESSAGES,
        GITHUB_GET_LATEST_RELEASE,
        AI_CHAT,
        AI_GENERATE,
        AI_ANALYZE,
        AI_CLASSIFY,
        AI_EXTRACT,
        AI_DECIDE,
        AI_PLAN,
        AI_CODE,
        AI_IMAGE,
        AI_AUDIO,
        AI_VIDEO,
        HUMAN_APPROVAL,
        WORKFLOW_COMPOSITE,
        AGENT_RUN,
        TOOL_RUN,
    ];

    pub fn category_for(type_id: &str) -> crate::NodeCategory {
        match type_id {
            TRIGGER_MANUAL
            | TRIGGER_WEBHOOK
            | TRIGGER_SCHEDULE
            | TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED => crate::NodeCategory::Trigger,
            DATA_SET | DATA_TRANSFORM => crate::NodeCategory::Data,
            LOGIC_CONDITION => crate::NodeCategory::Condition,
            LOGIC_DELAY => crate::NodeCategory::Logic,
            DEBUG_LOG | HTTP_REQUEST | WEB_SEARCH | WEB_OPEN | WEB_FETCH | WEB_EXTRACT => {
                crate::NodeCategory::Action
            }
            TELEGRAM_SEND_MESSAGE
            | TELEGRAM_SEND_PHOTO
            | TELEGRAM_SEND_DOCUMENT
            | TELEGRAM_USER_SEND_MESSAGE
            | TELEGRAM_USER_FORWARD_MESSAGE
            | TELEGRAM_USER_EDIT_MESSAGE
            | TELEGRAM_USER_DELETE_MESSAGES
            | GITHUB_GET_LATEST_RELEASE => crate::NodeCategory::Action,
            AI_CHAT | AI_GENERATE | AI_ANALYZE | AI_CLASSIFY | AI_EXTRACT | AI_DECIDE | AI_PLAN
            | AI_CODE | AI_IMAGE | AI_AUDIO | AI_VIDEO => crate::NodeCategory::Action,
            HUMAN_APPROVAL => crate::NodeCategory::Logic,
            WORKFLOW_COMPOSITE | AGENT_RUN | TOOL_RUN => crate::NodeCategory::Action,
            _ if type_id.starts_with("trigger.") => crate::NodeCategory::Trigger,
            _ if type_id.starts_with("data.") => crate::NodeCategory::Data,
            _ if type_id.starts_with("logic.") => crate::NodeCategory::Logic,
            _ if type_id.starts_with("http.") => crate::NodeCategory::Action,
            _ if type_id.starts_with("web.") => crate::NodeCategory::Action,
            _ if type_id.starts_with("telegram.") => crate::NodeCategory::Action,
            _ if type_id.starts_with("github.") => crate::NodeCategory::Action,
            _ if type_id.starts_with("ai.") => crate::NodeCategory::Action,
            _ if type_id.starts_with("agent.") => crate::NodeCategory::Action,
            _ if type_id.starts_with("composite.") => crate::NodeCategory::Action,
            _ => crate::NodeCategory::Action,
        }
    }
}
