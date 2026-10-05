//! Typed `web.*` action nodes.

use std::sync::Arc;

use async_trait::async_trait;
use boarddo_shared::v2::{
    WebError, WebFetchRequest, WebFetchResult, WebPolicy, WebSearchHit, WebSearchRequest,
};
use boarddo_shared::{Node, type_ids};
use chrono::{DateTime, Local, Utc};
use futures::future::join_all;
use serde_json::{Map, Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};
use crate::integrations::web::{
    Freshness, WebGateway, looks_like_news_query, parse_published, read_fetched,
};
use crate::integrations::browser::{self, BrowserRenderer, RenderRequest};
use crate::integrations::web_reader::{self, PageContent, ReadOptions};
use crate::integrations::{web_network, web_structured};
use sha2::{Digest, Sha256};

/// Pages read after a search when the node config does not say otherwise.
const DEFAULT_READ_PAGES: u64 = 3;
const DEFAULT_PAGE_CHARS: u64 = 3_000;
/// Per-page timeout for reading search results: a slow site must not stall the board.
pub(super) const PAGE_TIMEOUT_MS: u64 = 10_000;
/// Auto freshness widens the window until at least this many hits are found.
const MIN_FRESH_HITS: usize = 3;

pub struct WebSearchNode;

#[async_trait]
impl NodeHandler for WebSearchNode {
    fn type_id(&self) -> &'static str {
        type_ids::WEB_SEARCH
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let query = resolve_query(node, ctx)?;
        if query.trim().is_empty() {
            return Err("web.search: query resolved to empty text".into());
        }
        let limit = node
            .config
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(8)
            .clamp(1, 20) as u32;
        let read_pages = config_u64(node, "read_pages", DEFAULT_READ_PAGES).min(10) as usize;
        let page_chars = config_u64(node, "page_chars", DEFAULT_PAGE_CHARS).clamp(200, 20_000);
        let mode = node
            .config
            .get("freshness")
            .and_then(Value::as_str)
            .unwrap_or("auto")
            .trim()
            .to_ascii_lowercase();
        let (mut freshness, auto) = match mode.as_str() {
            "" | "auto" => (auto_freshness(&query), true),
            "any" | "none" | "all" => (None, false),
            other => (Freshness::parse(other), false),
        };

        // Auto mode widens day → week → month while there are too few fresh hits.
        let policy = WebPolicy::default();
        let min_hits = MIN_FRESH_HITS.min(limit as usize);
        let hits = loop {
            let result = ctx
                .web
                .search(
                    WebSearchRequest {
                        query: query.clone(),
                        limit: Some(limit),
                        freshness: freshness.map(|f| f.as_str().to_string()),
                    },
                    &policy,
                )
                .await
                .map_err(|e| format!("web.search: {e}"))?;
            let wider = freshness
                .and_then(Freshness::wider)
                .filter(|w| *w <= Freshness::Month);
            match wider {
                Some(w) if auto && result.results.len() < min_hits => freshness = Some(w),
                _ => break result.results,
            }
        };

        let now = Utc::now();
        let cutoff = freshness.map(|f| f.cutoff(now));
        let opts = ReadOptions {
            max_chars: page_chars as usize,
            selector: None,
            include_links: false,
        };
        let reader = PageReader::new(
            ctx,
            RenderMode::from_config(node),
            BrowserExtras::default(),
            PAGE_TIMEOUT_MS,
        );
        let pages = read_search_hits(&reader, &hits, read_pages, &opts, cutoff).await;

        let mut results = Vec::with_capacity(hits.len());
        let mut sources = Vec::with_capacity(hits.len());
        let mut outdated = 0;
        for hit in &hits {
            let mut item = json!(hit);
            let page = pages.iter().find(|p| p["source_url"] == hit.url.as_str());
            // The page itself says it is older than the window: drop the hit.
            if page.is_some_and(|p| p.get("outdated").is_some()) {
                outdated += 1;
                continue;
            }
            let mut body = hit.snippet.clone();
            let mut published = hit.published.clone();
            if let Some(page) = page {
                if let Some(text) = page.get("text").and_then(Value::as_str) {
                    item["content"] = json!(text);
                    body = text.to_string();
                }
                if let Some(err) = page.get("error") {
                    item["content_error"] = err.clone();
                }
                if published.is_none() {
                    published = page
                        .get("published")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    item["published"] = json!(published);
                }
            }
            sources.push(ContextSource {
                title: hit.title.clone(),
                url: hit.url.clone(),
                published,
                body,
            });
            results.push(item);
        }
        let pages_read = pages
            .iter()
            .filter(|p| p.get("error").is_none() && p.get("outdated").is_none())
            .count();

        Ok(NodeOutput::data(json!({
            "query": query,
            "today": Local::now().format("%Y-%m-%d").to_string(),
            "freshness": freshness.map(Freshness::as_str),
            "since": cutoff.map(|c| c.format("%Y-%m-%d").to_string()),
            "results": results,
            "pages": pages,
            "pages_read": pages_read,
            "outdated_dropped": outdated,
            "context": build_context(&sources, freshness, now),
        })))
    }
}

/// `web.open` — read one or several pages into clean text, metadata and CSS-selected data.
pub struct WebOpenNode;

#[async_trait]
impl NodeHandler for WebOpenNode {
    fn type_id(&self) -> &'static str {
        type_ids::WEB_OPEN
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let raw = node.config.get("url").cloned().unwrap_or(Value::Null);
        let resolved = ctx.resolve_value(raw)?;
        let max_pages = config_u64(node, "limit", 5).clamp(1, 10) as usize;
        let mut urls = urls_from_value(&resolved);
        urls.truncate(max_pages);
        if urls.is_empty() {
            return Err("web.open: url resolved to empty text".into());
        }

        let selector = node
            .config
            .get("selector")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let opts = ReadOptions {
            max_chars: config_u64(node, "max_chars", 8_000).clamp(200, 100_000) as usize,
            selector,
            include_links: node
                .config
                .get("include_links")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        };
        let fields = parse_fields(node.config.get("fields"));

        let reader = PageReader::new(
            ctx,
            RenderMode::from_config(node),
            BrowserExtras::from_config(node),
            PAGE_TIMEOUT_MS * 2,
        )
        .with_structured(
            node.config
                .get("structured")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        );
        let mut pages: Vec<Value> = join_all(
            urls.iter()
                .map(|url| reader.read(url, &opts, fields.as_ref())),
        )
        .await
        .into_iter()
        .zip(&urls)
        .map(|(res, url)| match res {
            Ok(page) => page,
            Err(e) => e.to_json(url),
        })
        .collect();
        // Screenshots go to the media store; the board gets a URI, not megabytes of base64.
        for page in &mut pages {
            let Some(png) = page
                .as_object_mut()
                .and_then(|p| p.remove("screenshot_png_base64"))
            else {
                continue;
            };
            match ctx
                .media
                .save_base64(png.as_str().unwrap_or_default(), "image/png", "png")
                .await
            {
                Ok(stored) => page["screenshot"] = json!(stored.uri),
                Err(e) => page["screenshot_error"] = json!(e),
            }
        }

        let Some(first) = pages.iter().find(|p| p.get("error").is_none()).cloned() else {
            let errors: Vec<&str> = pages
                .iter()
                .filter_map(|p| p.get("error").and_then(Value::as_str))
                .collect();
            return Err(format!("web.open: {}", errors.join("; ")));
        };

        let sources: Vec<ContextSource> = pages
            .iter()
            .filter(|p| p.get("error").is_none())
            .map(|p| ContextSource {
                title: str_field(p, "title"),
                url: str_field(p, "url"),
                published: p
                    .get("published")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                body: str_field(p, "text"),
            })
            .collect();

        let mut out = match first {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        out.insert(
            "context".into(),
            json!(build_context(&sources, None, Utc::now())),
        );
        out.insert("count".into(), json!(sources.len()));
        out.insert("pages".into(), Value::Array(pages));
        Ok(NodeOutput::data(Value::Object(out)))
    }
}

pub struct WebFetchNode;

#[async_trait]
impl NodeHandler for WebFetchNode {
    fn type_id(&self) -> &'static str {
        type_ids::WEB_FETCH
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let url = resolve_url(node, ctx)?;
        if url.trim().is_empty() {
            return Err("web.fetch: url resolved to empty text".into());
        }
        let method = node
            .config
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_string();
        let mut headers = std::collections::BTreeMap::new();
        if let Some(Value::Object(map)) = node.config.get("headers") {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    headers.insert(k.clone(), s.to_string());
                }
            }
        }
        let policy = WebPolicy::default();
        let result = ctx
            .web
            .fetch(
                WebFetchRequest {
                    url: url.clone(),
                    method,
                    headers,
                },
                &policy,
            )
            .await
            .map_err(|e| format!("web.fetch: {e}"))?;

        Ok(NodeOutput::data(json!({
            "url": result.url,
            "status": result.status,
            "content_type": result.content_type,
            "body": result.body,
            "truncated": result.truncated,
        })))
    }
}

/// Where a page is rendered: plain HTTP, the headless browser, or HTTP with a
/// browser fallback for JavaScript-built pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderMode {
    Auto,
    Http,
    Browser,
}

impl RenderMode {
    pub(super) fn from_config(node: &Node) -> Self {
        match node
            .config
            .get("render")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("http") => Self::Http,
            Some("browser") => Self::Browser,
            _ => Self::Auto,
        }
    }
}

/// Browser-only page actions (`web.open`).
#[derive(Debug, Clone, Default)]
pub(super) struct BrowserExtras {
    wait_for: Option<String>,
    wait_text: Option<String>,
    wait_js: Option<String>,
    network_idle_ms: Option<u64>,
    wait_ms: Option<u64>,
    timeout_ms: Option<u64>,
    script: Option<String>,
    screenshot: bool,
    /// Record fetch/XHR and report `api_candidates`.
    capture_network: bool,
    /// GET discovered JSON endpoints whose bodies the browser did not capture.
    fetch_api: bool,
}

impl BrowserExtras {
    pub(super) fn from_config(node: &Node) -> Self {
        let text = |key: &str| {
            node.config
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let flag = |key: &str| node.config.get(key).and_then(Value::as_bool).unwrap_or(false);
        Self {
            wait_for: text("wait_for"),
            wait_text: text("wait_text"),
            wait_js: text("wait_js"),
            network_idle_ms: node
                .config
                .get("network_idle_ms")
                .and_then(Value::as_u64)
                .filter(|ms| *ms > 0),
            wait_ms: node.config.get("wait_ms").and_then(Value::as_u64),
            timeout_ms: node.config.get("timeout_ms").and_then(Value::as_u64),
            script: text("script"),
            screenshot: flag("screenshot"),
            capture_network: flag("capture_network"),
            fetch_api: flag("fetch_api"),
        }
    }

    fn needs_browser(&self) -> bool {
        self.wait_for.is_some()
            || self.wait_text.is_some()
            || self.wait_js.is_some()
            || self.network_idle_ms.is_some()
            || self.script.is_some()
            || self.screenshot
            || self.capture_network
    }
}

/// Why a page could not be read, in a shape agents can act on (`retryable`, `retry_after`).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PageError {
    message: String,
    /// `blocked` | `rate_limited` | `not_found` | `unavailable` | `http_error` |
    /// `challenge` | `denied` | `network_error` | `error`
    kind: &'static str,
    http_status: Option<u16>,
    retryable: bool,
    retry_after: Option<u64>,
}

impl PageError {
    fn other(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: "error",
            http_status: None,
            retryable: false,
            retry_after: None,
        }
    }

    fn from_web(err: WebError) -> Self {
        let (kind, retryable) = match &err {
            WebError::UrlDenied(_) | WebError::InvalidUrl(_) | WebError::MethodDenied(_) => {
                ("denied", false)
            }
            WebError::Upstream(_) => ("network_error", true),
            _ => ("error", false),
        };
        Self {
            message: err.to_string(),
            kind,
            http_status: None,
            retryable,
            retry_after: None,
        }
    }

    /// Non-2xx statuses. Blocks are final: there is no retry through another client.
    fn from_status(status: u16, retry_after: Option<u64>, url: &str) -> Option<Self> {
        let (kind, retryable, what) = match status {
            200..=299 => return None,
            401 | 403 | 451 => ("blocked", false, "access denied"),
            404 | 410 => ("not_found", false, "not found"),
            429 => ("rate_limited", true, "rate limited"),
            502..=504 => ("unavailable", true, "temporarily unavailable"),
            _ => ("http_error", false, "request failed"),
        };
        Some(Self {
            message: format!("{what} (HTTP {status}) for {url}"),
            kind,
            http_status: Some(status),
            retryable,
            retry_after,
        })
    }

    pub(super) fn to_json(&self, url: &str) -> Value {
        json!({
            "source_url": url,
            "url": url,
            "error": self.message,
            "status": self.kind,
            "http_status": self.http_status,
            "retryable": self.retryable,
            "retry_after": self.retry_after,
        })
    }
}

/// Bot-check interstitials served with 200 (Cloudflare, DDoS-Guard, captchas).
/// They are reported, never solved or bypassed. `page.text` may be clipped to
/// `max_chars`, so a truncated page is never "short".
fn looks_like_challenge(html: &str, page: &PageContent) -> bool {
    let lower = html.to_ascii_lowercase();
    if [
        "/cdn-cgi/challenge-platform",
        "cf-chl-",
        "ddos-guard",
        "attention required! | cloudflare",
    ]
    .iter()
    .any(|m| lower.contains(m))
    {
        return true;
    }
    !page.truncated
        && page.text.chars().count() < 1_500
        && ["captcha", "verify you are human", "just a moment...", "проверка браузера"]
            .iter()
            .any(|m| lower.contains(m))
}

/// Discovered-but-uncaptured API endpoints fetched directly per page.
const MAX_API_REFETCH: usize = 5;

/// Retries for 429 / 502–504: at most this many, never waiting longer than below.
const MAX_RETRIES: u32 = 2;
const MAX_RETRY_WAIT_SECS: u64 = 10;

/// Reads pages into the board-facing JSON shape, over HTTP or the headless browser.
#[derive(Clone)]
pub(super) struct PageReader {
    web: Arc<dyn WebGateway>,
    browser: Arc<dyn BrowserRenderer>,
    policy: WebPolicy,
    mode: RenderMode,
    extras: BrowserExtras,
    /// Attach JSON-LD / meta / microdata / tables / embedded JSON as `structured`.
    structured: bool,
}

impl PageReader {
    pub(super) fn new(
        ctx: &ExecutionContext,
        mode: RenderMode,
        extras: BrowserExtras,
        timeout_ms: u64,
    ) -> Self {
        let mut policy = WebPolicy::default();
        policy.timeout_ms = timeout_ms;
        policy.browser_enabled = ctx.browser.is_configured();
        Self {
            web: ctx.web.clone(),
            browser: ctx.browser.clone(),
            policy,
            mode,
            extras,
            structured: false,
        }
    }

    pub(super) fn with_structured(mut self, on: bool) -> Self {
        self.structured = on;
        self
    }

    pub(super) async fn read(
        &self,
        url: &str,
        opts: &ReadOptions,
        fields: Option<&Map<String, Value>>,
    ) -> Result<Value, PageError> {
        let browser_ready = self.browser.is_configured();
        if self.mode == RenderMode::Browser
            || (self.mode == RenderMode::Auto && browser_ready && self.extras.needs_browser())
        {
            return self.read_with_browser(url, opts, fields).await;
        }

        // Blocked / failed responses (403, 429, …) end here: the browser is for
        // JavaScript-built pages, not for getting around a site's refusal.
        let fetched = self.fetch_with_retries(url).await?;
        let page = read_fetched(&fetched, opts).map_err(PageError::from_web)?;
        let is_html = web_reader::is_html(fetched.content_type.as_deref(), &fetched.body);
        if is_html && looks_like_challenge(&fetched.body, &page) {
            return Err(PageError {
                message: format!("bot-check page for {url} (not bypassed)"),
                kind: "challenge",
                http_status: Some(fetched.status),
                retryable: false,
                retry_after: None,
            });
        }
        let source = PageSource {
            source_url: url,
            final_url: &fetched.url,
            status: fetched.status,
            method: "http",
            body: &fetched.body,
            is_html,
        };
        let mut value = self.page_value(&source, &page, fields)?;

        if self.mode == RenderMode::Auto
            && browser_ready
            && is_html
            && browser::looks_like_js_shell(&fetched.body, &page.text)
        {
            match self.read_with_browser(url, opts, fields).await {
                Ok(rendered) => return Ok(rendered),
                Err(e) => value["browser_error"] = json!(e.message),
            }
        }
        Ok(value)
    }

    /// GET with bounded retries for 429 / 502–504, honouring `Retry-After`.
    async fn fetch_with_retries(&self, url: &str) -> Result<WebFetchResult, PageError> {
        let mut attempt = 0;
        loop {
            let fetched = self
                .web
                .fetch(
                    WebFetchRequest {
                        url: url.to_string(),
                        ..Default::default()
                    },
                    &self.policy,
                )
                .await
                .map_err(PageError::from_web)?;
            let Some(err) = PageError::from_status(fetched.status, fetched.retry_after, url) else {
                return Ok(fetched);
            };
            let wait = err.retry_after.unwrap_or(1 << attempt);
            if !err.retryable || attempt >= MAX_RETRIES || wait > MAX_RETRY_WAIT_SECS {
                return Err(err);
            }
            attempt += 1;
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        }
    }

    async fn read_with_browser(
        &self,
        url: &str,
        opts: &ReadOptions,
        fields: Option<&Map<String, Value>>,
    ) -> Result<Value, PageError> {
        let rendered = self
            .browser
            .render(
                RenderRequest {
                    url: url.to_string(),
                    wait_for: self.extras.wait_for.clone(),
                    wait_text: self.extras.wait_text.clone(),
                    wait_js: self.extras.wait_js.clone(),
                    network_idle_ms: self.extras.network_idle_ms,
                    wait_ms: self.extras.wait_ms,
                    timeout_ms: self.extras.timeout_ms,
                    script: self.extras.script.clone(),
                    screenshot: self.extras.screenshot,
                    capture_network: self.extras.capture_network,
                },
                &self.policy,
            )
            .await
            .map_err(PageError::from_web)?;
        let final_url = if rendered.final_url.is_empty() {
            url.to_string()
        } else {
            rendered.final_url.clone()
        };
        let mut page =
            web_reader::read_html(&rendered.html, &final_url, opts).map_err(PageError::other)?;
        if page.title.is_empty() {
            page.title = rendered.title.clone();
        }
        if looks_like_challenge(&rendered.html, &page) {
            return Err(PageError {
                message: format!("bot-check page for {url} (not bypassed)"),
                kind: "challenge",
                http_status: None,
                retryable: false,
                retry_after: None,
            });
        }
        let source = PageSource {
            source_url: url,
            final_url: &final_url,
            status: 200,
            method: "browser",
            body: &rendered.html,
            is_html: true,
        };
        let mut value = self.page_value(&source, &page, fields)?;
        value["load_complete"] = json!(rendered.load_complete);
        if let Some(found) = rendered.wait_for_found {
            value["wait_for_found"] = json!(found);
        }
        if self.extras.script.is_some() {
            value["script_result"] = rendered.script_result.unwrap_or(Value::Null);
            if let Some(e) = rendered.script_error {
                value["script_error"] = json!(e);
            }
        }
        if let Some(png) = rendered.screenshot_png_base64 {
            value["screenshot_png_base64"] = json!(png);
        }
        if let Some(e) = rendered.screenshot_error {
            value["screenshot_error"] = json!(e);
        }
        if let Some(network) = &rendered.network {
            let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            let mut summary = web_network::summarize(network, &final_url, &fetched_at);
            if self.extras.fetch_api {
                self.refetch_api(&mut summary.api_candidates, &fetched_at).await;
            }
            value["network"] = summary.network;
            value["api_candidates"] = json!(summary.api_candidates);
        }
        Ok(value)
    }

    /// GET up to [`MAX_API_REFETCH`] JSON endpoints seen in the network log but not captured.
    /// Same policy as any fetch: GET only, no private hosts, bounded size.
    async fn refetch_api(&self, candidates: &mut [Value], fetched_at: &str) {
        let pending: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c["captured"] == false && c["method"] == "GET")
            .map(|(i, _)| i)
            .take(MAX_API_REFETCH)
            .collect();
        let bodies = join_all(pending.iter().map(|&i| {
            let url = candidates[i]["url"].as_str().unwrap_or_default().to_string();
            async move {
                self.web
                    .fetch(
                        WebFetchRequest {
                            url,
                            ..Default::default()
                        },
                        &self.policy,
                    )
                    .await
            }
        }))
        .await;
        for (i, res) in pending.into_iter().zip(bodies) {
            match res {
                Ok(f) if (200..300).contains(&f.status) => {
                    if !web_network::attach_refetch(&mut candidates[i], &f.body, fetched_at) {
                        candidates[i]["refetch_error"] = json!("response is not JSON");
                    }
                }
                Ok(f) => {
                    candidates[i]["refetch_error"] = json!(format!("HTTP {}", f.status));
                }
                Err(e) => candidates[i]["refetch_error"] = json!(e.to_string()),
            }
        }
    }

    /// Page JSON with provenance, optional `structured`, CSS `data` and per-field `evidence`.
    fn page_value(
        &self,
        source: &PageSource<'_>,
        page: &PageContent,
        fields: Option<&Map<String, Value>>,
    ) -> Result<Value, PageError> {
        let fetched_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let content_hash = format!("sha256:{:x}", Sha256::digest(source.body.as_bytes()));
        let mut value = web_reader::page_json(source.final_url, source.status, page);
        value["source_url"] = json!(source.source_url);
        value["render"] = json!(source.method);
        value["provenance"] = json!({
            "url": source.final_url,
            "source_url": source.source_url,
            "method": source.method,
            "http_status": source.status,
            "fetched_at": fetched_at,
            "content_hash": content_hash,
        });
        let html = source.is_html.then_some(source.body);

        if let Some(html) = html {
            if self.structured || page.published.is_none() {
                let structured = web_structured::extract_structured(html, source.final_url);
                if page.published.is_none() {
                    if let Some(date) = structured.date_published() {
                        value["published"] = json!(date);
                    }
                }
                if self.structured && !structured.is_empty() {
                    value["structured"] = structured.to_json();
                }
            }
        }

        if let Some(fields) = fields {
            let data = match html {
                Some(html) => web_reader::select_fields(html, source.final_url, fields)
                    .map_err(PageError::other)?,
                None => Value::Object(Map::new()),
            };
            let dom_method = format!("{}_dom", source.method);
            let evidence: Map<String, Value> = fields
                .iter()
                .filter_map(|(name, selector)| {
                    let v = data.get(name).filter(|v| !v.is_null())?;
                    Some((
                        name.clone(),
                        json!({
                            "value": v,
                            "source": {
                                "url": source.final_url,
                                "method": dom_method,
                                "selector": selector,
                                "timestamp": fetched_at,
                                "content_hash": content_hash,
                            }
                        }),
                    ))
                })
                .collect();
            value["data"] = data;
            value["evidence"] = Value::Object(evidence);
        }
        Ok(value)
    }
}

/// Where a page body came from (for provenance / evidence).
struct PageSource<'a> {
    source_url: &'a str,
    final_url: &'a str,
    status: u16,
    method: &'static str,
    body: &'a str,
    is_html: bool,
}

/// Read up to `count` search hits in parallel; failed pages are replaced by the next hits.
async fn read_search_hits(
    reader: &PageReader,
    hits: &[WebSearchHit],
    count: usize,
    opts: &ReadOptions,
    cutoff: Option<DateTime<Utc>>,
) -> Vec<Value> {
    if count == 0 {
        return Vec::new();
    }
    let mut candidates: Vec<&WebSearchHit> = hits
        .iter()
        .filter(|h| h.url.starts_with("http://") || h.url.starts_with("https://"))
        .collect();
    // Google News RSS links are JS redirects with no readable article: read them last.
    candidates.sort_by_key(|h| h.url.contains("://news.google.com/"));

    let mut pages = Vec::new();
    let mut next = 0;
    // Two rounds: the first reads `count` hits, the second backfills failures.
    for _ in 0..2 {
        let ok = pages
            .iter()
            .filter(|p: &&Value| p.get("error").is_none())
            .count();
        let want = count.saturating_sub(ok);
        if want == 0 || next >= candidates.len() {
            break;
        }
        let batch = &candidates[next..(next + want).min(candidates.len())];
        next += batch.len();
        let results = join_all(
            batch
                .iter()
                .map(|hit| reader.read(&hit.url, opts, None)),
        )
        .await;
        for (hit, res) in batch.iter().zip(results) {
            pages.push(match res {
                Ok(mut page) => {
                    let page_date = page
                        .get("published")
                        .and_then(Value::as_str)
                        .and_then(|p| parse_published(p, Utc::now()));
                    if let (Some(date), Some(cutoff)) = (page_date, cutoff) {
                        if date < cutoff {
                            // Counts as a miss so the next hit gets read instead.
                            page["outdated"] = json!(true);
                            page["error"] = json!(format!(
                                "published {}, before {}",
                                date.format("%Y-%m-%d"),
                                cutoff.format("%Y-%m-%d")
                            ));
                        }
                    }
                    page
                }
                Err(e) => e.to_json(&hit.url),
            });
        }
    }
    pages
}

struct ContextSource {
    title: String,
    url: String,
    published: Option<String>,
    body: String,
}

/// `day` for "today"-style questions, `week` for news / "latest", else no window.
fn auto_freshness(query: &str) -> Option<Freshness> {
    let q = query.to_lowercase();
    if ["сегодня", "today", "за сутки", "last 24"].iter().any(|k| q.contains(k)) {
        Some(Freshness::Day)
    } else if looks_like_news_query(&q) {
        Some(Freshness::Week)
    } else {
        None
    }
}

/// "2026-10-04 08:30 UTC (1 day ago)"; unparseable dates are shown as-is.
fn describe_published(raw: &str, now: DateTime<Utc>) -> String {
    let Some(date) = parse_published(raw, now) else {
        return raw.to_string();
    };
    let age = now.signed_duration_since(date);
    let ago = if age.num_hours() < 1 {
        "less than an hour ago".to_string()
    } else if age.num_hours() < 24 {
        plural(age.num_hours(), "hour")
    } else {
        plural(age.num_days(), "day")
    };
    format!("{} UTC ({ago})", date.format("%Y-%m-%d %H:%M"))
}

fn plural(n: i64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

/// Numbered plain-text digest of sources, ready to paste into an LLM prompt.
/// Starts with today's date and the search window so the model can judge recency.
fn build_context(
    sources: &[ContextSource],
    freshness: Option<Freshness>,
    now: DateTime<Utc>,
) -> String {
    let local = now.with_timezone(&Local);
    let mut header = format!(
        "Today: {} ({}), {}",
        local.format("%Y-%m-%d"),
        local.format("%A"),
        local.format("%H:%M %:z")
    );
    match freshness {
        Some(f) => header.push_str(&format!(
            "\nPeriod: last {} days (since {})",
            f.days(),
            f.cutoff(now).format("%Y-%m-%d")
        )),
        None => header.push_str("\nPeriod: any time"),
    }
    let body = sources
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut block = format!("[{}] {}\nURL: {}", i + 1, s.title.trim(), s.url);
            match &s.published {
                Some(p) => block.push_str(&format!("\nPublished: {}", describe_published(p, now))),
                None => block.push_str("\nPublished: unknown"),
            }
            if !s.body.trim().is_empty() {
                block.push('\n');
                block.push_str(s.body.trim());
            }
            block
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    if body.is_empty() {
        format!("{header}\n\nNo sources found for this period.")
    } else {
        format!("{header}\n\n{body}")
    }
}

/// Accept a URL string, a list of URLs, or search hits / pages with a `url` field.
pub(super) fn urls_from_value(value: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim();
        if !s.is_empty() && !urls.iter().any(|u| u == s) {
            urls.push(s.to_string());
        }
    };
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            // A template like `{{nodes.search.output.results}}` inside text renders as JSON.
            if trimmed.starts_with('[') || trimmed.starts_with('{') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    return urls_from_value(&parsed);
                }
            }
            let links: Vec<&str> = trimmed
                .split(char::is_whitespace)
                .filter(|t| t.starts_with("http://") || t.starts_with("https://"))
                .collect();
            if links.is_empty() {
                push(trimmed);
            }
            for link in links {
                push(link);
            }
        }
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(s) => push(s),
                    Value::Object(_) => {
                        if let Some(u) = item.get("url").and_then(Value::as_str) {
                            push(u);
                        }
                    }
                    _ => {}
                }
            }
        }
        Value::Object(_) => {
            if let Some(u) = value.get("url").and_then(Value::as_str) {
                push(u);
            }
        }
        _ => {}
    }
    urls
}

/// `fields` may be an object `{name: selector}` or text lines `name: selector`.
fn parse_fields(value: Option<&Value>) -> Option<Map<String, Value>> {
    let map = match value? {
        Value::Object(map) => map.clone(),
        Value::String(text) => text
            .lines()
            .filter_map(|line| {
                let (name, sel) = line.split_once(':').or_else(|| line.split_once('='))?;
                let (name, sel) = (name.trim(), sel.trim());
                (!name.is_empty() && !sel.is_empty()).then(|| (name.to_string(), json!(sel)))
            })
            .collect(),
        _ => return None,
    };
    (!map.is_empty()).then_some(map)
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

pub(super) fn config_u64(node: &Node, key: &str, default: u64) -> u64 {
    match node.config.get(key) {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(default),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

fn resolve_url(node: &Node, ctx: &ExecutionContext) -> Result<String, String> {
    if let Some(raw) = node.config.get("url").cloned() {
        return match ctx.resolve_value(raw)? {
            Value::String(s) => Ok(s),
            Value::Null => Ok(String::new()),
            other => Ok(other.to_string().trim_matches('"').to_string()),
        };
    }
    Ok(String::new())
}

fn resolve_query(node: &Node, ctx: &ExecutionContext) -> Result<String, String> {
    if let Some(raw) = node.config.get("query").cloned() {
        return match ctx.resolve_value(raw)? {
            Value::String(s) => Ok(s),
            Value::Null => Ok(String::new()),
            other => Ok(other.to_string().trim_matches('"').to_string()),
        };
    }
    match ctx.resolve_template_result("{{trigger.text}}") {
        Ok(Value::String(s)) => Ok(s),
        Ok(other) => Ok(other.to_string().trim_matches('"').to_string()),
        Err(_) => Ok(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_from_many_shapes() {
        assert_eq!(
            urls_from_value(&json!("https://a.com")),
            vec!["https://a.com"]
        );
        assert_eq!(
            urls_from_value(&json!([{ "url": "https://a.com" }, "https://b.com", "https://a.com"])),
            vec!["https://a.com", "https://b.com"]
        );
        assert_eq!(
            urls_from_value(&json!("https://a.com\nhttps://b.com")),
            vec!["https://a.com", "https://b.com"]
        );
        assert_eq!(
            urls_from_value(&json!(r#"[{"url":"https://c.com","title":"x"}]"#)),
            vec!["https://c.com"]
        );
    }

    #[test]
    fn fields_from_text_lines() {
        let map = parse_fields(Some(&json!("price: .price\nimages = img@src[]\n\nbad"))).unwrap();
        assert_eq!(map["price"], ".price");
        assert_eq!(map["images"], "img@src[]");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn context_has_today_period_and_dates() {
        let now = DateTime::parse_from_rfc3339("2026-10-05T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let ctx = build_context(
            &[
                ContextSource {
                    title: "A".into(),
                    url: "https://a".into(),
                    published: Some("Sun, 04 Oct 2026 09:00:00 GMT".into()),
                    body: "text a".into(),
                },
                ContextSource {
                    title: "B".into(),
                    url: "https://b".into(),
                    published: None,
                    body: String::new(),
                },
            ],
            Some(Freshness::Week),
            now,
        );
        let local = now.with_timezone(&Local).format("%Y-%m-%d").to_string();
        assert!(ctx.starts_with(&format!("Today: {local} (")), "{ctx}");
        assert!(ctx.contains("\nPeriod: last 7 days (since 2026-09-28)\n\n[1] A"), "{ctx}");
        assert!(
            ctx.contains("URL: https://a\nPublished: 2026-10-04 09:00 UTC (1 day ago)\ntext a"),
            "{ctx}"
        );
        assert!(ctx.ends_with("[2] B\nURL: https://b\nPublished: unknown"), "{ctx}");

        let empty = build_context(&[], None, now);
        assert!(empty.ends_with("Period: any time\n\nNo sources found for this period."));
    }

    #[test]
    fn auto_freshness_by_query() {
        assert_eq!(auto_freshness("новости крипты сегодня"), Some(Freshness::Day));
        assert_eq!(
            auto_freshness("последние важные новости о криптовалюте"),
            Some(Freshness::Week)
        );
        assert_eq!(auto_freshness("what is sqlite"), None);
    }
}
