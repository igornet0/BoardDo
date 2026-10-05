//! Connection registry DTOs (safe for API — never include secret material).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Multimodal / chat capabilities declared on an openai connection model entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AiModelCapabilities {
    #[serde(default = "default_true")]
    pub chat: bool,
    #[serde(default)]
    pub image_generate: bool,
    #[serde(default)]
    pub image_edit: bool,
    #[serde(default)]
    pub video_generate: bool,
    #[serde(default)]
    pub audio_generate: bool,
    #[serde(default)]
    pub audio_transcribe: bool,
}

impl AiModelCapabilities {
    pub fn chat_only() -> Self {
        Self {
            chat: true,
            ..Self::default()
        }
    }

    pub fn supports(&self, capability: &str) -> bool {
        match capability {
            "chat" => self.chat,
            "image_generate" => self.image_generate,
            "image_edit" => self.image_edit,
            "video_generate" => self.video_generate,
            "audio_generate" => self.audio_generate,
            "audio_transcribe" => self.audio_transcribe,
            _ => false,
        }
    }
}

/// One model listed on an openai / ai connection `config.models[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiModelEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub capabilities: AiModelCapabilities,
}

impl AiModelEntry {
    pub fn chat(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: None,
            capabilities: AiModelCapabilities::chat_only(),
        }
    }
}

/// Parse openai connection config helpers.
pub fn openai_base_url(config: &Value) -> String {
    config
        .get("base_url")
        .or_else(|| config.get("api_base"))
        .and_then(Value::as_str)
        .unwrap_or("https://api.openai.com/v1")
        .trim_end_matches('/')
        .to_string()
}

pub fn openai_default_model(config: &Value) -> String {
    config
        .get("default_model")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("gpt-4o-mini")
        .to_string()
}

pub fn openai_models(config: &Value) -> Vec<AiModelEntry> {
    let mut models = Vec::new();
    if let Some(arr) = config.get("models").and_then(Value::as_array) {
        for item in arr {
            if let Ok(entry) = serde_json::from_value::<AiModelEntry>(item.clone()) {
                if !entry.id.trim().is_empty() {
                    models.push(entry);
                }
            } else if let Some(id) = item.as_str().filter(|s| !s.trim().is_empty()) {
                models.push(AiModelEntry::chat(id));
            } else if let Some(id) = item.get("id").and_then(Value::as_str) {
                models.push(AiModelEntry::chat(id));
            }
        }
    }
    if models.is_empty() {
        let default = openai_default_model(config);
        models.push(AiModelEntry::chat(default));
    }
    models
}

pub fn openai_model_capabilities(config: &Value, model_id: &str) -> AiModelCapabilities {
    let needle = model_id.trim();
    for entry in openai_models(config) {
        if entry.id == needle {
            return entry.capabilities;
        }
    }
    // Unknown model: allow chat only (safe default).
    AiModelCapabilities::chat_only()
}

pub fn require_openai_capability(
    config: &Value,
    model_id: &str,
    capability: &str,
) -> Result<(), String> {
    let caps = openai_model_capabilities(config, model_id);
    if caps.supports(capability) {
        Ok(())
    } else {
        Err(format!(
            "model `{model_id}` does not support `{capability}` — enable it on the connection model"
        ))
    }
}

/// Public connection record returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: Uuid,
    pub name: String,
    /// e.g. `telegram`, `http`, `postgres`
    #[serde(rename = "type")]
    pub connection_type: String,
    /// Non-secret configuration (base URLs, chat defaults, …).
    #[serde(default)]
    pub config: Value,
    pub enabled: bool,
    /// Whether a secret blob is stored (never the secret itself).
    pub has_secret: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateConnectionRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub connection_type: String,
    #[serde(default)]
    pub config: Value,
    /// Secret payload written only to SecretStore (never persisted on the connection row).
    #[serde(default)]
    pub secret: Option<Value>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConnectionRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub connection_type: String,
    #[serde(default)]
    pub config: Value,
    /// If set, replaces the secret. If omitted, existing secret is kept.
    #[serde(default)]
    pub secret: Option<Value>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTestResponse {
    pub ok: bool,
    pub message: String,
}

fn default_true() -> bool {
    true
}
