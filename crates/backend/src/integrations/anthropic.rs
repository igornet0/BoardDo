//! Anthropic Messages API client (Claude) — raw HTTP, no official Rust SDK.
//!
//! Called through [`super::openai::chat_complete_messages`] when the connection
//! provider is `anthropic`, so every chat surface (ai.* nodes, editor agent,
//! goal planner, objectives) works with Claude unchanged.

use serde_json::{Value, json};
use std::time::Duration;

use super::openai::{ChatCompletion, ChatMessage, OpenAiEndpoint};

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// `fallbacks: "default"` — re-runs a safety decline on Anthropic's recommended model.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Claude Messages API request built from OpenAI-style chat messages.
pub async fn messages_complete(
    endpoint: &OpenAiEndpoint,
    model: &str,
    messages: &[ChatMessage],
    temperature: f64,
    max_tokens: u64,
    timeout_ms: u64,
) -> Result<ChatCompletion, String> {
    let body = build_request(endpoint, model, messages, temperature, max_tokens);
    let url = format!("{}/messages", messages_base(&endpoint.base_url));

    let client = reqwest::Client::builder()
        // Thinking models can take a while even on short prompts.
        .timeout(Duration::from_millis(timeout_ms.max(60_000)))
        .build()
        .map_err(|e| format!("claude: http client: {e}"))?;

    let mut request = client
        .post(&url)
        .header("x-api-key", &endpoint.api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json");
    if body.get("fallbacks").is_some() {
        request = request.header("anthropic-beta", FALLBACK_BETA);
    }

    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("claude: request failed: {e}"))?;

    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|e| format!("claude: read body: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "claude: HTTP {status}: {}",
            error_message(&response_body)
        ));
    }

    let parsed: Value = serde_json::from_str(&response_body)
        .map_err(|e| format!("claude: invalid JSON response: {e}"))?;
    parse_response(&parsed, model)
}

/// `https://api.anthropic.com` and `https://api.anthropic.com/v1` both work.
fn messages_base(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return boarddo_shared::ANTHROPIC_BASE_URL.to_string();
    }
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

pub(crate) fn build_request(
    endpoint: &OpenAiEndpoint,
    model: &str,
    messages: &[ChatMessage],
    temperature: f64,
    max_tokens: u64,
) -> Value {
    let (system, turns) = split_messages(messages);
    let caps = ClaudeModel::of(model);

    let mut body = json!({
        "model": model,
        // Adaptive thinking counts against max_tokens — callers often ask for
        // a handful of tokens (health checks, classifiers), so keep headroom.
        "max_tokens": max_tokens.max(caps.min_max_tokens()),
        "messages": turns,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if caps.accepts_sampling {
        body["temperature"] = json!(temperature.clamp(0.0, 1.0));
    }
    if caps.supports_effort && max_tokens <= 512 {
        // Short, mechanical replies don't need deep reasoning.
        body["output_config"] = json!({ "effort": "low" });
    }
    if caps.server_fallbacks && endpoint.base_url.contains("api.anthropic.com") {
        body["fallbacks"] = json!("default");
    }
    body
}

/// System prompts go top-level; user/assistant turns must alternate and start with `user`.
fn split_messages(messages: &[ChatMessage]) -> (String, Vec<Value>) {
    let mut system = Vec::new();
    let mut turns: Vec<(String, String)> = Vec::new();
    for m in messages {
        let content = m.content.trim();
        if content.is_empty() {
            continue;
        }
        let role = match m.role.as_str() {
            "system" | "developer" => {
                system.push(content.to_string());
                continue;
            }
            "assistant" => "assistant",
            _ => "user",
        };
        match turns.last_mut() {
            Some((last_role, text)) if last_role == role => {
                text.push_str("\n\n");
                text.push_str(content);
            }
            _ => turns.push((role.to_string(), content.to_string())),
        }
    }
    if turns.first().is_some_and(|(r, _)| r == "assistant") {
        turns.insert(0, ("user".into(), "Continue.".into()));
    }
    // The 4.6+ family rejects a trailing assistant turn (prefill).
    if turns.last().is_some_and(|(r, _)| r == "assistant") {
        turns.push(("user".into(), "Continue.".into()));
    }
    if turns.is_empty() {
        turns.push(("user".into(), "Hello.".into()));
    }
    let turns = turns
        .into_iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    (system.join("\n\n"), turns)
}

pub(crate) fn parse_response(parsed: &Value, requested_model: &str) -> Result<ChatCompletion, String> {
    let stop_reason = parsed
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    if stop_reason == "refusal" {
        let category = parsed
            .pointer("/stop_details/category")
            .and_then(Value::as_str)
            .unwrap_or("unspecified");
        return Err(format!(
            "claude: request declined by safety policy (refusal, category: {category})"
        ));
    }

    let text = parsed
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(if stop_reason == "max_tokens" {
            "claude: response hit max_tokens before any text — raise `max_tokens`".into()
        } else {
            format!("claude: response has no text (stop_reason: {stop_reason})")
        });
    }

    let input = parsed.pointer("/usage/input_tokens").and_then(Value::as_u64);
    let output = parsed.pointer("/usage/output_tokens").and_then(Value::as_u64);
    // Normalised to the OpenAI shape the rest of BoardDo reads.
    let usage = match (input, output) {
        (None, None) => Value::Null,
        (i, o) => json!({
            "prompt_tokens": i,
            "completion_tokens": o,
            "total_tokens": i.unwrap_or(0) + o.unwrap_or(0),
            "cache_read_input_tokens": parsed.pointer("/usage/cache_read_input_tokens"),
            "stop_reason": stop_reason,
        }),
    };
    let model = parsed
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(requested_model)
        .to_string();
    Ok(ChatCompletion { text, usage, model })
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            let kind = v.pointer("/error/type").and_then(Value::as_str)?.to_string();
            let msg = v.pointer("/error/message").and_then(Value::as_str)?.to_string();
            Some(format!("{kind}: {msg}"))
        })
        .unwrap_or_else(|| body.chars().take(400).collect())
}

/// Request-shape differences between Claude generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaudeModel {
    /// `temperature` is rejected on Opus 4.7+ / Sonnet 5+ / Fable.
    accepts_sampling: bool,
    /// `output_config.effort` — not on Haiku 4.5 / Sonnet 4.5.
    supports_effort: bool,
    /// Thinking is on by default (adaptive) and spends from `max_tokens`.
    thinks_by_default: bool,
    /// `fallbacks: "default"` (server-side refusal fallback).
    server_fallbacks: bool,
}

impl ClaudeModel {
    fn of(model: &str) -> Self {
        let m = model.trim().to_ascii_lowercase();
        let legacy = [
            "claude-3",
            "claude-haiku-4-5",
            "claude-sonnet-4-0",
            "claude-sonnet-4-5",
            "claude-sonnet-4-6",
            "claude-opus-4-0",
            "claude-opus-4-1",
            "claude-opus-4-5",
            "claude-opus-4-6",
        ]
        .iter()
        .any(|p| m.starts_with(p));
        let effort = !legacy
            || ["claude-opus-4-5", "claude-opus-4-6", "claude-sonnet-4-6"]
                .iter()
                .any(|p| m.starts_with(p));
        let next_gen = m.starts_with("claude-opus-5")
            || m.starts_with("claude-sonnet-5")
            || m.starts_with("claude-fable")
            || m.starts_with("claude-mythos");
        let server_fallbacks = m.starts_with("claude-opus-5")
            || m.starts_with("claude-sonnet-5-5")
            || m.starts_with("claude-fable-5");
        Self {
            accepts_sampling: legacy,
            supports_effort: effort,
            thinks_by_default: next_gen,
            server_fallbacks,
        }
    }

    fn min_max_tokens(self) -> u64 {
        if self.thinks_by_default { 4096 } else { 1024 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::LlmProvider;

    fn endpoint() -> OpenAiEndpoint {
        OpenAiEndpoint {
            api_key: "k".into(),
            base_url: "https://api.anthropic.com/v1".into(),
            default_model: "claude-opus-5-5".into(),
            provider: LlmProvider::Anthropic,
        }
    }

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
        }
    }

    #[test]
    fn system_is_top_level_and_turns_alternate() {
        let body = build_request(
            &endpoint(),
            "claude-opus-5-5",
            &[
                msg("system", "be brief"),
                msg("user", "a"),
                msg("user", "b"),
                msg("assistant", "c"),
            ],
            0.2,
            8,
        );
        assert_eq!(body["system"], "be brief");
        let turns = body["messages"].as_array().unwrap();
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0]["role"], "user");
        assert_eq!(turns[0]["content"], "a\n\nb");
        assert_eq!(turns[1]["role"], "assistant");
        assert_eq!(turns[2]["role"], "user");
        // Opus 5.5: no temperature, low effort for tiny replies, headroom for thinking.
        assert!(body.get("temperature").is_none());
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["fallbacks"], "default");
    }

    #[test]
    fn haiku_keeps_temperature_without_effort() {
        let body = build_request(
            &endpoint(),
            "claude-haiku-4-5",
            &[msg("user", "hi")],
            0.3,
            2000,
        );
        assert_eq!(body["temperature"], 0.3);
        assert!(body.get("output_config").is_none());
        assert!(body.get("fallbacks").is_none());
        assert_eq!(body["max_tokens"], 2000);
    }

    #[test]
    fn parses_text_blocks_and_usage() {
        let v = json!({
            "model": "claude-opus-5-5",
            "stop_reason": "end_turn",
            "content": [
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": " pong " }
            ],
            "usage": { "input_tokens": 10, "output_tokens": 3 }
        });
        let c = parse_response(&v, "x").unwrap();
        assert_eq!(c.text, "pong");
        assert_eq!(c.usage["total_tokens"], 13);
        assert_eq!(c.model, "claude-opus-5-5");
    }

    #[test]
    fn refusal_is_an_error() {
        let v = json!({
            "stop_reason": "refusal",
            "stop_details": { "type": "refusal", "category": "cyber" },
            "content": []
        });
        let err = parse_response(&v, "claude-opus-5-5").unwrap_err();
        assert!(err.contains("cyber"));
    }

    #[test]
    fn base_url_variants() {
        assert_eq!(messages_base("https://api.anthropic.com"), "https://api.anthropic.com/v1");
        assert_eq!(messages_base("https://api.anthropic.com/v1/"), "https://api.anthropic.com/v1");
    }
}
