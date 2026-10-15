//! AI connection used to draft, execute and evaluate an objective.

use boarddo_shared::v2::Objective;
use boarddo_shared::{LlmProvider, is_llm_connection_type, llm_provider};
use uuid::Uuid;

use crate::connections::ConnectionProvider;
use crate::integrations::openai::{ChatMessage, OpenAiEndpoint, chat_complete_messages};
use crate::state::SharedState;

#[derive(Debug, Clone)]
pub struct ObjectiveLlm {
    pub connection_id: Uuid,
    pub endpoint: OpenAiEndpoint,
    pub model: String,
}

impl ObjectiveLlm {
    pub async fn chat(
        &self,
        system: &str,
        user: &str,
        temperature: f64,
        max_tokens: u64,
        timeout_ms: u64,
    ) -> Result<String, String> {
        let messages = [
            ChatMessage {
                role: "system".into(),
                content: system.to_string(),
            },
            ChatMessage {
                role: "user".into(),
                content: user.to_string(),
            },
        ];
        chat_complete_messages(
            &self.endpoint,
            &self.model,
            &messages,
            temperature,
            max_tokens,
            timeout_ms,
        )
        .await
        .map(|c| c.text)
    }
}

/// Objective's own connection, else the first usable one (Claude preferred).
pub async fn resolve(state: &SharedState, objective: &Objective) -> Result<ObjectiveLlm, String> {
    resolve_connection(state, objective.connection_id, objective.model.as_deref()).await
}

/// `connection_id` / `model` when given, else the default AI connection and its model.
pub async fn resolve_connection(
    state: &SharedState,
    connection_id: Option<Uuid>,
    model: Option<&str>,
) -> Result<ObjectiveLlm, String> {
    let connection_id = match connection_id {
        Some(id) => id,
        None => default_connection(state)
            .await?
            .ok_or_else(|| {
                "No AI connection — add an OpenAI-compatible or Claude provider in Settings → AI"
                    .to_string()
            })?,
    };
    let resolved = state.connections.resolve(&connection_id.to_string()).await?;
    let endpoint = OpenAiEndpoint::from_connection(
        &resolved.connection_type,
        &resolved.config,
        &resolved.credentials,
    )
    .map_err(|e| format!("connection `{}`: {e}", resolved.name))?;
    let model = model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(endpoint.default_model.as_str())
        .to_string();
    Ok(ObjectiveLlm {
        connection_id,
        endpoint,
        model,
    })
}

pub async fn default_connection(state: &SharedState) -> Result<Option<Uuid>, String> {
    let mut list: Vec<_> = state
        .storage
        .list_connections()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|c| c.enabled && c.has_secret && is_llm_connection_type(&c.connection_type))
        .collect();
    list.sort_by_key(|c| llm_provider(&c.connection_type, &c.config) != LlmProvider::Anthropic);
    Ok(list.first().map(|c| c.id))
}
