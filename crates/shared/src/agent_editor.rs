//! Internal AI editor agent (Settings + chat) — not the product `v2::AgentSpec`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{Edge, Node, Position};

/// Editor-agent settings: credentials come from an openai Connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSettings {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<Uuid>,
    /// Optional model override; otherwise connection `default_model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub has_connection: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            connection_id: None,
            model: None,
            has_connection: false,
            connection_name: None,
            base_url: None,
            default_model: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateAgentSettingsRequest {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSettingsTestResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentWorkflowSnapshot {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentSelection {
    #[serde(default)]
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChatRequest {
    pub messages: Vec<AgentChatMessage>,
    pub workflow: AgentWorkflowSnapshot,
    #[serde(default)]
    pub selection: AgentSelection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChatResponse {
    pub message: String,
    #[serde(default)]
    pub ops: Vec<AgentGraphOp>,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AgentUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AgentGraphOp {
    AddNode {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        type_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<Position>,
        #[serde(default)]
        config: Value,
    },
    UpdateNode {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<Position>,
    },
    RemoveNode {
        id: String,
    },
    AddEdge {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        source: String,
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_port: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_port: Option<String>,
    },
    RemoveEdge {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    SetMeta {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}
