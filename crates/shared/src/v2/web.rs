//! Typed Internet Research web tools.
//!
//! Agents never talk HTTP directly. They call `web.search` / `web.open` /
//! `web.fetch` / `web.extract`; the runtime chooses Search / HTTP / Browser.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Stable tool identifiers (match [`crate::type_ids`] and [`super::caps`]).
pub mod tools {
    pub const SEARCH: &str = "web.search";
    pub const OPEN: &str = "web.open";
    pub const FETCH: &str = "web.fetch";
    pub const EXTRACT: &str = "web.extract";
    pub const SCREENSHOT: &str = "web.screenshot";
}

const DEFAULT_MAX_REQUESTS: u32 = 50;
const DEFAULT_MAX_PAGE_SIZE: u64 = 5 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 15_000;

fn default_true() -> bool {
    true
}

fn default_max_requests() -> u32 {
    DEFAULT_MAX_REQUESTS
}

fn default_max_page_size() -> u64 {
    DEFAULT_MAX_PAGE_SIZE
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn default_allowed_methods() -> Vec<String> {
    vec!["GET".into(), "HEAD".into()]
}

/// Policy envelope around all internet access. Deny-by-default for local nets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPolicy {
    /// Master switch for any internet capability.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub search_enabled: bool,
    #[serde(default = "default_true")]
    pub fetch_enabled: bool,
    #[serde(default = "default_true")]
    pub open_enabled: bool,
    /// Browser / JS interaction (Servo). Off until that backend exists.
    #[serde(default)]
    pub browser_enabled: bool,
    /// If non-empty, host must match one of these (exact or subdomain).
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
    #[serde(default = "default_max_requests")]
    pub max_requests: u32,
    #[serde(default = "default_max_page_size")]
    pub max_page_size: u64,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub allow_file_upload: bool,
    #[serde(default)]
    pub allow_download: bool,
    /// When false, localhost / RFC1918 / link-local / unique-local are denied.
    #[serde(default)]
    pub allow_local_network: bool,
    #[serde(default = "default_allowed_methods")]
    pub allowed_methods: Vec<String>,
}

impl Default for WebPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            search_enabled: true,
            fetch_enabled: true,
            open_enabled: true,
            browser_enabled: false,
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            max_requests: DEFAULT_MAX_REQUESTS,
            max_page_size: DEFAULT_MAX_PAGE_SIZE,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            allow_file_upload: false,
            allow_download: false,
            allow_local_network: false,
            allowed_methods: default_allowed_methods(),
        }
    }
}

impl WebPolicy {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    pub fn check_search(&self) -> Result<(), WebError> {
        self.check_enabled()?;
        if !self.search_enabled {
            return Err(WebError::SearchDisabled);
        }
        Ok(())
    }

    pub fn check_open(&self) -> Result<(), WebError> {
        self.check_enabled()?;
        if !self.open_enabled {
            return Err(WebError::OpenDisabled);
        }
        Ok(())
    }

    pub fn check_fetch(&self) -> Result<(), WebError> {
        self.check_enabled()?;
        if !self.fetch_enabled {
            return Err(WebError::FetchDisabled);
        }
        Ok(())
    }

    pub fn check_browser(&self) -> Result<(), WebError> {
        self.check_enabled()?;
        if !self.browser_enabled {
            return Err(WebError::BrowserDisabled);
        }
        Ok(())
    }

    pub fn check_enabled(&self) -> Result<(), WebError> {
        if self.enabled {
            Ok(())
        } else {
            Err(WebError::Disabled)
        }
    }

    pub fn check_budget(&self, used: u32) -> Result<(), WebError> {
        if used >= self.max_requests {
            Err(WebError::BudgetExceeded {
                used,
                max: self.max_requests,
            })
        } else {
            Ok(())
        }
    }

    pub fn check_page_size(&self, bytes: u64) -> Result<(), WebError> {
        if bytes > self.max_page_size {
            Err(WebError::PageTooLarge {
                bytes,
                max: self.max_page_size,
            })
        } else {
            Ok(())
        }
    }

    pub fn check_method(&self, method: &str) -> Result<(), WebError> {
        let method = method.to_ascii_uppercase();
        if self
            .allowed_methods
            .iter()
            .any(|m| m.eq_ignore_ascii_case(&method))
        {
            Ok(())
        } else {
            Err(WebError::MethodDenied(method))
        }
    }

    /// True when the URL may be fetched / opened under this policy.
    pub fn is_url_allowed(&self, url: &str) -> bool {
        self.check_url(url).is_ok()
    }

    pub fn check_url(&self, url: &str) -> Result<ParsedHttpUrl, WebError> {
        self.check_enabled()?;
        let parsed = parse_http_url(url)?;
        if self.host_matches_any(&parsed.host, &self.blocked_domains) {
            return Err(WebError::UrlDenied(format!(
                "host {} is blocked",
                parsed.host
            )));
        }
        if !self.allowed_domains.is_empty()
            && !self.host_matches_any(&parsed.host, &self.allowed_domains)
        {
            return Err(WebError::UrlDenied(format!(
                "host {} is not in the allowlist",
                parsed.host
            )));
        }
        if !self.allow_local_network && is_local_or_private_host(&parsed.host) {
            return Err(WebError::UrlDenied(format!(
                "local/private host {} is not allowed",
                parsed.host
            )));
        }
        Ok(parsed)
    }

    fn host_matches_any(&self, host: &str, domains: &[String]) -> bool {
        domains.iter().any(|d| host_matches_domain(host, d))
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WebError {
    #[error("internet capability is disabled")]
    Disabled,
    #[error("web.search is disabled by policy")]
    SearchDisabled,
    #[error("web.open is disabled by policy")]
    OpenDisabled,
    #[error("web.fetch is disabled by policy")]
    FetchDisabled,
    #[error("browser / web.screenshot is disabled by policy")]
    BrowserDisabled,
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("url not allowed: {0}")]
    UrlDenied(String),
    #[error("HTTP method {0} is not allowed by policy")]
    MethodDenied(String),
    #[error("web request budget exceeded ({used}/{max})")]
    BudgetExceeded { used: u32, max: u32 },
    #[error("page exceeds max size ({bytes} > {max})")]
    PageTooLarge { bytes: u64, max: u64 },
    #[error("web gateway not configured")]
    NotConfigured,
    #[error("{0}")]
    Upstream(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHttpUrl {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
}

/// Parse `http` / `https` URLs enough to enforce host policy (no DNS).
pub fn parse_http_url(url: &str) -> Result<ParsedHttpUrl, WebError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(WebError::InvalidUrl("empty url".into()));
    }
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| WebError::InvalidUrl("missing scheme".into()))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(WebError::InvalidUrl(format!(
            "unsupported scheme `{scheme}`"
        )));
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(WebError::InvalidUrl("missing host".into()));
    }
    let hostport = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    let (host, port) = if let Some(inner) = hostport.strip_prefix('[') {
        let (host, after) = inner
            .split_once(']')
            .ok_or_else(|| WebError::InvalidUrl("unterminated IPv6 host".into()))?;
        let port = if after.is_empty() {
            None
        } else {
            let digits = after
                .strip_prefix(':')
                .ok_or_else(|| WebError::InvalidUrl("invalid IPv6 port".into()))?;
            Some(
                digits
                    .parse::<u16>()
                    .map_err(|_| WebError::InvalidUrl("invalid port".into()))?,
            )
        };
        (host.to_string(), port)
    } else if let Some((h, p)) = hostport.rsplit_once(':') {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) {
            (hostport.to_string(), None)
        } else {
            let port = p
                .parse::<u16>()
                .map_err(|_| WebError::InvalidUrl("invalid port".into()))?;
            (h.to_string(), Some(port))
        }
    } else {
        (hostport.to_string(), None)
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err(WebError::InvalidUrl("missing host".into()));
    }
    Ok(ParsedHttpUrl { scheme, host, port })
}

pub fn host_matches_domain(host: &str, domain: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || domain.is_empty() {
        return false;
    }
    host == domain || host.ends_with(&format!(".{domain}"))
}

pub fn is_local_or_private_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host == "localhost.localdomain"
        || host.ends_with(".local")
    {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip_is_non_public(ip);
    }
    false
}

fn ip_is_non_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return ip_is_non_public(IpAddr::V4(v4));
            }
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_unspecified()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Freshness window, e.g. `"30d"`, `"7d"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Publication date / age as reported by the provider (news freshness).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub results: Vec<WebSearchHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebOpenRequest {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebOpenResult {
    pub url: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebFetchRequest {
    pub url: String,
    #[serde(default = "default_get")]
    pub method: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

fn default_get() -> String {
    "GET".into()
}

impl Default for WebFetchRequest {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: default_get(),
            headers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebFetchResult {
    pub url: String,
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    pub body: String,
    #[serde(default)]
    pub truncated: bool,
    /// `Retry-After` in seconds (429 / 503), when the server sent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebExtractRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Inline content when the document is not yet in ResearchContext.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebExtractResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<Uuid>,
    pub text: String,
    #[serde(default)]
    pub chunks: Vec<String>,
    #[serde(default)]
    pub links: Vec<String>,
}
