//! Typed web / internet-research gateway.
//!
//! Agents call this API; they never get a raw HTTP client.
//! Search is live (DuckDuckGo + Google News RSS). `fetch` downloads a page under
//! [`WebPolicy`]; `open` / `extract` turn it into readable text via [`web_reader`].

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use boarddo_shared::v2::{
    WebError, WebExtractRequest, WebExtractResult, WebFetchRequest, WebFetchResult, WebOpenRequest,
    WebOpenResult, WebPolicy, WebSearchHit, WebSearchRequest, WebSearchResult,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::web_reader::{self, ReadOptions};

/// Browser-like UA: many sites serve stripped or blocked pages to unknown bots.
const PAGE_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/126.0 Safari/537.36 BoardDo/0.1";
const MAX_REDIRECTS: usize = 5;

/// Pluggable search backend used by [`HttpWebGateway`].
#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        freshness: Option<Freshness>,
    ) -> Result<Vec<WebSearchHit>, WebError>;
}

/// Time window for search results ("latest news" must not return last year's items).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Freshness {
    Day,
    Week,
    Month,
    Year,
}

impl Freshness {
    /// Accepts `day|week|month|year` and `1d|7d|30d|365d`-style windows.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "day" | "d" | "1d" | "24h" | "pd" => Some(Self::Day),
            "week" | "w" | "7d" | "pw" => Some(Self::Week),
            "month" | "m" | "30d" | "31d" | "pm" => Some(Self::Month),
            "year" | "y" | "365d" | "1y" | "py" => Some(Self::Year),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    pub fn days(self) -> i64 {
        match self {
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 31,
            Self::Year => 366,
        }
    }

    pub fn wider(self) -> Option<Self> {
        match self {
            Self::Day => Some(Self::Week),
            Self::Week => Some(Self::Month),
            Self::Month => Some(Self::Year),
            Self::Year => None,
        }
    }

    /// Oldest publication time still inside the window.
    pub fn cutoff(self, now: DateTime<Utc>) -> DateTime<Utc> {
        now - chrono::Duration::days(self.days())
    }

    fn brave(self) -> &'static str {
        match self {
            Self::Day => "pd",
            Self::Week => "pw",
            Self::Month => "pm",
            Self::Year => "py",
        }
    }

    fn google_news_when(self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "7d",
            Self::Month => "30d",
            Self::Year => "1y",
        }
    }
}

/// Parse provider / page dates: RFC 3339, RFC 2822 (RSS), ISO without zone,
/// plain dates and relative "3 hours ago".
pub fn parse_published(raw: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.and_utc());
        }
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s.get(..10).unwrap_or(s), "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc());
    }
    let lower = s.to_ascii_lowercase();
    let rest = lower.strip_suffix(" ago")?;
    let (n, unit) = rest.split_once(' ')?;
    let n: i64 = n.trim().parse().ok()?;
    let unit = unit.trim().trim_end_matches('s');
    let delta = match unit {
        "second" => chrono::Duration::seconds(n),
        "minute" => chrono::Duration::minutes(n),
        "hour" => chrono::Duration::hours(n),
        "day" => chrono::Duration::days(n),
        "week" => chrono::Duration::weeks(n),
        "month" => chrono::Duration::days(n * 30),
        "year" => chrono::Duration::days(n * 365),
        _ => return None,
    };
    Some(now - delta)
}

/// Normalise dates to RFC 3339, drop hits older than the window and put the
/// newest first. Undated hits are kept (after dated ones): providers already
/// applied the window server-side.
pub fn apply_freshness(
    hits: Vec<WebSearchHit>,
    freshness: Option<Freshness>,
    now: DateTime<Utc>,
) -> Vec<WebSearchHit> {
    let mut dated: Vec<(Option<DateTime<Utc>>, WebSearchHit)> = hits
        .into_iter()
        .map(|mut hit| {
            let parsed = hit.published.as_deref().and_then(|p| parse_published(p, now));
            if let Some(dt) = parsed {
                hit.published = Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            }
            (parsed, hit)
        })
        .collect();
    let Some(freshness) = freshness else {
        return dated.into_iter().map(|(_, h)| h).collect();
    };
    let cutoff = freshness.cutoff(now);
    dated.retain(|(dt, _)| dt.is_none_or(|dt| dt >= cutoff));
    // Stable sort: newest first, undated keep provider order at the end.
    dated.sort_by(|(a, _), (b, _)| b.cmp(a));
    dated.into_iter().map(|(_, h)| h).collect()
}

/// Runtime-owned internet capability. Implementations enforce [`WebPolicy`]
/// before talking to a search provider, HTTP client, or browser.
#[async_trait]
pub trait WebGateway: Send + Sync {
    async fn search(
        &self,
        request: WebSearchRequest,
        policy: &WebPolicy,
    ) -> Result<WebSearchResult, WebError>;

    async fn open(
        &self,
        request: WebOpenRequest,
        policy: &WebPolicy,
    ) -> Result<WebOpenResult, WebError>;

    async fn fetch(
        &self,
        request: WebFetchRequest,
        policy: &WebPolicy,
    ) -> Result<WebFetchResult, WebError>;

    async fn extract(
        &self,
        request: WebExtractRequest,
        policy: &WebPolicy,
    ) -> Result<WebExtractResult, WebError>;
}

/// Placeholder until a real HTTP / search backend is wired.
pub struct NullWebGateway;

#[async_trait]
impl WebGateway for NullWebGateway {
    async fn search(
        &self,
        _request: WebSearchRequest,
        _policy: &WebPolicy,
    ) -> Result<WebSearchResult, WebError> {
        Err(WebError::NotConfigured)
    }

    async fn open(
        &self,
        _request: WebOpenRequest,
        _policy: &WebPolicy,
    ) -> Result<WebOpenResult, WebError> {
        Err(WebError::NotConfigured)
    }

    async fn fetch(
        &self,
        _request: WebFetchRequest,
        _policy: &WebPolicy,
    ) -> Result<WebFetchResult, WebError> {
        Err(WebError::NotConfigured)
    }

    async fn extract(
        &self,
        _request: WebExtractRequest,
        _policy: &WebPolicy,
    ) -> Result<WebExtractResult, WebError> {
        Err(WebError::NotConfigured)
    }
}

/// Search + fetch capable gateway; `open` / `extract` read pages into clean text.
pub struct HttpWebGateway {
    search: Arc<dyn SearchProvider>,
    client: reqwest::Client,
}

impl HttpWebGateway {
    pub fn duckduckgo() -> Self {
        Self::with_search(Arc::new(DuckDuckGoSearchProvider::default()))
    }

    /// Search provider chain from the environment:
    /// `BRAVE_SEARCH_API_KEY` → Brave Search API, `SEARXNG_URL` → SearXNG instance,
    /// then the keyless DuckDuckGo / Google News fallback.
    pub fn from_env() -> Self {
        let env = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let mut providers: Vec<Arc<dyn SearchProvider>> = Vec::new();
        if let Some(key) = env("BRAVE_SEARCH_API_KEY") {
            providers.push(Arc::new(BraveSearchProvider::new(key)));
        }
        if let Some(url) = env("SEARXNG_URL") {
            providers.push(Arc::new(SearxngSearchProvider::new(url)));
        }
        providers.push(Arc::new(DuckDuckGoSearchProvider::default()));
        Self::with_search(Arc::new(ChainSearchProvider { providers }))
    }

    pub fn with_search(search: Arc<dyn SearchProvider>) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,text/plain;q=0.8,*/*;q=0.5",
            ),
        );
        headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            reqwest::header::HeaderValue::from_static("ru,en;q=0.8"),
        );
        // Redirects are followed by hand so every hop passes `WebPolicy::check_url`.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(PAGE_USER_AGENT)
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client");
        Self { search, client }
    }
}

#[async_trait]
impl WebGateway for HttpWebGateway {
    async fn search(
        &self,
        request: WebSearchRequest,
        policy: &WebPolicy,
    ) -> Result<WebSearchResult, WebError> {
        policy.check_search()?;
        policy.check_budget(0)?;
        let query = request.query.trim();
        if query.is_empty() {
            return Err(WebError::Upstream("search query is empty".into()));
        }
        let limit = request.limit.unwrap_or(8).clamp(1, 20);
        let freshness = request.freshness.as_deref().and_then(Freshness::parse);
        let results = self.search.search(query, limit, freshness).await?;
        Ok(WebSearchResult {
            results: apply_freshness(results, freshness, Utc::now()),
        })
    }

    async fn open(
        &self,
        request: WebOpenRequest,
        policy: &WebPolicy,
    ) -> Result<WebOpenResult, WebError> {
        policy.check_open()?;
        let fetched = self
            .fetch(
                WebFetchRequest {
                    url: request.url.clone(),
                    method: "GET".into(),
                    headers: Default::default(),
                },
                policy,
            )
            .await?;
        let page = read_fetched(&fetched, &ReadOptions::default())?;
        Ok(WebOpenResult {
            url: fetched.url,
            title: page.title,
            content: page.text,
            links: page.links.into_iter().map(|l| l.url).collect(),
        })
    }

    async fn fetch(
        &self,
        request: WebFetchRequest,
        policy: &WebPolicy,
    ) -> Result<WebFetchResult, WebError> {
        policy.check_fetch()?;
        policy.check_budget(0)?;
        let method = if request.method.trim().is_empty() {
            "GET".to_string()
        } else {
            request.method.to_ascii_uppercase()
        };
        policy.check_method(&method)?;
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| WebError::MethodDenied(method.clone()))?;

        let mut url = request.url.trim().to_string();
        let mut response = None;
        for _ in 0..=MAX_REDIRECTS {
            policy.check_url(&url)?;
            let mut builder = self
                .client
                .request(method.clone(), &url)
                .timeout(policy.timeout());
            for (k, v) in &request.headers {
                builder = builder.header(k, v);
            }
            let resp = builder
                .send()
                .await
                .map_err(upstream_error)?;
            let location = resp
                .status()
                .is_redirection()
                .then(|| resp.headers().get(reqwest::header::LOCATION))
                .flatten()
                .and_then(|v| v.to_str().ok());
            match location {
                Some(location) => {
                    url = resp
                        .url()
                        .join(location)
                        .map_err(|e| WebError::InvalidUrl(e.to_string()))?
                        .to_string();
                }
                None => {
                    response = Some(resp);
                    break;
                }
            }
        }
        let Some(mut response) = response else {
            return Err(WebError::Upstream(format!(
                "too many redirects (>{MAX_REDIRECTS})"
            )));
        };

        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_retry_after(v, Utc::now()));
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        // Stream the body so oversized pages are cut off without downloading them fully.
        let max = policy.max_page_size as usize;
        let mut bytes = Vec::new();
        let mut truncated = false;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(upstream_error)?
        {
            let room = max.saturating_sub(bytes.len());
            if chunk.len() > room {
                bytes.extend_from_slice(&chunk[..room]);
                truncated = true;
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = if web_reader::is_textual(content_type.as_deref()) {
            web_reader::decode_body(&bytes, content_type.as_deref())
        } else {
            String::new()
        };
        Ok(WebFetchResult {
            url,
            status,
            content_type,
            body,
            truncated,
            retry_after,
        })
    }

    async fn extract(
        &self,
        request: WebExtractRequest,
        policy: &WebPolicy,
    ) -> Result<WebExtractResult, WebError> {
        policy.check_enabled()?;
        let opts = ReadOptions {
            max_chars: request.max_chars.unwrap_or(8_000) as usize,
            selector: request.selector.clone(),
            include_links: true,
        };
        let page = match (&request.content, &request.url) {
            (Some(content), url) if !content.is_empty() => {
                if web_reader::is_html(None, content) {
                    web_reader::read_html(content, url.as_deref().unwrap_or(""), &opts)
                        .map_err(WebError::Upstream)?
                } else {
                    plain_page(content, opts.max_chars)
                }
            }
            (_, Some(url)) => {
                let fetched = self
                    .fetch(
                        WebFetchRequest {
                            url: url.clone(),
                            method: "GET".into(),
                            headers: Default::default(),
                        },
                        policy,
                    )
                    .await?;
                read_fetched(&fetched, &opts)?
            }
            _ => {
                return Err(WebError::Upstream(
                    "extract needs `url` or `content`".into(),
                ));
            }
        };
        Ok(WebExtractResult {
            document_id: request.document_id,
            chunks: web_reader::chunk_text(&page.text, 2_000),
            text: page.text,
            links: page.links.into_iter().map(|l| l.url).collect(),
        })
    }
}

/// `Retry-After`: delay in seconds or an HTTP date.
pub fn parse_retry_after(value: &str, now: DateTime<Utc>) -> Option<u64> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(secs);
    }
    let at = DateTime::parse_from_rfc2822(value).ok()?.with_timezone(&Utc);
    Some(at.signed_duration_since(now).num_seconds().max(0) as u64)
}

/// Upstream error with its cause chain ("error sending request" alone hides TLS / DNS issues).
fn upstream_error(e: reqwest::Error) -> WebError {
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        msg.push_str(": ");
        msg.push_str(&cause.to_string());
        source = cause.source();
    }
    WebError::Upstream(msg)
}

/// Turn a fetched response into readable content (HTML is parsed, text passes through).
pub fn read_fetched(
    fetched: &WebFetchResult,
    opts: &ReadOptions,
) -> Result<web_reader::PageContent, WebError> {
    if !(200..300).contains(&fetched.status) {
        return Err(WebError::Upstream(format!(
            "HTTP {} for {}",
            fetched.status, fetched.url
        )));
    }
    if !web_reader::is_textual(fetched.content_type.as_deref()) {
        return Err(WebError::Upstream(format!(
            "unsupported content type {}",
            fetched.content_type.as_deref().unwrap_or("?")
        )));
    }
    if web_reader::is_html(fetched.content_type.as_deref(), &fetched.body) {
        web_reader::read_html(&fetched.body, &fetched.url, opts).map_err(WebError::Upstream)
    } else {
        Ok(plain_page(&fetched.body, opts.max_chars))
    }
}

fn plain_page(text: &str, max_chars: usize) -> web_reader::PageContent {
    let (text, truncated) = web_reader::clip_chars(text.trim(), max_chars);
    web_reader::PageContent {
        text,
        truncated,
        ..Default::default()
    }
}

/// Tries providers in order and tops up results until `limit` is reached.
/// A failing provider is skipped so a bad key never takes search down.
pub struct ChainSearchProvider {
    pub providers: Vec<Arc<dyn SearchProvider>>,
}

#[async_trait]
impl SearchProvider for ChainSearchProvider {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        freshness: Option<Freshness>,
    ) -> Result<Vec<WebSearchHit>, WebError> {
        let mut hits = Vec::new();
        let mut last_err = None;
        for provider in &self.providers {
            if hits.len() >= limit as usize {
                break;
            }
            // Filter per provider so stale hits don't stop the chain from topping up.
            match provider.search(query, limit, freshness).await {
                Ok(found) => merge_hits(&mut hits, apply_freshness(found, freshness, Utc::now())),
                Err(e) => {
                    tracing::warn!("search provider failed: {e}");
                    last_err = Some(e);
                }
            }
        }
        if hits.is_empty() {
            if let Some(e) = last_err {
                return Err(e);
            }
        }
        hits.truncate(limit as usize);
        Ok(hits)
    }
}

/// Brave Search API (https://brave.com/search/api/) — direct publisher URLs,
/// a dedicated news index for news-like queries.
pub struct BraveSearchProvider {
    api_key: String,
    client: reqwest::Client,
}

impl BraveSearchProvider {
    pub fn new(api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self { api_key, client }
    }

    async fn get(&self, url: &str) -> Result<Value, WebError> {
        let response = self
            .client
            .get(url)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", &self.api_key)
            .send()
            .await
            .map_err(upstream_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(WebError::Upstream(format!("brave search: HTTP {status}")));
        }
        response
            .json()
            .await
            .map_err(|e| WebError::Upstream(format!("brave search: {e}")))
    }
}

#[async_trait]
impl SearchProvider for BraveSearchProvider {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        freshness: Option<Freshness>,
    ) -> Result<Vec<WebSearchHit>, WebError> {
        let q = urlencoding_lite(query);
        let count = limit.min(20);
        let window = freshness
            .map(|f| format!("&freshness={}", f.brave()))
            .unwrap_or_default();
        let mut hits = Vec::new();
        if looks_like_news_query(query) {
            let news = self
                .get(&format!(
                    "https://api.search.brave.com/res/v1/news/search?q={q}&count={count}{window}"
                ))
                .await?;
            hits.extend(parse_brave_results(news.get("results"), "brave-news"));
        }
        if hits.len() < limit as usize {
            let web = self
                .get(&format!(
                    "https://api.search.brave.com/res/v1/web/search?q={q}&count={count}{window}"
                ))
                .await?;
            let found = parse_brave_results(web.pointer("/web/results"), "brave");
            merge_hits(&mut hits, found);
        }
        hits.truncate(limit as usize);
        Ok(hits)
    }
}

pub fn parse_brave_results(results: Option<&Value>, source: &str) -> Vec<WebSearchHit> {
    let Some(Value::Array(items)) = results else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let url = item.get("url").and_then(Value::as_str)?.to_string();
            let title = item.get("title").and_then(Value::as_str).unwrap_or(&url);
            let snippet = item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let published = item
                .get("page_age")
                .or_else(|| item.get("age"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Some(WebSearchHit {
                title: strip_tags(title),
                url,
                snippet: strip_tags(snippet),
                source: Some(source.into()),
                published,
            })
        })
        .collect()
}

/// Self-hosted SearXNG metasearch (needs `json` in `search.formats` of settings.yml).
pub struct SearxngSearchProvider {
    base_url: String,
    client: reqwest::Client,
}

impl SearxngSearchProvider {
    pub fn new(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        }
    }
}

#[async_trait]
impl SearchProvider for SearxngSearchProvider {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        freshness: Option<Freshness>,
    ) -> Result<Vec<WebSearchHit>, WebError> {
        let category = if looks_like_news_query(query) {
            "news"
        } else {
            "general"
        };
        let time_range = freshness
            .map(|f| format!("&time_range={}", f.as_str()))
            .unwrap_or_default();
        let url = format!(
            "{}/search?q={}&format=json&categories={category}{time_range}",
            self.base_url,
            urlencoding_lite(query)
        );
        let response = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(upstream_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(WebError::Upstream(format!("searxng: HTTP {status}")));
        }
        let parsed: Value = response
            .json()
            .await
            .map_err(|e| WebError::Upstream(format!("searxng: {e}")))?;
        Ok(parse_searxng_results(&parsed, limit))
    }
}

pub fn parse_searxng_results(parsed: &Value, limit: u32) -> Vec<WebSearchHit> {
    let Some(Value::Array(items)) = parsed.get("results") else {
        return Vec::new();
    };
    let mut hits: Vec<WebSearchHit> = items
        .iter()
        .filter_map(|item| {
            let url = item.get("url").and_then(Value::as_str)?.to_string();
            let title = item.get("title").and_then(Value::as_str).unwrap_or(&url);
            Some(WebSearchHit {
                title: title.to_string(),
                url,
                snippet: item
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                source: Some("searxng".into()),
                published: item
                    .get("publishedDate")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect();
    hits.truncate(limit as usize);
    hits
}

/// DuckDuckGo Instant Answer API — no API key.
pub struct DuckDuckGoSearchProvider {
    client: reqwest::Client,
}

impl Default for DuckDuckGoSearchProvider {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("BoardDo/0.1 (+https://boarddo.local)")
            .build()
            .expect("reqwest client");
        Self { client }
    }
}

#[async_trait]
impl SearchProvider for DuckDuckGoSearchProvider {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        freshness: Option<Freshness>,
    ) -> Result<Vec<WebSearchHit>, WebError> {
        let mut hits = Vec::new();
        // Instant Answers are encyclopedic and undated: useless for a time window.
        if freshness.is_none() {
            if let Ok(body) = self
                .fetch_text(&format!(
                    "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
                    urlencoding_lite(query)
                ))
                .await
            {
                if let Ok(parsed) = serde_json::from_str::<Value>(&body) {
                    hits.extend(parse_duckduckgo_results(&parsed, limit));
                }
            }
        }

        if (hits.len() as u32) < limit {
            if let Ok(html) = self
                .fetch_text(&format!(
                    "https://html.duckduckgo.com/html/?q={}{}",
                    urlencoding_lite(query),
                    freshness
                        .map(|f| format!("&df={}", &f.as_str()[..1]))
                        .unwrap_or_default()
                ))
                .await
            {
                merge_hits(&mut hits, parse_ddg_html(&html, limit));
            }
        }

        if (hits.len() as u32) < limit || looks_like_news_query(query) || freshness.is_some() {
            let (hl, gl, ceid) = if query
                .chars()
                .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
            {
                ("ru", "RU", "RU:ru")
            } else {
                ("en-US", "US", "US:en")
            };
            if let Ok(rss) = self
                .fetch_text(&format!(
                    "https://news.google.com/rss/search?q={}&hl={hl}&gl={gl}&ceid={ceid}",
                    urlencoding_lite(&match freshness {
                        // Google News search operator: only items from the window.
                        Some(f) => format!("{query} when:{}", f.google_news_when()),
                        None => query.to_string(),
                    })
                ))
                .await
            {
                merge_hits(&mut hits, parse_rss_items(&rss, limit, "google-news"));
            }
        }

        hits.truncate(limit as usize);
        Ok(hits)
    }
}

impl DuckDuckGoSearchProvider {
    async fn fetch_text(&self, url: &str) -> Result<String, WebError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| WebError::Upstream(e.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| WebError::Upstream(e.to_string()))?;
        if !status.is_success() {
            return Err(WebError::Upstream(format!("HTTP {status}")));
        }
        Ok(body)
    }
}

/// Deterministic hits for tests — no network.
pub struct StaticSearchProvider {
    pub hits: Vec<WebSearchHit>,
}

#[async_trait]
impl SearchProvider for StaticSearchProvider {
    async fn search(
        &self,
        _query: &str,
        limit: u32,
        _freshness: Option<Freshness>,
    ) -> Result<Vec<WebSearchHit>, WebError> {
        Ok(self.hits.iter().take(limit as usize).cloned().collect())
    }
}

pub fn parse_duckduckgo_results(parsed: &Value, limit: u32) -> Vec<WebSearchHit> {
    let mut hits = Vec::new();
    let heading = parsed
        .get("Heading")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let abstract_text = parsed
        .get("AbstractText")
        .or_else(|| parsed.get("Abstract"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let abstract_url = parsed
        .get("AbstractURL")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !abstract_url.is_empty() {
        hits.push(WebSearchHit {
            title: if heading.is_empty() {
                abstract_url.to_string()
            } else {
                heading
            },
            url: abstract_url.to_string(),
            snippet: abstract_text.to_string(),
            source: Some("duckduckgo".into()),
            published: None,
        });
    }
    collect_ddg_topics(parsed.get("Results"), &mut hits);
    collect_ddg_topics(parsed.get("RelatedTopics"), &mut hits);

    let mut seen = std::collections::HashSet::new();
    hits.retain(|h| seen.insert(h.url.clone()));
    hits.truncate(limit as usize);
    hits
}

pub fn looks_like_news_query(query: &str) -> bool {
    let q = query.to_lowercase();
    [
        "news",
        "новост",
        "сегодня",
        "today",
        "latest",
        "свеж",
        "последн",
        "актуальн",
        "recent",
        "headline",
        "заголов",
    ]
    .iter()
    .any(|k| q.contains(k))
}

pub fn parse_ddg_html(html: &str, limit: u32) -> Vec<WebSearchHit> {
    let mut hits = Vec::new();
    let mut rest = html;
    while hits.len() < limit as usize {
        let Some(idx) = rest.find("uddg=") else {
            break;
        };
        let after = &rest[idx + 5..];
        let end = after
            .find(|c: char| c == '&' || c == '"' || c == '\'' || c.is_whitespace())
            .unwrap_or(after.len());
        let encoded = &after[..end];
        let url = html_unescape(&percent_decode(encoded));
        if url.starts_with("http://") || url.starts_with("https://") {
            let title = nearby_title(rest, idx).unwrap_or_else(|| url.clone());
            hits.push(WebSearchHit {
                title,
                url,
                snippet: String::new(),
                source: Some("duckduckgo".into()),
                published: None,
            });
        }
        rest = &after[end..];
    }
    if hits.is_empty() {
        let mut rest = html;
        while hits.len() < limit as usize {
            let Some(marker) = rest.find("result__a") else {
                break;
            };
            let slice = &rest[marker..];
            let Some(href_at) = slice.find("href=\"") else {
                rest = &slice[1..];
                continue;
            };
            let href_src = &slice[href_at + 6..];
            let end = href_src.find('"').unwrap_or(0);
            let href = html_unescape(&href_src[..end]);
            let after_tag = slice.find('>').map(|i| i + 1).unwrap_or(0);
            let rest_title = &slice[after_tag..];
            let title_end = rest_title.find('<').unwrap_or(rest_title.len().min(180));
            let title = html_unescape(rest_title[..title_end].trim());
            if href.starts_with("http") {
                hits.push(WebSearchHit {
                    title: if title.is_empty() {
                        href.clone()
                    } else {
                        title
                    },
                    url: href,
                    snippet: String::new(),
                    source: Some("duckduckgo".into()),
                    published: None,
                });
            }
            rest = &rest[marker + 1..];
        }
    }
    let mut seen = std::collections::HashSet::new();
    hits.retain(|h| seen.insert(h.url.clone()));
    hits.truncate(limit as usize);
    hits
}

fn nearby_title(html: &str, uddg_idx: usize) -> Option<String> {
    let before = &html[..uddg_idx];
    let start = before.rfind('>').map(|i| i + 1)?;
    let chunk = html.get(start..uddg_idx)?;
    let close = chunk.find('<')?;
    let title = html_unescape(chunk[..close].trim());
    if title.len() >= 3 { Some(title) } else { None }
}

pub fn parse_rss_items(xml: &str, limit: u32, source: &str) -> Vec<WebSearchHit> {
    let mut hits = Vec::new();
    let mut rest = xml;
    while hits.len() < limit as usize {
        let Some(item_start) = rest.find("<item") else {
            break;
        };
        let after_item = &rest[item_start..];
        let Some(item_end) = after_item.find("</item>") else {
            break;
        };
        let item = &after_item[..item_end];
        let title = xml_tag(item, "title").unwrap_or_default();
        let link = xml_tag(item, "link").unwrap_or_default();
        let snippet = xml_tag(item, "description").unwrap_or_default();
        if !link.is_empty() {
            hits.push(WebSearchHit {
                title: if title.is_empty() {
                    link.clone()
                } else {
                    title
                },
                url: link,
                snippet: strip_tags(&snippet),
                source: Some(source.into()),
                published: xml_tag(item, "pubDate"),
            });
        }
        rest = &after_item[item_end + 7..];
    }
    hits
}

fn xml_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    let raw = block[start..end].trim();
    let raw = raw
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(raw);
    Some(html_unescape(raw))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    html_unescape(out.trim())
}

fn merge_hits(into: &mut Vec<WebSearchHit>, extra: Vec<WebSearchHit>) {
    let mut seen: std::collections::HashSet<String> = into.iter().map(|h| h.url.clone()).collect();
    for hit in extra {
        if seen.insert(hit.url.clone()) {
            into.push(hit);
        }
    }
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn percent_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let raw = s.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                bytes.push(b);
                i += 3;
                continue;
            }
        }
        if raw[i] == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(raw[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn collect_ddg_topics(value: Option<&Value>, hits: &mut Vec<WebSearchHit>) {
    let Some(Value::Array(items)) = value else {
        return;
    };
    for item in items {
        if let Some(nested) = item.get("Topics") {
            collect_ddg_topics(Some(nested), hits);
            continue;
        }
        let url = item
            .get("FirstURL")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if url.is_empty() {
            continue;
        }
        let text = item.get("Text").and_then(Value::as_str).unwrap_or("");
        let title = text.split(" - ").next().unwrap_or(text).trim();
        hits.push(WebSearchHit {
            title: if title.is_empty() {
                url.clone()
            } else {
                title.to_string()
            },
            url,
            snippet: text.to_string(),
            source: Some("duckduckgo".into()),
            published: None,
        });
    }
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn null_gateway_is_not_configured() {
        let gw = NullWebGateway;
        let policy = WebPolicy::default();
        assert_eq!(
            gw.search(
                WebSearchRequest {
                    query: "rust".into(),
                    limit: None,
                    freshness: None,
                },
                &policy,
            )
            .await
            .unwrap_err(),
            WebError::NotConfigured
        );
    }

    #[test]
    fn parse_ddg_related_topics() {
        let parsed = serde_json::json!({
            "Heading": "Rust",
            "AbstractText": "A language.",
            "AbstractURL": "https://www.rust-lang.org/",
            "RelatedTopics": [
                { "FirstURL": "https://doc.rust-lang.org/", "Text": "The Rust Book - docs" },
                {
                    "Name": "See also",
                    "Topics": [
                        { "FirstURL": "https://crates.io/", "Text": "crates.io - packages" }
                    ]
                }
            ]
        });
        let hits = parse_duckduckgo_results(&parsed, 10);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].url, "https://www.rust-lang.org/");
        assert_eq!(hits[1].title, "The Rust Book");
        assert_eq!(hits[2].url, "https://crates.io/");
    }

    #[tokio::test]
    async fn http_gateway_search_uses_provider() {
        let gw = HttpWebGateway::with_search(Arc::new(StaticSearchProvider {
            hits: vec![WebSearchHit {
                title: "Docs".into(),
                url: "https://docs.rs".into(),
                snippet: "crates".into(),
                source: Some("test".into()),
                published: None,
            }],
        }));
        let result = gw
            .search(
                WebSearchRequest {
                    query: "serde".into(),
                    limit: Some(5),
                    freshness: None,
                },
                &WebPolicy::default(),
            )
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].url, "https://docs.rs");
    }

    #[tokio::test]
    async fn http_gateway_search_respects_policy() {
        let mut policy = WebPolicy::default();
        policy.search_enabled = false;
        let gw = HttpWebGateway::duckduckgo();
        assert_eq!(
            gw.search(
                WebSearchRequest {
                    query: "x".into(),
                    limit: None,
                    freshness: None,
                },
                &policy,
            )
            .await
            .unwrap_err(),
            WebError::SearchDisabled
        );
    }

    #[test]
    fn parse_google_news_rss() {
        let xml = r#"<?xml version="1.0"?><rss><channel>
            <item>
              <title>AI chip news</title>
              <link>https://example.com/ai</link>
              <description><![CDATA[<p>Today in AI</p>]]></description>
            </item>
            <item>
              <title>Other</title>
              <link>https://example.com/other</link>
              <description>Hello</description>
            </item>
        </channel></rss>"#;
        let hits = parse_rss_items(xml, 10, "google-news");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "AI chip news");
        assert_eq!(hits[0].url, "https://example.com/ai");
        assert_eq!(hits[0].snippet, "Today in AI");
    }

    #[test]
    fn parse_ddg_html_uddg_links() {
        let html = r#"<a href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fopenai.com%2Fnews&amp;rut=1">OpenAI News</a>"#;
        let hits = parse_ddg_html(html, 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://openai.com/news");
    }

    #[test]
    fn parse_brave_news_and_web() {
        let news = serde_json::json!({ "results": [
            { "title": "<strong>Bitcoin</strong> ETF", "url": "https://coindesk.com/a",
              "description": "Flows <strong>up</strong>", "age": "2 hours ago" },
            { "title": "no url" }
        ]});
        let hits = parse_brave_results(news.get("results"), "brave-news");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Bitcoin ETF");
        assert_eq!(hits[0].snippet, "Flows up");
        assert_eq!(hits[0].published.as_deref(), Some("2 hours ago"));

        let web = serde_json::json!({ "web": { "results": [
            { "title": "Docs", "url": "https://docs.rs/", "description": "", "page_age": "2026-10-01T00:00:00" }
        ]}});
        let hits = parse_brave_results(web.pointer("/web/results"), "brave");
        assert_eq!(hits[0].published.as_deref(), Some("2026-10-01T00:00:00"));
    }

    #[test]
    fn parse_searxng() {
        let parsed = serde_json::json!({ "results": [
            { "title": "ETH update", "url": "https://example.com/eth", "content": "Merge",
              "publishedDate": "2026-10-04T10:00:00" },
            { "title": "B", "url": "https://example.com/b" }
        ]});
        let hits = parse_searxng_results(&parsed, 1);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "Merge");
        assert_eq!(hits[0].published.as_deref(), Some("2026-10-04T10:00:00"));
    }

    #[tokio::test]
    async fn chain_skips_failing_provider_and_tops_up() {
        struct Failing;
        #[async_trait]
        impl SearchProvider for Failing {
            async fn search(
                &self,
                _q: &str,
                _l: u32,
                _f: Option<Freshness>,
            ) -> Result<Vec<WebSearchHit>, WebError> {
                Err(WebError::Upstream("bad key".into()))
            }
        }
        let hit = |u: &str| WebSearchHit {
            title: u.into(),
            url: u.into(),
            snippet: String::new(),
            source: None,
            published: None,
        };
        let chain = ChainSearchProvider {
            providers: vec![
                Arc::new(Failing),
                Arc::new(StaticSearchProvider { hits: vec![hit("https://a"), hit("https://b")] }),
                Arc::new(StaticSearchProvider { hits: vec![hit("https://b"), hit("https://c")] }),
            ],
        };
        let urls: Vec<String> = chain.search("x", 3, None).await.unwrap().into_iter().map(|h| h.url).collect();
        assert_eq!(urls, vec!["https://a", "https://b", "https://c"]);

        let only_failing = ChainSearchProvider { providers: vec![Arc::new(Failing)] };
        assert!(only_failing.search("x", 3, None).await.is_err());
    }

    #[test]
    fn parses_retry_after() {
        let now = DateTime::parse_from_rfc3339("2026-10-05T12:00:00Z").unwrap().with_timezone(&Utc);
        assert_eq!(parse_retry_after("120", now), Some(120));
        assert_eq!(parse_retry_after("Mon, 05 Oct 2026 12:00:30 GMT", now), Some(30));
        assert_eq!(parse_retry_after("Mon, 05 Oct 2026 11:00:00 GMT", now), Some(0));
        assert_eq!(parse_retry_after("soon", now), None);
    }

    #[test]
    fn parses_provider_dates() {
        let now = DateTime::parse_from_rfc3339("2026-10-05T12:00:00Z").unwrap().with_timezone(&Utc);
        let iso = |s: &str| parse_published(s, now).map(|d| d.to_rfc3339());
        assert_eq!(iso("Sun, 04 Oct 2026 08:30:00 GMT").unwrap(), "2026-10-04T08:30:00+00:00");
        assert_eq!(iso("2026-10-01T10:00:00").unwrap(), "2026-10-01T10:00:00+00:00");
        assert_eq!(iso("2026-09-30T10:00:00+03:00").unwrap(), "2026-09-30T07:00:00+00:00");
        assert_eq!(iso("2026-01-15").unwrap(), "2026-01-15T00:00:00+00:00");
        assert_eq!(iso("3 hours ago").unwrap(), "2026-10-05T09:00:00+00:00");
        assert_eq!(iso("2 days ago").unwrap(), "2026-10-03T12:00:00+00:00");
        assert!(iso("someday").is_none());
    }

    #[test]
    fn freshness_drops_old_and_sorts_newest_first() {
        let now = DateTime::parse_from_rfc3339("2026-10-05T12:00:00Z").unwrap().with_timezone(&Utc);
        let hit = |url: &str, published: Option<&str>| WebSearchHit {
            title: url.into(),
            url: url.into(),
            snippet: String::new(),
            source: None,
            published: published.map(str::to_string),
        };
        let hits = vec![
            hit("https://jan", Some("Thu, 15 Jan 2026 10:00:00 GMT")),
            hit("https://undated", None),
            hit("https://older", Some("2026-10-01T10:00:00")),
            hit("https://newest", Some("5 hours ago")),
        ];
        let week: Vec<String> = apply_freshness(hits.clone(), Some(Freshness::Week), now)
            .into_iter()
            .map(|h| h.url)
            .collect();
        assert_eq!(week, vec!["https://newest", "https://older", "https://undated"]);

        let any = apply_freshness(hits, None, now);
        assert_eq!(any.len(), 4);
        assert_eq!(any[0].published.as_deref(), Some("2026-01-15T10:00:00Z"));
        assert_eq!(Freshness::parse("7d"), Some(Freshness::Week));
        assert_eq!(Freshness::Week.wider(), Some(Freshness::Month));
    }

    #[test]
    fn news_query_detection() {
        assert!(looks_like_news_query("новости ИИ на сегодня"));
        assert!(looks_like_news_query("AI news today"));
        assert!(!looks_like_news_query("what is sqlite"));
    }
}




