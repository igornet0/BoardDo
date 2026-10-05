//! First-class artifacts that flow through BoardDo runs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// High-level kind of data produced / consumed by nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Any,
    Text,
    Markdown,
    Json,
    Number,
    Boolean,
    Url,
    Image,
    Audio,
    Video,
    File,
    Code,
    Dataset,
    Message,
    /// Retrieved research page / payload.
    Document,
    /// Quoted support for a research claim.
    Evidence,
    /// Agent conclusion backed by evidence.
    Claim,
    /// Opaque reference to a stored blob (S3 / local store).
    BlobRef,
}

/// A concrete value produced during execution (debugger / agent memory).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: Uuid,
    pub kind: ArtifactKind,
    /// Human label, e.g. "BTC daily analysis".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Inline payload for small values; large blobs use `uri`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// External / store URI for images, files, datasets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// MIME type when relevant (`image/png`, `text/markdown`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    /// Node that produced this artifact (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub produced_by: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl Artifact {
    pub fn text(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            kind: ArtifactKind::Text,
            name: Some(name.into()),
            value: Some(Value::String(text.into())),
            uri: None,
            mime: Some("text/plain".into()),
            produced_by: None,
            created_at: Utc::now(),
            meta: None,
        }
    }

    pub fn json(name: impl Into<String>, value: Value) -> Self {
        Self {
            id: Uuid::now_v7(),
            kind: ArtifactKind::Json,
            name: Some(name.into()),
            value: Some(value),
            uri: None,
            mime: Some("application/json".into()),
            produced_by: None,
            created_at: Utc::now(),
            meta: None,
        }
    }

    pub fn url(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            kind: ArtifactKind::Url,
            name: Some(name.into()),
            value: Some(Value::String(url.into())),
            uri: None,
            mime: None,
            produced_by: None,
            created_at: Utc::now(),
            meta: None,
        }
    }
}
