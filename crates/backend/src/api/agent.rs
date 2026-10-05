//! Internal AI agent for creating and editing BoardDo scenarios.
//! Credentials come from an openai Connection (unified with workflow nodes).

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use boarddo_shared::{
    AgentChatRequest, AgentChatResponse, AgentGraphOp, AgentSettings, AgentSettingsTestResponse,
    AgentUsage, AgentWorkflowSnapshot, CreateConnectionRequest, Node, UpdateAgentSettingsRequest,
    openai_base_url, openai_default_model, openai_models,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::connections::ConnectionProvider;
use crate::integrations::openai::{
    ChatMessage, OpenAiEndpoint, chat_complete, chat_complete_messages, extract_json_object,
};
use crate::state::SharedState;

const SETTINGS_KEY: &str = "agent:settings";
const LEGACY_API_KEY_KEY: &str = "agent:api_key";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSettings {
    enabled: bool,
    #[serde(default)]
    connection_id: Option<Uuid>,
    #[serde(default)]
    model: Option<String>,
    /// Legacy fields — used only for migration.
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    legacy_model: Option<String>,
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            connection_id: None,
            model: None,
            base_url: None,
            legacy_model: None,
        }
    }
}

/// Deserialize either new shape or old `{enabled, base_url, model}`.
fn parse_stored(bytes: &[u8]) -> StoredSettings {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        enabled: bool,
        #[serde(default)]
        connection_id: Option<Uuid>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        base_url: Option<String>,
    }
    let Ok(raw) = serde_json::from_slice::<Raw>(bytes) else {
        return StoredSettings::default();
    };
    // Old format stored model as required string field named `model`.
    let (model, legacy_model) = if raw.connection_id.is_some() {
        (raw.model, None)
    } else if let Some(m) = raw.model.filter(|s| !s.trim().is_empty()) {
        (None, Some(m))
    } else {
        (None, None)
    };
    StoredSettings {
        enabled: raw.enabled,
        connection_id: raw.connection_id,
        model,
        base_url: raw.base_url,
        legacy_model,
    }
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/settings", get(get_settings).put(put_settings))
        .route("/settings/test", post(test_settings))
        .route("/chat", post(chat))
}

pub async fn get_settings(
    State(state): State<SharedState>,
) -> Result<Json<AgentSettings>, (StatusCode, String)> {
    let stored = migrate_if_needed(&state).await.map_err(internal)?;
    Ok(Json(to_public(&state, &stored).await.map_err(internal)?))
}

pub async fn put_settings(
    State(state): State<SharedState>,
    Json(req): Json<UpdateAgentSettingsRequest>,
) -> Result<Json<AgentSettings>, (StatusCode, String)> {
    let mut stored = migrate_if_needed(&state).await.map_err(internal)?;
    stored.enabled = req.enabled;
    stored.connection_id = req.connection_id;
    stored.model = req
        .model
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    stored.base_url = None;
    stored.legacy_model = None;

    if let Some(id) = stored.connection_id {
        let conn = state
            .connections
            .get_public(&id.to_string())
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        let Some(conn) = conn else {
            return Err((StatusCode::BAD_REQUEST, "connection not found".into()));
        };
        if conn.connection_type != "openai" && conn.connection_type != "ai" {
            return Err((
                StatusCode::BAD_REQUEST,
                "connection must be type openai".into(),
            ));
        }
        if !conn.has_secret {
            return Err((
                StatusCode::BAD_REQUEST,
                "connection has no API key secret".into(),
            ));
        }
    }

    persist_stored(&state, &stored).await.map_err(internal)?;
    Ok(Json(to_public(&state, &stored).await.map_err(internal)?))
}

pub async fn test_settings(
    State(state): State<SharedState>,
) -> Result<Json<AgentSettingsTestResponse>, (StatusCode, String)> {
    let stored = migrate_if_needed(&state).await.map_err(internal)?;
    let (endpoint, model) = match resolve_endpoint(&state, &stored).await {
        Ok(v) => v,
        Err(err) => {
            return Ok(Json(AgentSettingsTestResponse {
                ok: false,
                message: err,
            }));
        }
    };
    match chat_complete(
        &endpoint,
        &model,
        "You are a BoardDo health check. Reply with the single word pong.",
        "ping",
        0.0,
        8,
        20_000,
    )
    .await
    {
        Ok((text, _, model)) => Ok(Json(AgentSettingsTestResponse {
            ok: true,
            message: format!(
                "Connected ({model}): {}",
                text.chars().take(80).collect::<String>()
            ),
        })),
        Err(err) => Ok(Json(AgentSettingsTestResponse {
            ok: false,
            message: err,
        })),
    }
}

pub async fn chat(
    State(state): State<SharedState>,
    Json(req): Json<AgentChatRequest>,
) -> Result<Json<AgentChatResponse>, (StatusCode, String)> {
    let stored = migrate_if_needed(&state).await.map_err(internal)?;
    if !stored.enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            "AI agent is disabled. Enable it in Settings.".into(),
        ));
    }
    let (endpoint, model) = resolve_endpoint(&state, &stored)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if req.messages.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "messages must not be empty".into()));
    }

    let locale = req.locale.as_deref().unwrap_or("en");
    let system = build_system_prompt(locale, &req.workflow, &req.selection.node_ids);

    let mut messages = vec![ChatMessage {
        role: "system".into(),
        content: system,
    }];
    for m in req
        .messages
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let role = match m.role.as_str() {
            "assistant" => "assistant",
            _ => "user",
        };
        if m.content.trim().is_empty() {
            continue;
        }
        messages.push(ChatMessage {
            role: role.into(),
            content: m.content.clone(),
        });
    }
    if messages.len() < 2 {
        return Err((StatusCode::BAD_REQUEST, "no user content to send".into()));
    }

    let completion = chat_complete_messages(&endpoint, &model, &messages, 0.2, 2048, 90_000)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    let parsed = parse_agent_payload(&completion.text);
    let ops = crate::workflow::validate_ops(parsed.ops, &req.workflow)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let usage = usage_from_value(&completion.usage);

    Ok(Json(AgentChatResponse {
        message: parsed.message,
        ops,
        model: completion.model,
        usage,
    }))
}

async fn migrate_if_needed(state: &SharedState) -> anyhow::Result<StoredSettings> {
    let mut stored = load_stored(state).await?;
    if stored.connection_id.is_some() {
        return Ok(stored);
    }
    let Some(api_key) = load_legacy_api_key(state).await? else {
        return Ok(stored);
    };

    let base_url = OpenAiEndpoint::normalize_base_url(
        stored
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1"),
    );
    let default_model = stored
        .legacy_model
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "gpt-4o-mini".into());
    let models = openai_models(&json!({
        "default_model": default_model,
        "models": [{
            "id": default_model,
            "capabilities": { "chat": true }
        }]
    }));

    let conn = state
        .connections
        .create(CreateConnectionRequest {
            name: "Editor agent (migrated)".into(),
            connection_type: "openai".into(),
            config: json!({
                "base_url": base_url,
                "default_model": default_model,
                "models": models,
            }),
            secret: Some(json!({ "api_key": api_key })),
            enabled: true,
        })
        .await?;

    stored.connection_id = Some(conn.id);
    stored.model = Some(default_model);
    stored.base_url = None;
    stored.legacy_model = None;
    persist_stored(state, &stored).await?;
    let _ = state.secrets.delete(LEGACY_API_KEY_KEY).await;
    tracing::info!(connection_id = %conn.id, "agent.settings.migrated_to_connection");
    Ok(stored)
}

async fn to_public(state: &SharedState, stored: &StoredSettings) -> anyhow::Result<AgentSettings> {
    let mut out = AgentSettings {
        enabled: stored.enabled,
        connection_id: stored.connection_id,
        model: stored.model.clone(),
        has_connection: false,
        connection_name: None,
        base_url: None,
        default_model: None,
    };
    if let Some(id) = stored.connection_id {
        if let Some(conn) = state
            .connections
            .get_public(&id.to_string())
            .await
            .map_err(|e| anyhow::anyhow!(e))?
        {
            out.has_connection = conn.has_secret && conn.enabled;
            out.connection_name = Some(conn.name);
            out.base_url = Some(openai_base_url(&conn.config));
            out.default_model = Some(openai_default_model(&conn.config));
            if out.model.is_none() {
                out.model = out.default_model.clone();
            }
        }
    }
    Ok(out)
}

async fn resolve_endpoint(
    state: &SharedState,
    stored: &StoredSettings,
) -> Result<(OpenAiEndpoint, String), String> {
    let id = stored
        .connection_id
        .ok_or_else(|| "No openai connection selected for the AI agent".to_string())?;
    let resolved = state.connections.resolve(&id.to_string()).await?;
    if resolved.connection_type != "openai" && resolved.connection_type != "ai" {
        return Err(format!(
            "connection `{}` must be type openai",
            resolved.name
        ));
    }
    let api_key = resolved
        .credentials
        .get("api_key")
        .or_else(|| resolved.credentials.get("token"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "connection secret must include api_key".to_string())?
        .to_string();
    let base_url = openai_base_url(&resolved.config);
    let default_model = openai_default_model(&resolved.config);
    let model = stored
        .model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(default_model.as_str())
        .to_string();
    Ok((
        OpenAiEndpoint {
            api_key,
            base_url,
            default_model,
        },
        model,
    ))
}

async fn load_stored(state: &SharedState) -> anyhow::Result<StoredSettings> {
    match state.secrets.get(SETTINGS_KEY).await? {
        Some(bytes) => Ok(parse_stored(&bytes)),
        None => Ok(StoredSettings::default()),
    }
}

async fn persist_stored(state: &SharedState, stored: &StoredSettings) -> anyhow::Result<()> {
    let blob = serde_json::to_vec(&json!({
        "enabled": stored.enabled,
        "connection_id": stored.connection_id,
        "model": stored.model,
    }))?;
    state.secrets.set(SETTINGS_KEY, &blob).await?;
    Ok(())
}

async fn load_legacy_api_key(state: &SharedState) -> anyhow::Result<Option<String>> {
    match state.secrets.get(LEGACY_API_KEY_KEY).await? {
        Some(bytes) => {
            let s = String::from_utf8_lossy(&bytes).trim().to_string();
            if s.is_empty() {
                Ok(None)
            } else {
                Ok(Some(s))
            }
        }
        None => Ok(None),
    }
}

struct ParsedPayload {
    message: String,
    ops: Vec<AgentGraphOp>,
}

fn parse_agent_payload(raw: &str) -> ParsedPayload {
    if let Some(value) = extract_json_object(raw)
        && let Ok(parsed) = serde_json::from_value::<AgentChatResponse>(value)
    {
        return ParsedPayload {
            message: parsed.message,
            ops: parsed.ops,
        };
    }
    if let Some(value) = extract_json_object(raw) {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(raw)
            .to_string();
        let ops = value
            .get("ops")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        return ParsedPayload { message, ops };
    }
    ParsedPayload {
        message: raw.trim().to_string(),
        ops: Vec::new(),
    }
}

fn validate_ops(
    ops: Vec<AgentGraphOp>,
    workflow: &AgentWorkflowSnapshot,
) -> Result<Vec<AgentGraphOp>, (StatusCode, String)> {
    crate::workflow::validate_ops(ops, workflow).map_err(|e| (StatusCode::BAD_REQUEST, e))
}

fn build_system_prompt(locale: &str, workflow: &AgentWorkflowSnapshot, selection: &[String]) -> String {
    let lang = if locale.starts_with("ru") {
        "Reply in Russian."
    } else {
        "Reply in English."
    };
    let graph = json!({
        "name": workflow.name,
        "description": workflow.description,
        "nodes": workflow.nodes.iter().map(|n| json!({
            "id": n.id,
            "type_id": n.type_id,
            "position": n.position,
            "config": n.config,
        })).collect::<Vec<_>>(),
        "edges": workflow.edges,
    });
    let pinned: Vec<&Node> = workflow
        .nodes
        .iter()
        .filter(|n| selection.iter().any(|id| id == &n.id))
        .collect();
    let pinned_json = serde_json::to_string(&pinned).unwrap_or_else(|_| "[]".into());
    let graph_json = serde_json::to_string(&graph).unwrap_or_else(|_| "{}".into());

    format!(
        r#"You are the BoardDo scenario editor agent. {lang}
BoardDo is a visual automation canvas. Users describe workflows; you edit the graph.

Return ONLY a JSON object (no markdown) with this shape:
{{"message":"short explanation for the user","ops":[ ... graph ops ... ]}}

Allowed ops:
- {{"op":"add_node","id":"optional_id","type_id":"ai.chat","position":{{"x":280,"y":160}},"config":{{}}}}
- {{"op":"update_node","id":"existing_id","config":{{"prompt":"..."}}}}
- {{"op":"remove_node","id":"existing_id"}}
- {{"op":"add_edge","source":"a","target":"b","source_port":null}}
- {{"op":"remove_edge","id":"e1"}} or {{"op":"remove_edge","source":"a","target":"b"}}
- {{"op":"set_meta","name":"New name","description":"..."}}

Use only these type_id values:
trigger.manual, trigger.webhook, trigger.schedule, trigger.telegram.user.message_received,
logic.condition, logic.delay, data.set, data.transform, http.request, debug.log,
ai.chat, ai.classify, ai.analyze, ai.image, ai.audio, ai.video, web.search, web.open, web.extract, web.fetch,
telegram.send_message, telegram.send_photo, telegram.send_document,
telegram.user.send_message, telegram.user.forward_message, telegram.user.edit_message, telegram.user.delete_messages.

Config notes:
- Expressions use {{trigger.text}}, {{nodes.<id>.output.*}}, {{variables.*}}
- Condition nodes may use source_port "true" / "false" on outgoing edges
- AI / Telegram / HTTP nodes take connection_id as a string (leave empty if unknown)
- web.search: query, limit, read_pages (0-10, how many found pages to read), freshness (auto|day|week|month|year|any; auto = week for news), render (auto|http|browser); output.context is a digest with today's date, period, publication dates and page texts for LLM prompts
- web.open: url (string, list, or {{{{nodes.search.output.results}}}}), max_chars, selector (CSS), fields ("name: css" lines, "css@attr", "css[]" for all), render (auto|http|browser), structured (JSON-LD/meta/microdata/tables/embedded JSON → output.structured), browser-only: wait_for (CSS), wait_text, wait_js, network_idle_ms, wait_ms, script (JS → output.script_result), screenshot (→ output.screenshot media URI), capture_network (→ output.api_candidates with JSON), fetch_api; output.text / output.data / output.evidence / output.provenance / output.render; failed pages carry status blocked|rate_limited|not_found|challenge and retryable
- web.extract: connection_id, model, url, instruction ("product name, price, availability"), schema ("name: description" lines), render, capture_network, strict; output.data, output.evidence (value, verified, source url/method/json_path/quote), output.unverified
- ai.image modes: generate | edit; ai.audio modes: tts | transcribe; ai.video: prompt (+ optional image)
- Prefer updating existing selected nodes over creating duplicates
- If the user only asks a question, return ops: []

Current workflow:
{graph_json}

Pinned nodes the user attached for this turn:
{pinned_json}
"#
    )
}

fn usage_from_value(v: &Value) -> Option<AgentUsage> {
    if v.is_null() {
        return None;
    }
    Some(AgentUsage {
        prompt_tokens: v.get("prompt_tokens").and_then(Value::as_u64),
        completion_tokens: v.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: v.get("total_tokens").and_then(Value::as_u64),
    })
}

fn internal(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = %err, "agent api error");
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::Position;

    #[test]
    fn parses_ops_json() {
        let raw = r#"{"message":"Added a log","ops":[{"op":"add_node","type_id":"debug.log","config":{"message":"hi"}}]}"#;
        let parsed = parse_agent_payload(raw);
        assert_eq!(parsed.message, "Added a log");
        assert_eq!(parsed.ops.len(), 1);
        match &parsed.ops[0] {
            AgentGraphOp::AddNode { type_id, .. } => assert_eq!(type_id, "debug.log"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_type() {
        let wf = AgentWorkflowSnapshot {
            name: "t".into(),
            description: String::new(),
            nodes: vec![],
            edges: vec![],
        };
        let ops = vec![AgentGraphOp::AddNode {
            id: None,
            type_id: "not.a.type".into(),
            position: Some(Position { x: 0.0, y: 0.0 }),
            config: json!({}),
        }];
        assert!(validate_ops(ops, &wf).is_err());
    }

    #[test]
    fn parses_legacy_settings_blob() {
        let bytes = serde_json::to_vec(&json!({
            "enabled": true,
            "base_url": "https://api.deepseek.com/v1",
            "model": "deepseek-chat"
        }))
        .unwrap();
        let s = parse_stored(&bytes);
        assert!(s.enabled);
        assert!(s.connection_id.is_none());
        assert_eq!(s.legacy_model.as_deref(), Some("deepseek-chat"));
        assert_eq!(s.base_url.as_deref(), Some("https://api.deepseek.com/v1"));
    }
}
