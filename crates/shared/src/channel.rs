//! Channel / Stream / Trigger — configuration lifecycle (not runtime execution).
//!
//! Hierarchy:
//! ```text
//! Channel
//!   └── Stream
//!         └── Trigger
//! ```
//!
//! SmartDo persists and validates these definitions. Firing / processing events
//! is out of scope for this module.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// How a channel receives or exposes events (definition only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Internal,
    Webhook,
    Http,
    Telegram,
}

impl Default for ChannelKind {
    fn default() -> Self {
        Self::Internal
    }
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Webhook => "webhook",
            Self::Http => "http",
            Self::Telegram => "telegram",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "internal" => Some(Self::Internal),
            "webhook" => Some(Self::Webhook),
            "http" => Some(Self::Http),
            "telegram" => Some(Self::Telegram),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamDirection {
    Inbound,
    Outbound,
}

impl Default for StreamDirection {
    fn default() -> Self {
        Self::Inbound
    }
}

impl StreamDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "inbound" => Some(Self::Inbound),
            "outbound" => Some(Self::Outbound),
            _ => None,
        }
    }
}

/// Trigger *configuration* kind — does not execute by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Event,
    Webhook,
    Schedule,
    Manual,
}

impl Default for TriggerKind {
    fn default() -> Self {
        Self::Event
    }
}

impl TriggerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Webhook => "webhook",
            Self::Schedule => "schedule",
            Self::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "event" => Some(Self::Event),
            "webhook" => Some(Self::Webhook),
            "schedule" => Some(Self::Schedule),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: ChannelKind,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stream {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub direction: StreamDirection,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    pub id: Uuid,
    pub stream_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: TriggerKind,
    #[serde(default)]
    pub enabled: bool,
    /// Optional link to a BoardDo workflow (binding only — not execution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<Uuid>,
    #[serde(default)]
    pub config: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: ChannelKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChannelRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: ChannelKind,
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStreamRequest {
    pub channel_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub direction: StreamDirection,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStreamRequest {
    pub channel_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub direction: StreamDirection,
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTriggerRequest {
    pub stream_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: TriggerKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTriggerRequest {
    pub stream_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: TriggerKind,
    pub enabled: bool,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    #[serde(default)]
    pub config: Value,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn channel_wire_roundtrip() {
        let ch = Channel {
            id: Uuid::now_v7(),
            name: "ingress".into(),
            description: "main".into(),
            kind: ChannelKind::Webhook,
            enabled: true,
            config: json!({"path": "/hooks/x"}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let v = serde_json::to_value(&ch).unwrap();
        let back: Channel = serde_json::from_value(v).unwrap();
        assert_eq!(back.kind, ChannelKind::Webhook);
        assert_eq!(back.name, "ingress");
    }

    #[test]
    fn stream_trigger_wire_roundtrip() {
        let stream = Stream {
            id: Uuid::now_v7(),
            channel_id: Uuid::now_v7(),
            name: "orders".into(),
            description: String::new(),
            direction: StreamDirection::Inbound,
            enabled: true,
            config: json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let trigger = Trigger {
            id: Uuid::now_v7(),
            stream_id: stream.id,
            name: "on-order".into(),
            description: String::new(),
            kind: TriggerKind::Event,
            enabled: false,
            workflow_id: Some(Uuid::now_v7()),
            config: json!({"event": "order.created"}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let s: Stream = serde_json::from_value(serde_json::to_value(&stream).unwrap()).unwrap();
        let t: Trigger = serde_json::from_value(serde_json::to_value(&trigger).unwrap()).unwrap();
        assert_eq!(s.direction, StreamDirection::Inbound);
        assert_eq!(t.kind, TriggerKind::Event);
        assert!(!t.enabled);
    }
}
