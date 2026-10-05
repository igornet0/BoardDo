//! Typed web / internet-research gateway.
//!
//! Agents call this API; they never get a raw HTTP client.
//! Search is live (DuckDuckGo Instant Answer). open/fetch/extract stay stubbed.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use boarddo_shared::v2::{
    WebError, WebExtractRequest, WebExtractResult, WebFetchRequest, WebFetchResult, WebOpenRequest,
    WebOpenResult, WebPolicy, WebSearchHit, WebSearchRequest, WebSearchResult,
};
use serde_json::Value;

/// Pluggable search backend used by [`HttpWebGateway`].
#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, query: &str, limit: u32) -> Result<Vec<WebSearchHit>, WebError>;
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

/// Search + fetch capable gateway. `open` remains a fetch alias; extract is text cleanup.
pub struct HttpWebGateway {
    search: Arc<dyn SearchProvider>,
    client: reqwest::Client,
}

impl HttpWebGateway {
    pub fn duckduckgo() -> Self {
        Self::with_search(Arc::new(DuckDuckGoSearchProvider::default()))
    }

    pub fn with_search(search: Arc<dyn SearchProvider>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("BoardDo/0.1 (+https://boarddo.local)")
            .redirect(reqwest::redirect::Policy::limited(5))
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
        let results = self.search.search(query, limit).await?;
        Ok(WebSearchResult { results })
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
        Ok(WebOpenResult {
            url: fetched.url,
            title: first_html_title(&fetched.body).unwrap_or_default(),
            content: fetched.body,
            links: Vec::new(),
        })
    }

    async fn fetch(
        &self,
        request: WebFetchRequest,
        policy: &WebPolicy,
    ) -> Result<WebFetchResult, WebError> {
        policy.check_fetch()?;
        policy.check_budget(0)?;
        let _parsed = policy.check_url(&request.url)?;
        let method = if request.method.trim().is_empty() {
            "GET".to_string()
        } else {
            request.method.to_ascii_uppercase()
        };
        policy.check_method(&method)?;

        let mut builder = self
            .client
            .request(
                reqwest::Method::from_bytes(method.as_bytes())
                    .map_err(|_| WebError::MethodDenied(method.clone()))?,
                &request.url,
            )
            .timeout(policy.timeout());
        for (k, v) in &request.headers {
            builder = builder.header(k, v);
        }
        let response = builder
            .send()
            .await
            .map_err(|e| WebError::Upstream(e.to_string()))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let bytes = response
            .bytes()
            .await
            .map_err(|e| WebError::Upstream(e.to_string()))?;
        let truncated = bytes.len() as u64 > policy.max_page_size;
        let slice = if truncated {
            &bytes[..policy.max_page_size as usize]
        } else {
            &bytes
        };
        let body = String::from_utf8_lossy(slice).into_owned();
        Ok(WebFetchResult {
            url: request.url,
            status,
            content_type,
            body,
            truncated,
        })
    }

    async fn extract(
        &self,
        request: WebExtractRequest,
        policy: &WebPolicy,
    ) -> Result<WebExtractResult, WebError> {
        policy.check_enabled()?;
        let mut text = request.content.clone().unwrap_or_default();
        if text.is_empty() {
            if let Some(url) = &request.url {
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
                text = fetched.body;
            }
        }
        let stripped = strip_tags_simple(&text);
        let max = request.max_chars.unwrap_or(8_000) as usize;
        let clipped: String = stripped.chars().take(max).collect();
        Ok(WebExtractResult {
            document_id: request.document_id,
            text: clipped,
            chunks: Vec::new(),
            links: Vec::new(),
        })
    }
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
    async fn search(&self, query: &str, limit: u32) -> Result<Vec<WebSearchHit>, WebError> {
        let mut hits = Vec::new();
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

        if (hits.len() as u32) < limit {
            if let Ok(html) = self
                .fetch_text(&format!(
                    "https://html.duckduckgo.com/html/?q={}",
                    urlencoding_lite(query)
                ))
                .await
            {
                merge_hits(&mut hits, parse_ddg_html(&html, limit));
            }
        }

        if (hits.len() as u32) < limit || looks_like_news_query(query) {
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
                    urlencoding_lite(query)
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
    async fn search(&self, _query: &str, limit: u32) -> Result<Vec<WebSearchHit>, WebError> {
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
        });
    }
}

fn first_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let after = html.get(start..)?;
    let gt = after.find('>')?;
    let rest = after.get(gt + 1..)?;
    let end = rest.to_ascii_lowercase().find("</title>")?;
    let title = html_unescape(rest[..end].trim());
    if title.is_empty() { None } else { Some(title) }
}

fn strip_tags_simple(s: &str) -> String {
    strip_tags(s)
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
    fn news_query_detection() {
        assert!(looks_like_news_query("новости ИИ на сегодня"));
        assert!(looks_like_news_query("AI news today"));
        assert!(!looks_like_news_query("what is sqlite"));
    }
}
