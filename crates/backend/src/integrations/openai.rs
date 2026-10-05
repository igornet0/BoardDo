//! OpenAI-compatible Chat Completions + media clients.

use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct OpenAiEndpoint {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
}

impl OpenAiEndpoint {
    pub fn normalize_base_url(raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            "https://api.openai.com/v1".into()
        } else {
            trimmed.trim_end_matches('/').to_string()
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ChatCompletion {
    pub text: String,
    pub usage: Value,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct ImageResult {
    pub b64_json: Option<String>,
    pub url: Option<String>,
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VideoJob {
    pub id: String,
    pub status: String,
    pub raw: Value,
}

pub async fn chat_complete(
    endpoint: &OpenAiEndpoint,
    model: &str,
    system: &str,
    user: &str,
    temperature: f64,
    max_tokens: u64,
    timeout_ms: u64,
) -> Result<(String, Value, String), String> {
    let mut messages = Vec::new();
    if !system.trim().is_empty() {
        messages.push(ChatMessage {
            role: "system".into(),
            content: system.to_string(),
        });
    }
    messages.push(ChatMessage {
        role: "user".into(),
        content: user.to_string(),
    });
    let result =
        chat_complete_messages(endpoint, model, &messages, temperature, max_tokens, timeout_ms)
            .await?;
    Ok((result.text, result.usage, result.model))
}

pub async fn chat_complete_messages(
    endpoint: &OpenAiEndpoint,
    model: &str,
    messages: &[ChatMessage],
    temperature: f64,
    max_tokens: u64,
    timeout_ms: u64,
) -> Result<ChatCompletion, String> {
    let payload: Vec<Value> = messages
        .iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();

    let body = json!({
        "model": model,
        "messages": payload,
        "temperature": temperature,
        "max_tokens": max_tokens,
    });

    let url = format!("{}/chat/completions", endpoint.base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(1_000)))
        .build()
        .map_err(|e| format!("ai: http client: {e}"))?;

    let response = client
        .post(&url)
        .bearer_auth(&endpoint.api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("ai: request failed: {e}"))?;

    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|e| format!("ai: read body: {e}"))?;

    if !status.is_success() {
        let snippet: String = response_body.chars().take(400).collect();
        return Err(format!("ai: HTTP {status}: {snippet}"));
    }

    let parsed: Value = serde_json::from_str(&response_body)
        .map_err(|e| format!("ai: invalid JSON response: {e}"))?;

    let text = extract_assistant_text(&parsed)
        .ok_or_else(|| "ai: response missing assistant text".to_string())?;
    let usage = parsed.get("usage").cloned().unwrap_or(Value::Null);
    let model_used = parsed
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(model)
        .to_string();
    Ok(ChatCompletion {
        text,
        usage,
        model: model_used,
    })
}

pub async fn images_generate(
    endpoint: &OpenAiEndpoint,
    model: &str,
    prompt: &str,
    size: &str,
    quality: Option<&str>,
    timeout_ms: u64,
) -> Result<ImageResult, String> {
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "n": 1,
        "size": if size.trim().is_empty() { "1024x1024" } else { size },
        "response_format": "b64_json",
    });
    if let Some(q) = quality.filter(|s| !s.trim().is_empty()) {
        body["quality"] = json!(q);
    }

    let url = format!("{}/images/generations", endpoint.base_url);
    let parsed = post_json(endpoint, &url, &body, timeout_ms).await?;
    parse_image_result(&parsed)
}

pub async fn images_edit(
    endpoint: &OpenAiEndpoint,
    model: &str,
    prompt: &str,
    image_bytes: &[u8],
    image_filename: &str,
    image_mime: &str,
    size: &str,
    timeout_ms: u64,
) -> Result<ImageResult, String> {
    let url = format!("{}/images/edits", endpoint.base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(5_000)))
        .build()
        .map_err(|e| format!("ai.image: http client: {e}"))?;

    let part = reqwest::multipart::Part::bytes(image_bytes.to_vec())
        .file_name(image_filename.to_string())
        .mime_str(image_mime)
        .map_err(|e| format!("ai.image: mime: {e}"))?;

    let mut form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .text("prompt", prompt.to_string())
        .text("n", "1")
        .text(
            "size",
            if size.trim().is_empty() {
                "1024x1024".into()
            } else {
                size.to_string()
            },
        )
        .text("response_format", "b64_json")
        .part("image", part);

    // Some gateways ignore unused fields; keep form lean.
    let _ = &mut form;

    let response = client
        .post(&url)
        .bearer_auth(&endpoint.api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("ai.image edit: {e}"))?;

    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|e| format!("ai.image edit body: {e}"))?;
    if !status.is_success() {
        let snippet: String = response_body.chars().take(400).collect();
        return Err(format!("ai.image edit: HTTP {status}: {snippet}"));
    }
    let parsed: Value = serde_json::from_str(&response_body)
        .map_err(|e| format!("ai.image edit JSON: {e}"))?;
    parse_image_result(&parsed)
}

pub async fn audio_speech(
    endpoint: &OpenAiEndpoint,
    model: &str,
    input: &str,
    voice: &str,
    format: &str,
    timeout_ms: u64,
) -> Result<Vec<u8>, String> {
    let body = json!({
        "model": model,
        "input": input,
        "voice": if voice.trim().is_empty() { "alloy" } else { voice },
        "response_format": if format.trim().is_empty() { "mp3" } else { format },
    });
    let url = format!("{}/audio/speech", endpoint.base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(5_000)))
        .build()
        .map_err(|e| format!("ai.audio: http client: {e}"))?;
    let response = client
        .post(&url)
        .bearer_auth(&endpoint.api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("ai.audio tts: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        let snippet: String = text.chars().take(400).collect();
        return Err(format!("ai.audio tts: HTTP {status}: {snippet}"));
    }
    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("ai.audio tts body: {e}"))
}

pub async fn audio_transcribe(
    endpoint: &OpenAiEndpoint,
    model: &str,
    audio_bytes: &[u8],
    filename: &str,
    mime: &str,
    timeout_ms: u64,
) -> Result<String, String> {
    let url = format!("{}/audio/transcriptions", endpoint.base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(5_000)))
        .build()
        .map_err(|e| format!("ai.audio: http client: {e}"))?;

    let part = reqwest::multipart::Part::bytes(audio_bytes.to_vec())
        .file_name(filename.to_string())
        .mime_str(mime)
        .map_err(|e| format!("ai.audio mime: {e}"))?;

    let form = reqwest::multipart::Form::new()
        .text(
            "model",
            if model.trim().is_empty() {
                "whisper-1".into()
            } else {
                model.to_string()
            },
        )
        .part("file", part);

    let response = client
        .post(&url)
        .bearer_auth(&endpoint.api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("ai.audio transcribe: {e}"))?;

    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|e| format!("ai.audio transcribe body: {e}"))?;
    if !status.is_success() {
        let snippet: String = response_body.chars().take(400).collect();
        return Err(format!("ai.audio transcribe: HTTP {status}: {snippet}"));
    }
    if let Ok(v) = serde_json::from_str::<Value>(&response_body) {
        if let Some(t) = v.get("text").and_then(Value::as_str) {
            return Ok(t.to_string());
        }
    }
    Ok(response_body)
}

pub async fn videos_generate(
    endpoint: &OpenAiEndpoint,
    model: &str,
    prompt: &str,
    size: Option<&str>,
    seconds: Option<u64>,
    timeout_ms: u64,
) -> Result<VideoJob, String> {
    let mut body = json!({
        "model": model,
        "prompt": prompt,
    });
    if let Some(s) = size.filter(|s| !s.trim().is_empty()) {
        body["size"] = json!(s);
    }
    if let Some(sec) = seconds {
        body["seconds"] = json!(sec.to_string());
    }
    let url = format!("{}/videos", endpoint.base_url);
    let parsed = match post_json(endpoint, &url, &body, timeout_ms).await {
        Ok(v) => v,
        Err(err) if err.contains("HTTP 404") || err.contains("HTTP 405") => {
            return Err(format!(
                "ai.video: provider does not support `/videos` ({err})"
            ));
        }
        Err(err) => return Err(err),
    };
    let id = parsed
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let status = parsed
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("submitted")
        .to_string();
    Ok(VideoJob {
        id,
        status,
        raw: parsed,
    })
}

async fn post_json(
    endpoint: &OpenAiEndpoint,
    url: &str,
    body: &Value,
    timeout_ms: u64,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(1_000)))
        .build()
        .map_err(|e| format!("ai: http client: {e}"))?;
    let response = client
        .post(url)
        .bearer_auth(&endpoint.api_key)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("ai: request failed: {e}"))?;
    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|e| format!("ai: read body: {e}"))?;
    if !status.is_success() {
        let snippet: String = response_body.chars().take(400).collect();
        return Err(format!("ai: HTTP {status}: {snippet}"));
    }
    serde_json::from_str(&response_body).map_err(|e| format!("ai: invalid JSON response: {e}"))
}

fn parse_image_result(parsed: &Value) -> Result<ImageResult, String> {
    let item = parsed
        .pointer("/data/0")
        .ok_or_else(|| "ai.image: response missing data[0]".to_string())?;
    Ok(ImageResult {
        b64_json: item
            .get("b64_json")
            .and_then(Value::as_str)
            .map(str::to_string),
        url: item.get("url").and_then(Value::as_str).map(str::to_string),
        revised_prompt: item
            .get("revised_prompt")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub fn extract_assistant_text(parsed: &Value) -> Option<String> {
    let content = parsed
        .pointer("/choices/0/message/content")
        .or_else(|| parsed.pointer("/choices/0/text"))?;
    match content {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                if let Some(t) = part.get("text").and_then(Value::as_str) {
                    out.push_str(t);
                } else if part.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(t) = part.get("text").and_then(Value::as_str)
                {
                    out.push_str(t);
                }
            }
            let s = out.trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    }
}

pub fn extract_json_object(raw: &str) -> Option<Value> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&raw[start..=end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_content() {
        let v = json!({
            "choices": [{ "message": { "role": "assistant", "content": "  hi  " } }]
        });
        assert_eq!(extract_assistant_text(&v).as_deref(), Some("hi"));
    }
}
