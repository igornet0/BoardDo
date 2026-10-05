//! OpenAI-compatible AI nodes (`ai.chat`, `ai.classify`, `ai.image`, …).
//!
//! Credentials come from a connection of type `openai` (`api_key` secret).

use async_trait::async_trait;
use boarddo_shared::{
    Node, openai_base_url, openai_default_model, require_openai_capability, type_ids,
};
use serde_json::{Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};
use crate::integrations::openai::{
    OpenAiEndpoint, audio_speech, audio_transcribe, chat_complete, images_edit, images_generate,
    videos_generate,
};

pub struct AiChatNode;
pub struct AiClassifyNode;
pub struct AiAnalyzeNode;
pub struct AiImageNode;
pub struct AiAudioNode;
pub struct AiVideoNode;

const DEFAULT_ANALYZE_SYSTEM: &str = "You analyze the provided material for a BoardDo agent. \
Reply with concise structured findings: what it says, what is uncertain, and what to do next.";

const DEFAULT_CLASSIFY_SYSTEM: &str = "You classify whether a user question needs live internet search.\n\
Reply with a JSON object only, no markdown:\n\
{\"label\":\"needs_web_search|knowledge\",\"needs_web_search\":true|false,\"query\":\"search query\",\"confidence\":0.0}\n\
Set needs_web_search true when the question needs current facts, prices, news, documentation, comparisons, or anything that may have changed after your knowledge cutoff.\n\
query is a concise web search query (or the original question if it is already specific).";

#[async_trait]
impl NodeHandler for AiChatNode {
    fn type_id(&self) -> &'static str {
        type_ids::AI_CHAT
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let endpoint = openai_from_ctx(ctx, node, "ai.chat").await?;
        let model = resolve_model(node, ctx, &endpoint)?;
        let system = resolve_string_config(node, ctx, "system").unwrap_or_default();
        let mut prompt = resolve_string_config(node, ctx, "prompt")
            .ok_or_else(|| "ai.chat: missing `prompt`".to_string())?;

        if let Some(prefix) = node
            .config
            .get("strip_prefix")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            prompt = strip_command_prefix(&prompt, prefix);
        }

        if prompt.trim().is_empty() {
            return Err(
                "ai.chat: `prompt` resolved to empty text — for Telegram @ai scenarios \
                 write a message like `@ai your question` (do not press Run with empty trigger), \
                 or set a non-empty prompt on the node"
                    .into(),
            );
        }

        let temperature = node
            .config
            .get("temperature")
            .and_then(Value::as_f64)
            .unwrap_or(0.2);
        let max_tokens = node
            .config
            .get("max_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(1024)
            .max(1);
        let timeout_ms = timeout_ms(node);

        let (text, usage, model_used) = chat_complete(
            &endpoint,
            &model,
            &system,
            &prompt,
            temperature,
            max_tokens,
            timeout_ms,
        )
        .await?;

        Ok(NodeOutput::data(json!({
            "text": text,
            "model": model_used,
            "usage": usage,
            "prompt": prompt,
        })))
    }
}

#[async_trait]
impl NodeHandler for AiClassifyNode {
    fn type_id(&self) -> &'static str {
        type_ids::AI_CLASSIFY
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let endpoint = openai_from_ctx(ctx, node, "ai.classify").await?;
        let model = resolve_model(node, ctx, &endpoint)?;
        let text = resolve_classify_text(node, ctx)?;
        if text.trim().is_empty() {
            return Err(
                "ai.classify: `text` resolved to empty — type a question in Ask, or set trigger.text"
                    .into(),
            );
        }

        let labels = classify_labels(node);
        let mut system = resolve_string_config(node, ctx, "system").unwrap_or_default();
        if system.trim().is_empty() {
            system = DEFAULT_CLASSIFY_SYSTEM.to_string();
            if !labels.is_empty() {
                system.push_str("\nAllowed labels: ");
                system.push_str(&labels.join(", "));
            }
        }

        let timeout_ms = timeout_ms(node);
        let (raw, usage, model_used) =
            chat_complete(&endpoint, &model, &system, &text, 0.0, 256, timeout_ms).await?;

        let mut classified = parse_classification(&raw, &text);
        if let Some(obj) = classified.as_object_mut() {
            obj.insert("raw".into(), Value::String(raw));
            obj.insert("model".into(), Value::String(model_used));
            obj.insert("usage".into(), usage);
            obj.insert("text".into(), Value::String(text));
        }
        Ok(NodeOutput::data(classified))
    }
}

#[async_trait]
impl NodeHandler for AiAnalyzeNode {
    fn type_id(&self) -> &'static str {
        type_ids::AI_ANALYZE
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let endpoint = openai_from_ctx(ctx, node, "ai.analyze").await?;
        let model = resolve_model(node, ctx, &endpoint)?;
        let mut prompt = resolve_string_config(node, ctx, "prompt")
            .or_else(|| resolve_string_config(node, ctx, "text"))
            .unwrap_or_default();
        if prompt.trim().is_empty() {
            prompt = match ctx.resolve_template_result("{{trigger.text}}") {
                Ok(Value::String(s)) => s,
                Ok(other) => other.to_string().trim_matches('"').to_string(),
                Err(_) => String::new(),
            };
        }
        if prompt.trim().is_empty() {
            return Err("ai.analyze: `prompt`/`text` resolved to empty".into());
        }
        let mut system = resolve_string_config(node, ctx, "system").unwrap_or_default();
        if system.trim().is_empty() {
            system = DEFAULT_ANALYZE_SYSTEM.to_string();
        }
        let temperature = node
            .config
            .get("temperature")
            .and_then(Value::as_f64)
            .unwrap_or(0.2);
        let max_tokens = node
            .config
            .get("max_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(1024)
            .max(1);
        let timeout_ms = timeout_ms(node);
        let (text, usage, model_used) = chat_complete(
            &endpoint,
            &model,
            &system,
            &prompt,
            temperature,
            max_tokens,
            timeout_ms,
        )
        .await?;
        Ok(NodeOutput::data(json!({
            "text": text,
            "analysis": text,
            "model": model_used,
            "usage": usage,
            "prompt": prompt,
        })))
    }
}

async fn openai_from_ctx(
    ctx: &ExecutionContext,
    node: &Node,
    kind: &str,
) -> Result<OpenAiEndpoint, String> {
    Ok(openai_from_ctx_full(ctx, node, kind).await?.0)
}

async fn openai_from_ctx_full(
    ctx: &ExecutionContext,
    node: &Node,
    kind: &str,
) -> Result<(OpenAiEndpoint, Value), String> {
    let connection_id = node
        .config
        .get("connection_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{kind}: `connection_id` is required"))?;

    let resolved = ctx.connections.resolve(connection_id).await?;
    if resolved.connection_type != "openai" && resolved.connection_type != "ai" {
        return Err(format!(
            "{kind}: connection `{}` must be type `openai` (got `{}`)",
            resolved.name, resolved.connection_type
        ));
    }

    let api_key = resolved
        .credentials
        .get("api_key")
        .or_else(|| resolved.credentials.get("token"))
        .and_then(Value::as_str)
        .or_else(|| {
            resolved
                .credentials
                .get("auth")
                .and_then(|a| a.get("token"))
                .and_then(Value::as_str)
        })
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{kind}: connection secret must include `api_key`"))?
        .to_string();

    let base_url = openai_base_url(&resolved.config);
    let default_model = openai_default_model(&resolved.config);

    Ok((
        OpenAiEndpoint {
            api_key,
            base_url,
            default_model,
        },
        resolved.config,
    ))
}

fn resolve_model(
    node: &Node,
    ctx: &ExecutionContext,
    endpoint: &OpenAiEndpoint,
) -> Result<String, String> {
    match node.config.get("model") {
        Some(v) => match ctx.resolve_value(v.clone())? {
            Value::String(s) if !s.trim().is_empty() => Ok(s),
            Value::String(_) | Value::Null => Ok(endpoint.default_model.clone()),
            other => Ok(other.to_string().trim_matches('"').to_string()),
        },
        None => Ok(endpoint.default_model.clone()),
    }
}

#[async_trait]
impl NodeHandler for AiImageNode {
    fn type_id(&self) -> &'static str {
        type_ids::AI_IMAGE
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let (endpoint, conn_config) = openai_from_ctx_full(ctx, node, "ai.image").await?;
        let model = resolve_model(node, ctx, &endpoint)?;
        let mode = resolve_string_config(node, ctx, "mode")
            .unwrap_or_else(|| "generate".into())
            .to_ascii_lowercase();
        let prompt = resolve_string_config(node, ctx, "prompt")
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "ai.image: missing `prompt`".to_string())?;
        let size = resolve_string_config(node, ctx, "size").unwrap_or_else(|| "1024x1024".into());
        let quality = resolve_string_config(node, ctx, "quality");
        let timeout_ms = timeout_ms(node);

        let result = if mode == "edit" {
            require_openai_capability(&conn_config, &model, "image_edit")?;
            let image_src = resolve_string_config(node, ctx, "image")
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "ai.image edit: missing `image`".to_string())?;
            let (bytes, mime) = ctx.media.read_input_bytes(&image_src).await?;
            let filename = if mime.contains("png") {
                "image.png"
            } else if mime.contains("webp") {
                "image.webp"
            } else {
                "image.png"
            };
            images_edit(
                &endpoint,
                &model,
                &prompt,
                &bytes,
                filename,
                if mime.starts_with("image/") {
                    &mime
                } else {
                    "image/png"
                },
                &size,
                timeout_ms,
            )
            .await?
        } else {
            require_openai_capability(&conn_config, &model, "image_generate")?;
            images_generate(
                &endpoint,
                &model,
                &prompt,
                &size,
                quality.as_deref(),
                timeout_ms,
            )
            .await?
        };

        let stored = if let Some(b64) = &result.b64_json {
            ctx.media.save_base64(b64, "image/png", "png").await?
        } else if let Some(url) = &result.url {
            let (bytes, mime) = ctx.media.read_input_bytes(url).await?;
            let ext = if mime.contains("jpeg") || mime.contains("jpg") {
                "jpg"
            } else if mime.contains("webp") {
                "webp"
            } else {
                "png"
            };
            ctx.media.save_bytes(&bytes, &mime, ext).await?
        } else {
            return Err("ai.image: response had neither b64_json nor url".into());
        };

        Ok(NodeOutput::data(json!({
            "uri": stored.uri,
            "mime": stored.mime,
            "model": model,
            "mode": mode,
            "prompt": prompt,
            "revised_prompt": result.revised_prompt,
        })))
    }
}

#[async_trait]
impl NodeHandler for AiAudioNode {
    fn type_id(&self) -> &'static str {
        type_ids::AI_AUDIO
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let (endpoint, conn_config) = openai_from_ctx_full(ctx, node, "ai.audio").await?;
        let model = resolve_model(node, ctx, &endpoint)?;
        let mode = resolve_string_config(node, ctx, "mode")
            .unwrap_or_else(|| "tts".into())
            .to_ascii_lowercase();
        let timeout_ms = timeout_ms(node);

        if mode == "transcribe" {
            require_openai_capability(&conn_config, &model, "audio_transcribe")?;
            let audio_src = resolve_string_config(node, ctx, "audio")
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "ai.audio transcribe: missing `audio`".to_string())?;
            let (bytes, mime) = ctx.media.read_input_bytes(&audio_src).await?;
            let filename = if mime.contains("wav") {
                "audio.wav"
            } else if mime.contains("ogg") {
                "audio.ogg"
            } else {
                "audio.mp3"
            };
            let text = audio_transcribe(
                &endpoint,
                &model,
                &bytes,
                filename,
                if mime.starts_with("audio/") {
                    &mime
                } else {
                    "audio/mpeg"
                },
                timeout_ms,
            )
            .await?;
            Ok(NodeOutput::data(json!({
                "text": text,
                "model": model,
                "mode": "transcribe",
            })))
        } else {
            require_openai_capability(&conn_config, &model, "audio_generate")?;
            let text = resolve_string_config(node, ctx, "text")
                .or_else(|| resolve_string_config(node, ctx, "prompt"))
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "ai.audio tts: missing `text`".to_string())?;
            let voice = resolve_string_config(node, ctx, "voice").unwrap_or_else(|| "alloy".into());
            let format = resolve_string_config(node, ctx, "format").unwrap_or_else(|| "mp3".into());
            let bytes = audio_speech(&endpoint, &model, &text, &voice, &format, timeout_ms).await?;
            let mime = match format.as_str() {
                "wav" => "audio/wav",
                "opus" | "ogg" => "audio/ogg",
                "aac" => "audio/aac",
                "flac" => "audio/flac",
                _ => "audio/mpeg",
            };
            let stored = ctx.media.save_bytes(&bytes, mime, &format).await?;
            Ok(NodeOutput::data(json!({
                "uri": stored.uri,
                "mime": stored.mime,
                "text": text,
                "model": model,
                "mode": "tts",
                "voice": voice,
            })))
        }
    }
}

#[async_trait]
impl NodeHandler for AiVideoNode {
    fn type_id(&self) -> &'static str {
        type_ids::AI_VIDEO
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let (endpoint, conn_config) = openai_from_ctx_full(ctx, node, "ai.video").await?;
        let model = resolve_model(node, ctx, &endpoint)?;
        require_openai_capability(&conn_config, &model, "video_generate")?;
        let prompt = resolve_string_config(node, ctx, "prompt")
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "ai.video: missing `prompt`".to_string())?;
        let size = resolve_string_config(node, ctx, "size");
        let seconds = node
            .config
            .get("seconds")
            .and_then(Value::as_u64)
            .or_else(|| {
                resolve_string_config(node, ctx, "seconds").and_then(|s| s.parse().ok())
            });
        let timeout_ms = timeout_ms(node).max(60_000);
        let job = videos_generate(
            &endpoint,
            &model,
            &prompt,
            size.as_deref(),
            seconds,
            timeout_ms,
        )
        .await?;

        Ok(NodeOutput::data(json!({
            "job_id": job.id,
            "status": job.status,
            "model": model,
            "prompt": prompt,
            "raw": job.raw,
            "uri": Value::Null,
            "note": "Video providers often return an async job; poll or download when status is completed.",
        })))
    }
}

fn resolve_string_config(node: &Node, ctx: &ExecutionContext, key: &str) -> Option<String> {
    let raw = node.config.get(key)?.clone();
    match ctx.resolve_value(raw) {
        Ok(Value::String(s)) => Some(s),
        Ok(Value::Null) => Some(String::new()),
        Ok(other) => Some(other.to_string().trim_matches('"').to_string()),
        Err(_) => None,
    }
}

fn resolve_classify_text(node: &Node, ctx: &ExecutionContext) -> Result<String, String> {
    if let Some(raw) = node.config.get("text").cloned() {
        return match ctx.resolve_value(raw)? {
            Value::String(s) => Ok(s),
            Value::Null => Ok(String::new()),
            other => Ok(other.to_string().trim_matches('"').to_string()),
        };
    }
    match ctx.resolve_template_result("{{trigger.text}}") {
        Ok(Value::String(s)) => Ok(s),
        Ok(Value::Null) => Ok(String::new()),
        Ok(other) => Ok(other.to_string().trim_matches('"').to_string()),
        Err(_) => Ok(String::new()),
    }
}

fn classify_labels(node: &Node) -> Vec<String> {
    match node.config.get("labels") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec!["needs_web_search".into(), "knowledge".into()],
    }
}

fn timeout_ms(node: &Node) -> u64 {
    node.config
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(60_000)
        .max(1_000)
}

fn strip_command_prefix(text: &str, prefix: &str) -> String {
    let trimmed = text.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    let pref = prefix.trim().to_ascii_lowercase();
    if !lower.starts_with(&pref) {
        return text.to_string();
    }
    let mut rest = &trimmed[prefix.trim().len()..];
    rest = rest.trim_start_matches([':', ',', '-', ' ', '\t']);
    rest.to_string()
}

pub(crate) fn parse_classification(raw: &str, original_query: &str) -> Value {
    if let Some(obj) = extract_json_object(raw) {
        let needs = obj
            .get("needs_web_search")
            .and_then(json_truthy)
            .or_else(|| {
                obj.get("label")
                    .and_then(Value::as_str)
                    .map(|l| l.eq_ignore_ascii_case("needs_web_search"))
            })
            .unwrap_or(false);
        let query = obj
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(original_query)
            .to_string();
        let label = obj
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if needs {
                    "needs_web_search".into()
                } else {
                    "knowledge".into()
                }
            });
        let confidence = obj
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(if needs { 0.7 } else { 0.6 });
        return json!({
            "label": label,
            "needs_web_search": needs,
            "query": query,
            "confidence": confidence,
        });
    }

    let lower = raw.to_ascii_lowercase();
    let needs = lower.contains("\"needs_web_search\": true") || lower.contains("needs_web_search");
    json!({
        "label": if needs { "needs_web_search" } else { "knowledge" },
        "needs_web_search": needs,
        "query": original_query,
        "confidence": 0.4,
    })
}

fn json_truthy(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(n.as_f64().unwrap_or(0.0) != 0.0),
        Value::String(s) => {
            let s = s.trim().to_ascii_lowercase();
            if s == "true" || s == "yes" || s == "1" {
                Some(true)
            } else if s == "false" || s == "no" || s == "0" {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn extract_json_object(raw: &str) -> Option<Value> {
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
    fn strips_ai_prefix() {
        assert_eq!(
            strip_command_prefix("@ai how many people live in Russia?", "@ai"),
            "how many people live in Russia?"
        );
        assert_eq!(strip_command_prefix("@AI: hello", "@ai"), "hello");
        assert_eq!(strip_command_prefix("hello", "@ai"), "hello");
    }

    #[test]
    fn extracts_openai_content() {
        let v = json!({
            "choices": [{ "message": { "role": "assistant", "content": "  hi  " } }]
        });
        assert_eq!(
            crate::integrations::openai::extract_assistant_text(&v).as_deref(),
            Some("hi")
        );
    }

    #[test]
    fn parse_classification_from_fenced_json() {
        let raw = "```json\n{\"label\":\"needs_web_search\",\"needs_web_search\":true,\"query\":\"best VPS under $20\",\"confidence\":0.91}\n```";
        let v = parse_classification(raw, "find a cheap vps");
        assert_eq!(v["needs_web_search"], true);
        assert_eq!(v["query"], "best VPS under $20");
        assert_eq!(v["label"], "needs_web_search");
    }

    #[test]
    fn parse_classification_knowledge_label() {
        let v = parse_classification(
            r#"{"label":"knowledge","needs_web_search":false,"query":"what is 2+2","confidence":0.99}"#,
            "what is 2+2",
        );
        assert_eq!(v["needs_web_search"], false);
        assert_eq!(v["label"], "knowledge");
    }

    #[test]
    fn parse_classification_falls_back_to_original_query() {
        let v = parse_classification(r#"{"needs_web_search":true}"#, "rust sqlite vs postgres");
        assert_eq!(v["needs_web_search"], true);
        assert_eq!(v["query"], "rust sqlite vs postgres");
    }
}
