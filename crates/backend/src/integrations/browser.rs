//! Client for the RustBrowser automation server (`rust-browser --automation-server`).
//!
//! The browser renders JavaScript-heavy pages in Servo and returns the final DOM,
//! script results and screenshots. Where it runs ([`shared`]):
//!
//! - `BROWSER_AUTOMATION_URL` (+ `BROWSER_AUTOMATION_TOKEN`): an external server;
//! - `BROWSER_AUTOMATION=off`: disabled;
//! - otherwise BoardDo starts the bundled `vendor/RustBrowser` binary on demand
//!   ([`super::browser_supervisor::BrowserSupervisor`]) and stops it when idle.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use boarddo_shared::v2::{WebError, WebPolicy};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default whole-job budget sent to the browser.
pub const DEFAULT_RENDER_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RenderRequest {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_for: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    pub screenshot: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_js: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_idle_ms: Option<u64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub capture_network: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct RenderResult {
    pub final_url: String,
    pub title: String,
    pub html: String,
    pub html_truncated: bool,
    pub load_complete: bool,
    pub wait_for_found: Option<bool>,
    pub script_result: Option<Value>,
    pub script_error: Option<String>,
    pub screenshot_png_base64: Option<String>,
    pub screenshot_error: Option<String>,
    /// `{requests, responses, hook}` when `capture_network` was requested.
    pub network: Option<Value>,
    pub elapsed_ms: u64,
}

#[async_trait]
pub trait BrowserRenderer: Send + Sync {
    /// False when no automation server is configured (auto mode then stays on HTTP).
    fn is_configured(&self) -> bool;

    async fn render(
        &self,
        request: RenderRequest,
        policy: &WebPolicy,
    ) -> Result<RenderResult, WebError>;

    /// Where the browser runs and whether it is up (for `/api/browser/status`).
    async fn status(&self) -> Value {
        serde_json::json!({ "mode": "off", "available": self.is_configured() })
    }

    /// Stop a browser process this renderer owns (BoardDo shutdown).
    async fn shutdown(&self) {}
}

pub struct NullBrowserRenderer;

#[async_trait]
impl BrowserRenderer for NullBrowserRenderer {
    fn is_configured(&self) -> bool {
        false
    }

    async fn render(
        &self,
        _request: RenderRequest,
        _policy: &WebPolicy,
    ) -> Result<RenderResult, WebError> {
        Err(WebError::Upstream(
            "browser rendering is not configured: start `rust-browser --automation-server \
             127.0.0.1:9333` and set BROWSER_AUTOMATION_URL"
                .into(),
        ))
    }
}

#[derive(Clone)]
pub struct HttpBrowserRenderer {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpBrowserRenderer {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .build()
            .expect("reqwest client");
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            client,
        }
    }
}

impl HttpBrowserRenderer {
    /// `GET /health` of the automation server.
    pub async fn health(&self) -> Result<Value, WebError> {
        let mut builder = self
            .client
            .get(format!("{}/health", self.base_url))
            .timeout(Duration::from_secs(3));
        if let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .await
            .map_err(|e| WebError::Upstream(format!("browser: {e}")))?;
        if !response.status().is_success() {
            return Err(WebError::Upstream(format!(
                "browser: health HTTP {}",
                response.status()
            )));
        }
        response
            .json()
            .await
            .map_err(|e| WebError::Upstream(format!("browser: bad health response: {e}")))
    }
}

fn env_var(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The process-wide renderer: one browser for every board, agent and node.
pub fn shared() -> Arc<dyn BrowserRenderer> {
    static SHARED: OnceLock<Arc<dyn BrowserRenderer>> = OnceLock::new();
    // Unit tests must never start a locally built browser behind the scenes.
    if cfg!(test) {
        return SHARED.get_or_init(|| Arc::new(NullBrowserRenderer)).clone();
    }
    SHARED.get_or_init(from_env).clone()
}

/// Build a renderer from the environment (see the module docs). Prefer [`shared`].
pub fn from_env() -> Arc<dyn BrowserRenderer> {
    if let Some(url) = env_var("BROWSER_AUTOMATION_URL") {
        return Arc::new(HttpBrowserRenderer::new(url, env_var("BROWSER_AUTOMATION_TOKEN")));
    }
    if env_var("BROWSER_AUTOMATION").is_some_and(|v| v.eq_ignore_ascii_case("off")) {
        return Arc::new(NullBrowserRenderer);
    }
    super::browser_supervisor::BrowserSupervisor::from_env()
}

#[async_trait]
impl BrowserRenderer for HttpBrowserRenderer {
    fn is_configured(&self) -> bool {
        true
    }

    async fn status(&self) -> Value {
        let health = self.health().await;
        serde_json::json!({
            "mode": "external",
            "url": self.base_url,
            "available": health.is_ok(),
            "health": health.unwrap_or(Value::Null),
        })
    }

    async fn render(
        &self,
        request: RenderRequest,
        policy: &WebPolicy,
    ) -> Result<RenderResult, WebError> {
        policy.check_browser()?;
        policy.check_url(&request.url)?;
        let budget = request.timeout_ms.unwrap_or(DEFAULT_RENDER_TIMEOUT_MS);
        let mut builder = self
            .client
            .post(format!("{}/render", self.base_url))
            .timeout(Duration::from_millis(budget) + Duration::from_secs(10))
            .json(&request);
        if let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .await
            .map_err(|e| WebError::Upstream(format!("browser: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            let body: Value = response.json().await.unwrap_or_default();
            let message = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("request failed");
            return Err(WebError::Upstream(format!("browser: HTTP {status}: {message}")));
        }
        response
            .json()
            .await
            .map_err(|e| WebError::Upstream(format!("browser: bad response: {e}")))
    }
}

/// Heuristic: an HTTP-fetched page whose content is built by JavaScript.
/// `html` is the raw response body, `text` the text extracted from it.
pub fn looks_like_js_shell(html: &str, text: &str) -> bool {
    let text_len = text.chars().count();
    let lower = html.to_ascii_lowercase();
    let has_scripts = lower.contains("<script");
    if text_len < 300 && has_scripts {
        return true;
    }
    if text_len < 1_500 && noscript_asks_for_js(&lower) {
        return true;
    }
    let squashed: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    text_len < 1_000
        && [
            "id=\"root\"></div>",
            "id=\"app\"></div>",
            "id=\"__next\"></div>",
            "id='root'></div>",
            "id='app'></div>",
        ]
        .iter()
        .any(|p| squashed.contains(p))
}

fn noscript_asks_for_js(lower_html: &str) -> bool {
    let mut rest = lower_html;
    while let Some(start) = rest.find("<noscript") {
        let block = &rest[start..];
        let end = block.find("</noscript>").unwrap_or(block.len());
        if block[..end].contains("javascript") {
            return true;
        }
        rest = &block[end.min(block.len())..];
        if end == block.len() {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_js_shells() {
        let spa = r#"<html><body><div id="root"></div><script src="/app.js"></script></body></html>"#;
        assert!(looks_like_js_shell(spa, ""));
        let noscript = format!(
            "<html><body><noscript>You need to enable JavaScript to run this app.</noscript>\
             <p>{}</p></body></html>",
            "x".repeat(500)
        );
        assert!(looks_like_js_shell(&noscript, &"x".repeat(500)));
        // Short static page without scripts (example.com) is fine as is.
        assert!(!looks_like_js_shell(
            "<html><body><h1>Example Domain</h1><p>For use in examples.</p></body></html>",
            "Example Domain For use in examples."
        ));
        let article = format!("<html><body><script></script><p>{}</p></body></html>", "a".repeat(2000));
        assert!(!looks_like_js_shell(&article, &"a".repeat(2000)));
    }

    #[tokio::test]
    async fn null_renderer_explains_setup() {
        let err = NullBrowserRenderer
            .render(RenderRequest::default(), &WebPolicy::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("BROWSER_AUTOMATION_URL"));
        assert!(!NullBrowserRenderer.is_configured());
    }

    #[test]
    fn request_serializes_only_set_fields() {
        let req = RenderRequest {
            url: "https://example.com".into(),
            wait_for: Some(".feed".into()),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&req).unwrap(),
            serde_json::json!({ "url": "https://example.com", "wait_for": ".feed", "screenshot": false })
        );
    }
}
