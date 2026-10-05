//! Typed ports and JSON-ish schemas for node I/O.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::artifact::ArtifactKind;

/// A named input or output port on a node / workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Primary kind this port carries (may be `Any`).
    pub kind: ArtifactKind,
    #[serde(default)]
    pub required: bool,
    /// Optional JSON Schema fragment for structured validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

impl PortSpec {
    pub fn required(name: impl Into<String>, kind: ArtifactKind) -> Self {
        Self {
            name: name.into(),
            description: None,
            kind,
            required: true,
            schema: None,
            default: None,
        }
    }

    pub fn optional(name: impl Into<String>, kind: ArtifactKind) -> Self {
        Self {
            name: name.into(),
            description: None,
            kind,
            required: false,
            schema: None,
            default: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Full I/O contract for a node or composite workflow.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IoContract {
    #[serde(default)]
    pub inputs: Vec<PortSpec>,
    #[serde(default)]
    pub outputs: Vec<PortSpec>,
}
