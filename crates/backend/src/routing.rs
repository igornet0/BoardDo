//! Multi-model routing over the configured AI connections: candidate list,
//! per-model circuit breaker and resolution into an [`ObjectiveLlm`] with
//! ordered fallbacks. Selection rules live in `boarddo_shared::v2::route`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use boarddo_shared::v2::{RouteCandidate, RouteChoice, RouteRequest, TaskKind, route};
use boarddo_shared::{
    AiModelEntry, Connection, LlmProvider, claude_default_models, is_llm_connection_type,
    llm_base_url, llm_default_model, llm_provider, openai_models,
};
use serde_json::Value;
use uuid::Uuid;

use crate::connections::ConnectionProvider;
use crate::integrations::openai::{OpenAiEndpoint, is_local_endpoint};
use crate::objectives::llm::ObjectiveLlm;
use crate::state::SharedState;

/// Consecutive provider failures that take a model out of rotation.
pub const FAILURE_THRESHOLD: u32 = 3;
/// How long a tripped model stays out of rotation.
pub const COOL_DOWN: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, Default)]
struct Health {
    failures: u32,
    open_until: Option<Instant>,
}

#[derive(Default)]
struct Breaker {
    models: HashMap<(Uuid, String), Health>,
}

impl Breaker {
    fn failure(&mut self, key: (Uuid, String), now: Instant) {
        let h = self.models.entry(key).or_default();
        h.failures += 1;
        if h.failures >= FAILURE_THRESHOLD {
            h.open_until = Some(now + COOL_DOWN);
        }
    }

    fn success(&mut self, key: &(Uuid, String)) {
        self.models.remove(key);
    }

    /// After the cool-down one probe is let through; a further failure
    /// re-opens the breaker at once (the count is still at the threshold).
    fn available(&self, key: &(Uuid, String), now: Instant) -> bool {
        match self.models.get(key).and_then(|h| h.open_until) {
            Some(until) => now >= until,
            None => true,
        }
    }
}

fn breaker() -> &'static Mutex<Breaker> {
    static B: OnceLock<Mutex<Breaker>> = OnceLock::new();
    B.get_or_init(Default::default)
}

fn with_breaker<T>(f: impl FnOnce(&mut Breaker) -> T) -> T {
    let mut guard = breaker().lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard)
}

pub fn record_failure(connection_id: Uuid, model: &str) {
    with_breaker(|b| b.failure((connection_id, model.to_string()), Instant::now()));
}

pub fn record_success(connection_id: Uuid, model: &str) {
    with_breaker(|b| b.success(&(connection_id, model.to_string())));
}

pub fn is_available(connection_id: Uuid, model: &str) -> bool {
    with_breaker(|b| b.available(&(connection_id, model.to_string()), Instant::now()))
}

/// Models a connection offers: its configured list, else the Claude catalog
/// for Anthropic, else its default model. The default model is always kept.
pub fn connection_models(connection_type: &str, config: &Value) -> Vec<AiModelEntry> {
    let has_list = config
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty());
    let mut models = if has_list {
        openai_models(config)
    } else if llm_provider(connection_type, config) == LlmProvider::Anthropic {
        claude_default_models()
    } else {
        Vec::new()
    };
    let default = llm_default_model(connection_type, config);
    if !default.trim().is_empty() && !models.iter().any(|m| m.id == default) {
        models.push(AiModelEntry::chat(default));
    }
    models
}

/// Every model of every enabled AI connection that has a secret.
pub fn candidates_from(connections: &[Connection]) -> Vec<RouteCandidate> {
    let mut out = Vec::new();
    for c in connections {
        if !(c.enabled && c.has_secret && is_llm_connection_type(&c.connection_type)) {
            continue;
        }
        let provider = llm_provider(&c.connection_type, &c.config);
        let local = is_local_endpoint(&llm_base_url(&c.connection_type, &c.config));
        for m in connection_models(&c.connection_type, &c.config) {
            out.push(RouteCandidate {
                connection_id: c.id,
                connection_name: c.name.clone(),
                provider,
                available: is_available(c.id, &m.id),
                model: m.id,
                capabilities: m.capabilities,
                local,
            });
        }
    }
    out
}

pub async fn candidates(state: &SharedState) -> Result<Vec<RouteCandidate>, String> {
    let list = state.storage.list_connections().await.map_err(|e| e.to_string())?;
    Ok(candidates_from(&list))
}

/// Size of the call the route is for (worst case is priced from it).
#[derive(Debug, Clone, Copy)]
pub struct CallSize {
    pub est_input_tokens: u64,
    pub max_output_tokens: u64,
}

impl Default for CallSize {
    fn default() -> Self {
        Self {
            est_input_tokens: 4_000,
            max_output_tokens: 4_096,
        }
    }
}

/// Resolves the model for a task.
///
/// * `model` given → exactly that model on the given (or default) connection, no fallbacks;
/// * only `connection_id` given → its default model first, then other
///   models of the same connection that fit the task and budget;
/// * neither → the routed choice over all connections, with fallbacks.
pub async fn resolve(
    state: &SharedState,
    task: TaskKind,
    connection_id: Option<Uuid>,
    model: Option<&str>,
    size: CallSize,
) -> Result<ObjectiveLlm, String> {
    let model = model.map(str::trim).filter(|s| !s.is_empty());
    if model.is_some() {
        return crate::objectives::llm::resolve_connection(state, connection_id, model).await;
    }
    // Fail closed: an active budget whose spend cannot be read stops the call.
    let budget_remaining_usd = crate::ledger::remaining_usd().await?;
    let req = RouteRequest {
        task,
        required: vec!["chat".into()],
        est_input_tokens: size.est_input_tokens,
        max_output_tokens: size.max_output_tokens,
        budget_remaining_usd,
        connection_id,
        prefer_provider: Some(LlmProvider::Anthropic),
        exclude: Vec::new(),
    };
    let candidates = candidates(state).await?;

    let mut endpoints: HashMap<Uuid, OpenAiEndpoint> = HashMap::new();
    let mut chain: Vec<(Uuid, String)> = Vec::new();
    if let Some(id) = connection_id {
        // The connection's own default model leads (explicit choice is kept).
        let primary = crate::objectives::llm::resolve_connection(state, Some(id), None).await?;
        chain.push((id, primary.model.clone()));
        endpoints.insert(id, primary.endpoint);
        if let Ok(d) = route(&req, &candidates) {
            push_choices(&mut chain, std::iter::once(&d.choice).chain(&d.fallbacks));
        }
    } else {
        let d = route(&req, &candidates).map_err(|e| e.to_string())?;
        tracing::debug!(reason = %d.reason, "model routed");
        push_choices(&mut chain, std::iter::once(&d.choice).chain(&d.fallbacks));
    }

    let mut resolved: Vec<(Uuid, OpenAiEndpoint, String)> = Vec::new();
    let mut last_err = None;
    for (id, model) in chain {
        if !endpoints.contains_key(&id) {
            match endpoint_for(state, id).await {
                Ok(e) => {
                    endpoints.insert(id, e);
                }
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            }
        }
        resolved.push((id, endpoints[&id].clone(), model));
    }
    let mut it = resolved.into_iter();
    let (connection_id, endpoint, model) = it
        .next()
        .ok_or_else(|| last_err.unwrap_or_else(|| "no AI model available".into()))?;
    Ok(ObjectiveLlm {
        connection_id,
        endpoint,
        model,
        fallbacks: it.collect(),
    })
}

fn push_choices<'a>(chain: &mut Vec<(Uuid, String)>, choices: impl Iterator<Item = &'a RouteChoice>) {
    for c in choices {
        let key = (c.connection_id, c.model.clone());
        if !chain.contains(&key) {
            chain.push(key);
        }
    }
}

async fn endpoint_for(state: &SharedState, id: Uuid) -> Result<OpenAiEndpoint, String> {
    let r = state.connections.resolve(&id.to_string()).await?;
    OpenAiEndpoint::from_connection(&r.connection_type, &r.config, &r.credentials)
        .map_err(|e| format!("connection `{}`: {e}", r.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn conn(id: u128, ty: &str, config: Value, enabled: bool, has_secret: bool) -> Connection {
        Connection {
            id: Uuid::from_u128(id),
            name: format!("c{id}"),
            connection_type: ty.into(),
            config,
            enabled,
            has_secret,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn breaker_opens_after_threshold_and_half_opens_after_cool_down() {
        let mut b = Breaker::default();
        let key = (Uuid::from_u128(1), "m".to_string());
        let t0 = Instant::now();
        for _ in 0..FAILURE_THRESHOLD - 1 {
            b.failure(key.clone(), t0);
        }
        assert!(b.available(&key, t0), "below threshold stays available");
        b.failure(key.clone(), t0);
        assert!(!b.available(&key, t0));
        assert!(!b.available(&key, t0 + COOL_DOWN - Duration::from_secs(1)));
        assert!(b.available(&key, t0 + COOL_DOWN), "probe allowed after cool-down");
        // Failed probe re-opens immediately.
        b.failure(key.clone(), t0 + COOL_DOWN);
        assert!(!b.available(&key, t0 + COOL_DOWN + Duration::from_secs(1)));
        b.success(&key);
        assert!(b.available(&key, t0 + COOL_DOWN + Duration::from_secs(1)));
    }

    #[test]
    fn success_resets_the_failure_count() {
        let mut b = Breaker::default();
        let key = (Uuid::from_u128(2), "m".to_string());
        let t0 = Instant::now();
        b.failure(key.clone(), t0);
        b.failure(key.clone(), t0);
        b.success(&key);
        b.failure(key.clone(), t0);
        assert!(b.available(&key, t0));
    }

    #[test]
    fn candidates_cover_models_and_skip_unusable_connections() {
        let list = vec![
            conn(1, "anthropic", json!({ "provider": "anthropic" }), true, true),
            conn(2, "openai", json!({ "base_url": "http://127.0.0.1:1/v1", "models": ["qwen", "llama"], "default_model": "qwen" }), true, true),
            conn(3, "openai", json!({}), false, true),
            conn(4, "openai", json!({}), true, false),
            conn(5, "telegram", json!({}), true, true),
        ];
        let c = candidates_from(&list);
        let claude: Vec<_> = c.iter().filter(|c| c.connection_id == Uuid::from_u128(1)).collect();
        assert!(claude.iter().any(|c| c.model == "claude-opus-5-5"));
        assert!(claude.iter().all(|c| c.provider == LlmProvider::Anthropic && !c.local));
        let local: Vec<_> = c.iter().filter(|c| c.connection_id == Uuid::from_u128(2)).collect();
        assert_eq!(local.iter().map(|c| c.model.as_str()).collect::<Vec<_>>(), ["qwen", "llama"]);
        assert!(local.iter().all(|c| c.local));
        assert!(c.iter().all(|c| [1, 2].contains(&c.connection_id.as_u128())));
    }

    #[test]
    fn default_model_is_always_a_candidate() {
        let m = connection_models("openai", &json!({ "models": ["a"], "default_model": "b" }));
        assert_eq!(m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn tripped_model_is_marked_unavailable_in_candidates() {
        let id = Uuid::now_v7();
        let list = vec![Connection {
            id,
            ..conn(0, "openai", json!({ "models": ["x", "y"] }), true, true)
        }];
        for _ in 0..FAILURE_THRESHOLD {
            record_failure(id, "x");
        }
        let c = candidates_from(&list);
        assert!(!c.iter().find(|c| c.model == "x").unwrap().available);
        assert!(c.iter().find(|c| c.model == "y").unwrap().available);
        record_success(id, "x");
        assert!(is_available(id, "x"));
    }

    /// OpenAI-compatible mock on 127.0.0.1: model `bad` answers 503,
    /// `invalid` answers 400, anything else answers with its own name.
    async fn mock_openai() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let log = log.clone();
                async move {
                    let model = body["model"].as_str().unwrap_or_default().to_string();
                    log.lock().unwrap().push(model.clone());
                    let status = match model.as_str() {
                        "bad" => axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        "invalid" => axum::http::StatusCode::BAD_REQUEST,
                        _ => axum::http::StatusCode::OK,
                    };
                    let body = json!({
                        "model": model,
                        "choices": [{ "message": { "role": "assistant", "content": format!("from {model}") } }],
                        "usage": { "prompt_tokens": 10, "completion_tokens": 3 }
                    });
                    (status, axum::Json(body))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/v1"), seen)
    }

    async fn add_connection(state: &SharedState, base_url: &str, models: &[&str], default: &str) -> Uuid {
        let id = Uuid::now_v7();
        let secret_ref = format!("connection:{id}");
        state
            .secrets
            .set(&secret_ref, json!({ "api_key": "sk-local" }).to_string().as_bytes())
            .await
            .unwrap();
        let config = json!({ "base_url": base_url, "models": models, "default_model": default });
        state
            .storage
            .insert_connection(id, "Local", "openai", &config, Some(&secret_ref), true, chrono::Utc::now())
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn connection_default_model_leads_and_failover_uses_the_next_model() {
        let state = crate::objectives::tests::test_state().await;
        let (url, seen) = mock_openai().await;
        let id = add_connection(&state, &url, &["bad", "good"], "bad").await;

        let llm = crate::objectives::llm::resolve_connection_for(&state, Some(id), None, TaskKind::Generation)
            .await
            .unwrap();
        assert_eq!(llm.model, "bad", "connection default model stays primary");
        assert!(llm.fallbacks.iter().any(|(c, _, m)| *c == id && m == "good"));

        let text = llm.chat("s", "u", 0.0, 64, 5_000).await.unwrap();
        assert_eq!(text, "from good");
        assert_eq!(*seen.lock().unwrap(), ["bad", "good"]);
        record_success(id, "bad");
    }

    #[tokio::test]
    async fn explicit_model_is_kept_without_fallbacks() {
        let state = crate::objectives::tests::test_state().await;
        let (url, seen) = mock_openai().await;
        let id = add_connection(&state, &url, &["bad", "good"], "good").await;

        let llm = crate::objectives::llm::resolve_connection_for(&state, Some(id), Some("bad"), TaskKind::Planning)
            .await
            .unwrap();
        assert_eq!(llm.model, "bad");
        assert!(llm.fallbacks.is_empty());
        let err = llm.chat("s", "u", 0.0, 64, 5_000).await.unwrap_err();
        assert!(err.contains("503"), "{err}");
        assert_eq!(*seen.lock().unwrap(), ["bad"]);
        record_success(id, "bad");
    }

    #[tokio::test]
    async fn non_retryable_error_does_not_fail_over() {
        let state = crate::objectives::tests::test_state().await;
        let (url, seen) = mock_openai().await;
        let id = add_connection(&state, &url, &["invalid", "good"], "invalid").await;

        let llm = crate::objectives::llm::resolve_connection_for(&state, Some(id), None, TaskKind::Generation)
            .await
            .unwrap();
        assert!(!llm.fallbacks.is_empty());
        let err = llm.chat("s", "u", 0.0, 64, 5_000).await.unwrap_err();
        assert!(err.contains("400"), "{err}");
        assert_eq!(*seen.lock().unwrap(), ["invalid"]);
        assert!(is_available(id, "invalid"), "client errors do not trip the breaker");
    }

    #[tokio::test]
    async fn routes_without_explicit_connection_and_skips_tripped_models() {
        let state = crate::objectives::tests::test_state().await;
        let (url, seen) = mock_openai().await;
        let id = add_connection(&state, &url, &["alpha", "beta"], "alpha").await;

        let first = resolve(&state, TaskKind::Generation, None, None, CallSize::default()).await.unwrap();
        assert_eq!(first.connection_id, id);
        let primary = first.model.clone();
        for _ in 0..FAILURE_THRESHOLD {
            record_failure(id, &primary);
        }
        let second = resolve(&state, TaskKind::Generation, None, None, CallSize::default()).await.unwrap();
        assert_ne!(second.model, primary, "tripped model is not routed to");
        assert!(second.fallbacks.iter().all(|(_, _, m)| *m != primary));
        assert_eq!(second.chat("s", "u", 0.0, 64, 5_000).await.unwrap(), format!("from {}", second.model));
        assert_eq!(seen.lock().unwrap().len(), 1);
        record_success(id, &primary);
    }

    #[tokio::test]
    async fn no_connection_is_an_error() {
        let state = crate::objectives::tests::test_state().await;
        let err = resolve(&state, TaskKind::Planning, None, None, CallSize::default()).await.unwrap_err();
        assert!(!err.is_empty());
    }
}
