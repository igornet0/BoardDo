use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::{Value, json};
use std::time::Duration;

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

/// Outbound HTTP request node.
///
/// Config:
/// ```json
/// {
///   "method": "POST",
///   "url": "https://api.example.com/items",
///   "headers": { "X-Trace": "{{variables.trace}}" },
///   "query": { "limit": "10" },
///   "body": { "name": "{{trigger.name}}" },
///   "timeout_ms": 10000,
///   "auth": { "type": "bearer", "token": "{{variables.token}}" },
///   "retry": { "max": 2, "backoff_ms": 200 }
/// }
/// ```
pub struct HttpRequestNode;

#[async_trait]
impl NodeHandler for HttpRequestNode {
    fn type_id(&self) -> &'static str {
        type_ids::HTTP_REQUEST
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let method = node
            .config
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_uppercase();

        // Optional connection_id → credentials from SecretStore (never logged).
        let mut conn_auth: Option<Value> = None;
        let mut conn_headers: Vec<(String, String)> = Vec::new();
        let mut base_url = String::new();

        if let Some(cid) = node.config.get("connection_id").and_then(Value::as_str) {
            if !cid.is_empty() {
                let resolved = ctx.connections.resolve(cid).await?;
                if let Some(base) = resolved
                    .config
                    .get("base_url")
                    .or_else(|| resolved.config.get("url"))
                    .and_then(Value::as_str)
                {
                    base_url = base.trim_end_matches('/').to_string();
                }
                if let Some(auth) = resolved.credentials.get("auth") {
                    conn_auth = Some(auth.clone());
                } else if let Some(token) = resolved
                    .credentials
                    .get("token")
                    .or_else(|| resolved.credentials.get("api_key"))
                    .and_then(Value::as_str)
                {
                    conn_auth = Some(json!({ "type": "bearer", "token": token }));
                }
                if let Some(Value::Object(hdrs)) = resolved.credentials.get("headers") {
                    for (k, v) in hdrs {
                        let s = match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string().trim_matches('"').to_string(),
                        };
                        conn_headers.push((k.clone(), s));
                    }
                }
            }
        }

        let url_raw = node
            .config
            .get("url")
            .cloned()
            .ok_or_else(|| "http.request: missing `url`".to_string())?;
        let path_or_url = match ctx.resolve_value(url_raw)? {
            Value::String(s) => s,
            other => other.to_string().trim_matches('"').to_string(),
        };
        let url = if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
            path_or_url
        } else if !base_url.is_empty() {
            format!("{}/{}", base_url, path_or_url.trim_start_matches('/'))
        } else {
            path_or_url
        };
        if url.is_empty() {
            return Err("http.request: `url` resolved to empty string".into());
        }

        let timeout_ms = node
            .config
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(10_000)
            .max(1);

        let retry_max = node
            .config
            .get("retry")
            .and_then(|r| r.get("max"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let backoff_ms = node
            .config
            .get("retry")
            .and_then(|r| r.get("backoff_ms"))
            .and_then(Value::as_u64)
            .unwrap_or(250);

        let mut headers = conn_headers;
        headers.extend(resolve_string_map(ctx, node.config.get("headers"))?);
        let query = resolve_string_map(ctx, node.config.get("query"))?;
        let body = match node.config.get("body") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(ctx.resolve_template_result(s)?),
            Some(obj) => Some(resolve_json_templates(ctx, obj.clone())?),
        };

        // Node-level auth overrides connection auth when present.
        let auth_raw = node.config.get("auth").cloned().or(conn_auth);
        let auth_header = resolve_auth(ctx, auth_raw.as_ref())?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(|e| format!("http.request: client error: {e}"))?;

        let mut attempts = 0u64;

        loop {
            attempts += 1;
            match perform_request(
                &client,
                &method,
                &url,
                &headers,
                &query,
                body.as_ref(),
                auth_header.as_deref(),
            )
            .await
            {
                Ok(result) => {
                    ctx.set_variable(
                        format!("{}_status", node.id),
                        json!(result.get("status").cloned().unwrap_or(Value::Null)),
                    );
                    return Ok(NodeOutput::data(result));
                }
                Err(err) => {
                    if attempts > retry_max {
                        return Err(format!(
                            "http.request failed after {attempts} attempt(s): {err}"
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(backoff_ms * attempts)).await;
                }
            }
        }
    }
}

async fn perform_request(
    client: &reqwest::Client,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    query: &[(String, String)],
    body: Option<&Value>,
    auth_header: Option<&str>,
) -> Result<Value, String> {
    let method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| format!("http.request: unsupported method `{method}`"))?;

    let mut req = client.request(method, url);
    for (k, v) in query {
        req = req.query(&[(k.as_str(), v.as_str())]);
    }
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    if let Some(auth) = auth_header {
        req = req.header(reqwest::header::AUTHORIZATION, auth);
    }
    if let Some(b) = body {
        req = req.json(b);
    }

    let response = req
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;

    let status = response.status().as_u16();
    let header_map: serde_json::Map<String, Value> = response
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|s| (k.to_string(), Value::String(s.to_string())))
        })
        .collect();

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("read body error: {e}"))?;

    let body_value = if bytes.is_empty() {
        Value::Null
    } else if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
        v
    } else {
        Value::String(String::from_utf8_lossy(&bytes).into_owned())
    };

    if status >= 400 {
        return Err(format!("HTTP {status}: {body_value}"));
    }

    Ok(json!({
        "status": status,
        "headers": header_map,
        "body": body_value,
    }))
}

fn resolve_string_map(
    ctx: &ExecutionContext,
    raw: Option<&Value>,
) -> Result<Vec<(String, String)>, String> {
    let Some(Value::Object(map)) = raw else {
        return Ok(vec![]);
    };
    let mut out = Vec::new();
    for (k, v) in map {
        let resolved = ctx.resolve_value(v.clone())?;
        let s = match resolved {
            Value::String(s) => s,
            other => value_to_string(&other),
        };
        out.push((k.clone(), s));
    }
    Ok(out)
}

fn resolve_json_templates(ctx: &ExecutionContext, value: Value) -> Result<Value, String> {
    match value {
        Value::String(s) => ctx.resolve_template_result(&s),
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(resolve_json_templates(ctx, item)?);
            }
            Ok(Value::Array(out))
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k, resolve_json_templates(ctx, v)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other),
    }
}

fn resolve_auth(ctx: &ExecutionContext, auth: Option<&Value>) -> Result<Option<String>, String> {
    let Some(auth) = auth else {
        return Ok(None);
    };
    if auth.is_null() {
        return Ok(None);
    }
    let kind = auth.get("type").and_then(Value::as_str).unwrap_or("bearer");
    match kind {
        "bearer" => {
            let token = auth
                .get("token")
                .cloned()
                .ok_or_else(|| "http.request auth.bearer: missing `token`".to_string())?;
            let token = match ctx.resolve_value(token)? {
                Value::String(s) => s,
                other => value_to_string(&other),
            };
            Ok(Some(format!("Bearer {token}")))
        }
        "basic" => {
            let user = auth
                .get("username")
                .cloned()
                .ok_or_else(|| "http.request auth.basic: missing `username`".to_string())?;
            let pass = auth
                .get("password")
                .cloned()
                .unwrap_or(Value::String(String::new()));
            let user = match ctx.resolve_value(user)? {
                Value::String(s) => s,
                other => value_to_string(&other),
            };
            let pass = match ctx.resolve_value(pass)? {
                Value::String(s) => s,
                other => value_to_string(&other),
            };
            let encoded = encode_basic(&user, &pass);
            Ok(Some(format!("Basic {encoded}")))
        }
        "header" => {
            // raw Authorization value
            let value = auth
                .get("value")
                .cloned()
                .ok_or_else(|| "http.request auth.header: missing `value`".to_string())?;
            let value = match ctx.resolve_value(value)? {
                Value::String(s) => s,
                other => value_to_string(&other),
            };
            Ok(Some(value))
        }
        other => Err(format!("http.request: unsupported auth type `{other}`")),
    }
}

fn encode_basic(user: &str, pass: &str) -> String {
    // Minimal base64 for basic auth without adding base64 crate to Phase 2 deps.
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let raw = format!("{user}:{pass}");
    let bytes = raw.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() {
            bytes[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            bytes[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string().trim_matches('"').to_string(),
    }
}
