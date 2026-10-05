//! API discovery from a browser network capture.
//!
//! Single-page apps usually render data they fetched as JSON. Taking that JSON
//! (with provenance) is more reliable than scraping the DOM it was painted into.

use reqwest::Url;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const MAX_CANDIDATES: usize = 20;
const MAX_LISTED_RESPONSES: usize = 50;
/// Budget for JSON bodies embedded into node output.
const MAX_JSON_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkSummary {
    /// Counts and a body-less list of captured responses.
    pub network: Value,
    /// Structured-data sources, best first: same site, captured JSON, larger bodies.
    pub api_candidates: Vec<Value>,
}

/// `network` is the browser's `{requests, responses, hook}` capture.
pub fn summarize(network: &Value, page_url: &str, fetched_at: &str) -> NetworkSummary {
    let empty = Vec::new();
    let requests = network
        .get("requests")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let responses = network
        .get("responses")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let page_site = site_of(page_url);

    let mut candidates: Vec<(bool, bool, usize, Value)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in responses {
        let url = r.get("url").and_then(Value::as_str).unwrap_or_default();
        let content_type = r
            .get("content_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = r.get("body").and_then(Value::as_str);
        let parsed = body.and_then(|b| {
            let t = b.trim_start();
            (content_type.contains("json") || t.starts_with('{') || t.starts_with('['))
                .then(|| serde_json::from_str::<Value>(b).ok())
                .flatten()
        });
        let Some(parsed) = parsed else {
            continue;
        };
        if url.is_empty() || !seen.insert(url.to_string()) {
            continue;
        }
        let body = body.unwrap_or_default();
        let body_length = number(r.get("body_length")).unwrap_or(body.len() as u64) as usize;
        let same_site = site_of(url) == page_site;
        let candidate = json!({
            "url": url,
            "method": r.get("method").and_then(Value::as_str).unwrap_or("GET"),
            "status": number(r.get("status")),
            "content_type": content_type,
            "kind": r.get("kind").and_then(Value::as_str).unwrap_or("fetch"),
            "same_site": same_site,
            "captured": true,
            "body_length": body_length,
            "body_truncated": body_length > body.len(),
            "request_body": r.get("request_body").cloned().unwrap_or(Value::Null),
            "json": parsed,
            "source": {
                "url": url,
                "method": "network_json",
                "timestamp": fetched_at,
                "content_hash": format!("sha256:{:x}", Sha256::digest(body.as_bytes())),
            },
        });
        candidates.push((same_site, true, body_length, candidate));
    }

    // fetch()/XHR the page hook missed (it starts after the earliest scripts).
    let mut fetch_requests = 0;
    let mut blocked = 0;
    for r in requests {
        let destination = r.get("destination").and_then(Value::as_str).unwrap_or("");
        if r.get("blocked").and_then(Value::as_bool).unwrap_or(false) {
            blocked += 1;
            continue;
        }
        if !matches!(destination, "none" | "empty") {
            continue;
        }
        fetch_requests += 1;
        let url = r.get("url").and_then(Value::as_str).unwrap_or_default();
        if url.is_empty() || !seen.insert(url.to_string()) || !looks_like_api(url) {
            continue;
        }
        let same_site = site_of(url) == page_site;
        candidates.push((
            same_site,
            false,
            0,
            json!({
                "url": url,
                "method": r.get("method").and_then(Value::as_str).unwrap_or("GET"),
                "same_site": same_site,
                "captured": false,
                "json": Value::Null,
                "source": { "url": url, "method": "network_request", "timestamp": fetched_at },
            }),
        ));
    }

    candidates.sort_by(|a, b| (b.0, b.1, b.2).cmp(&(a.0, a.1, a.2)));
    let mut budget = MAX_JSON_BYTES;
    let api_candidates = candidates
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(_, _, len, mut c)| {
            if c["captured"] == true {
                if len > budget {
                    c["json"] = Value::Null;
                    c["json_omitted"] = json!(true);
                } else {
                    budget -= len;
                }
            }
            c
        })
        .collect();

    let listed: Vec<Value> = responses
        .iter()
        .take(MAX_LISTED_RESPONSES)
        .map(|r| {
            json!({
                "url": r.get("url").cloned().unwrap_or(Value::Null),
                "method": r.get("method").cloned().unwrap_or(Value::Null),
                "status": number(r.get("status")),
                "content_type": r.get("content_type").cloned().unwrap_or(Value::Null),
                "kind": r.get("kind").cloned().unwrap_or(Value::Null),
                "body_length": number(r.get("body_length")),
            })
        })
        .collect();
    NetworkSummary {
        network: json!({
            "hook": network.get("hook").and_then(Value::as_bool).unwrap_or(false),
            "requests_total": requests.len(),
            "fetch_requests": fetch_requests,
            "blocked_requests": blocked,
            "responses": listed,
        }),
        api_candidates,
    }
}

/// JS numbers arrive as floats (`200.0`).
fn number(v: Option<&Value>) -> Option<u64> {
    v.and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
}

fn looks_like_api(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    path.ends_with(".json")
        || ["/api/", "/graphql", "/v1/", "/v2/", "/v3/", "/rest/", "/ajax/", "/_next/data/"]
            .iter()
            .any(|p| lower.contains(p))
}

/// Registrable-ish site: the last two host labels (`api.shop.com` → `shop.com`).
fn site_of(url: &str) -> String {
    let host = Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    let labels: Vec<&str> = host.split('.').collect();
    labels[labels.len().saturating_sub(2)..].join(".")
}

/// Fill a refetched JSON body into a `captured: false` candidate.
pub fn attach_refetch(candidate: &mut Value, body: &str, fetched_at: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let Some(obj) = candidate.as_object_mut() else {
        return false;
    };
    let url = obj.get("url").cloned().unwrap_or(Value::Null);
    obj.insert("json".into(), parsed);
    obj.insert("captured".into(), json!(true));
    obj.insert("body_length".into(), json!(body.len()));
    let mut source = Map::new();
    source.insert("url".into(), url);
    source.insert("method".into(), json!("api_refetch"));
    source.insert("timestamp".into(), json!(fetched_at));
    source.insert(
        "content_hash".into(),
        json!(format!("sha256:{:x}", Sha256::digest(body.as_bytes()))),
    );
    obj.insert("source".into(), Value::Object(source));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> Value {
        json!({
            "hook": true,
            "requests": [
                { "url": "https://shop.test/", "method": "GET", "destination": "document", "main_frame": true },
                { "url": "https://shop.test/app.js", "method": "GET", "destination": "script", "main_frame": false },
                { "url": "https://shop.test/api/products?page=1", "method": "GET", "destination": "none", "main_frame": false },
                { "url": "https://shop.test/api/early-config", "method": "GET", "destination": "none", "main_frame": false },
                { "url": "https://cdn.other.test/pixel", "method": "GET", "destination": "none", "main_frame": false },
                { "url": "http://127.0.0.1:9000/internal", "method": "GET", "destination": "none", "main_frame": false, "blocked": true }
            ],
            "responses": [
                { "kind": "fetch", "url": "https://shop.test/api/products?page=1", "method": "GET", "status": 200.0,
                  "content_type": "application/json", "body": "{\"products\":[{\"name\":\"iPhone 17\",\"price\":999}]}", "body_length": 49.0 },
                { "kind": "xhr", "url": "https://stats.other.test/v1/collect", "method": "POST", "status": 204.0,
                  "content_type": "application/json", "body": "{}", "body_length": 2.0 },
                { "kind": "fetch", "url": "https://shop.test/readme.html", "method": "GET", "status": 200.0,
                  "content_type": "text/html", "body": null }
            ]
        })
    }

    #[test]
    fn ranks_same_site_json_first() {
        let s = summarize(&capture(), "https://www.shop.test/catalog", "2026-10-05T12:00:00Z");
        let urls: Vec<&str> = s.api_candidates.iter().map(|c| c["url"].as_str().unwrap()).collect();
        assert_eq!(
            urls,
            vec![
                "https://shop.test/api/products?page=1",
                "https://shop.test/api/early-config",
                "https://stats.other.test/v1/collect",
            ]
        );
        let top = &s.api_candidates[0];
        assert_eq!(top["status"], 200);
        assert_eq!(top["json"]["products"][0]["price"], 999);
        assert_eq!(top["source"]["method"], "network_json");
        assert!(top["source"]["content_hash"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(s.api_candidates[1]["captured"], false);

        assert_eq!(s.network["requests_total"], 6);
        assert_eq!(s.network["fetch_requests"], 3);
        assert_eq!(s.network["blocked_requests"], 1);
        assert_eq!(s.network["responses"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn refetch_fills_candidate() {
        let mut c = json!({ "url": "https://shop.test/api/early-config", "captured": false, "json": null });
        assert!(attach_refetch(&mut c, "{\"currency\":\"USD\"}", "t"));
        assert_eq!(c["json"]["currency"], "USD");
        assert_eq!(c["source"]["method"], "api_refetch");
        assert!(!attach_refetch(&mut c, "<html>", "t"));
    }
}
