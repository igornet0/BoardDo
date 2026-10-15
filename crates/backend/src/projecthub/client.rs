//! HTTP client for a running `project-hub-server` (`/api/v1`).

use std::time::Duration;

use reqwest::Method;
use serde_json::Value;

#[derive(Clone)]
pub struct PhClient {
    http: reqwest::Client,
    /// `http://127.0.0.1:PORT/api/v1`
    base: String,
    token: Option<String>,
}

impl std::fmt::Debug for PhClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhClient").field("base", &self.base).finish()
    }
}

impl PhClient {
    /// `url` is the server root (`http://127.0.0.1:7878`).
    pub fn new(url: &str, token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(1900))
                .build()
                .unwrap_or_default(),
            base: format!("{}/api/v1", url.trim_end_matches('/')),
            token: token.filter(|t| !t.is_empty()),
        }
    }

    /// Raw call used by the proxy: returns status + body text.
    pub async fn raw(
        &self,
        method: Method,
        path_and_query: &str,
        body: Option<Vec<u8>>,
    ) -> Result<(u16, String), String> {
        let url = format!("{}/{}", self.base, path_and_query.trim_start_matches('/'));
        let mut req = self.http.request(method, url);
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.header("content-type", "application/json").body(b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("projecthub: request failed: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("projecthub: read body: {e}"))?;
        Ok((status, text))
    }

    pub async fn call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        let bytes = body.map(|b| serde_json::to_vec(&b).unwrap_or_default());
        let (status, text) = self.raw(method, path, bytes).await?;
        let value: Value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text.clone()))
        };
        if (200..300).contains(&status) {
            Ok(value)
        } else {
            let msg = value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| text.chars().take(300).collect());
            Err(format!("projecthub: HTTP {status}: {msg}"))
        }
    }

    pub async fn get(&self, path: &str) -> Result<Value, String> {
        self.call(Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        self.call(Method::POST, path, Some(body)).await
    }

    pub async fn patch(&self, path: &str, body: Value) -> Result<Value, String> {
        self.call(Method::PATCH, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Value) -> Result<Value, String> {
        self.call(Method::PUT, path, Some(body)).await
    }

    pub async fn health(&self) -> Result<Value, String> {
        self.get("health").await
    }

    /// Project by UUID or by (case-insensitive) name / path fragment.
    pub async fn resolve_project(&self, project: &str) -> Result<Value, String> {
        let project = project.trim();
        if project.is_empty() {
            return Err("projecthub: `project` is required (id or name)".into());
        }
        if uuid::Uuid::parse_str(project).is_ok() {
            return self.get(&format!("projects/{project}")).await;
        }
        let found = self
            .get(&format!("projects?q={}&limit=50", urlencode(project)))
            .await?;
        let items = found.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        let lower = project.to_lowercase();
        items
            .iter()
            .find(|p| p["name"].as_str().is_some_and(|n| n.to_lowercase() == lower))
            .or_else(|| items.first())
            .cloned()
            .ok_or_else(|| format!("projecthub: project `{project}` not found"))
    }
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
