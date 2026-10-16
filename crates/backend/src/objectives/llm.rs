//! AI connection used to draft, execute and evaluate an objective.

use boarddo_shared::v2::{Objective, TaskKind, is_retryable_on_other_model};
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
    /// Ordered alternatives tried when the primary fails in a way another
    /// model could fix (timeouts, overload, unknown model). Never on budget
    /// or policy refusals.
    pub fallbacks: Vec<(Uuid, OpenAiEndpoint, String)>,
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
        let primary = (self.connection_id, &self.endpoint, &self.model);
        let rest = self.fallbacks.iter().map(|(id, e, m)| (*id, e, m));
        let mut errors = Vec::new();
        let mut last = String::new();
        for (connection_id, endpoint, model) in std::iter::once(primary).chain(rest) {
            match chat_complete_messages(
                endpoint,
                model,
                &messages,
                temperature,
                max_tokens,
                timeout_ms,
            )
            .await
            {
                Ok(c) => {
                    crate::routing::record_success(connection_id, model);
                    return Ok(c.text);
                }
                Err(e) if is_retryable_on_other_model(&e) => {
                    crate::routing::record_failure(connection_id, model);
                    tracing::warn!(%model, error = %e, "model failed, trying next");
                    errors.push(format!("{model}: {e}"));
                    last = e;
                }
                Err(e) => {
                    errors.push(format!("{model}: {e}"));
                    last = e;
                    break;
                }
            }
        }
        // Single attempt: keep the provider's message verbatim.
        Err(if errors.len() == 1 {
            last
        } else {
            errors.join("; ")
        })
    }
}

/// Routed model for `task`: the objective's explicit connection/model are
/// kept; otherwise the router picks by task, capabilities, health and budget.
pub async fn resolve_for(
    state: &SharedState,
    objective: &Objective,
    task: TaskKind,
) -> Result<ObjectiveLlm, String> {
    resolve_connection_for(state, objective.connection_id, objective.model.as_deref(), task).await
}

pub async fn resolve_connection_for(
    state: &SharedState,
    connection_id: Option<Uuid>,
    model: Option<&str>,
    task: TaskKind,
) -> Result<ObjectiveLlm, String> {
    crate::routing::resolve(state, task, connection_id, model, Default::default()).await
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
        fallbacks: Vec::new(),
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
