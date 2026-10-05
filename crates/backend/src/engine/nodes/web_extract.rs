//! `web.extract` — pull named fields out of pages with an LLM, then verify every
//! value against the source it claims (a JSON path or an exact quote).
//!
//! Sources are offered most-reliable first: JSON-LD, microdata, embedded app
//! state, captured API JSON, meta tags, tables, then page text. Values the model
//! cannot point to are reported as unverified (or dropped with `strict`).

use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use chrono::Local;
use futures::future::join_all;
use serde_json::{Map, Value, json};

use super::ai::{extract_json_object, openai_from_ctx, resolve_model};
use super::web::{BrowserExtras, PAGE_TIMEOUT_MS, PageReader, RenderMode, config_u64, urls_from_value};
use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};
use crate::integrations::openai::chat_complete;
use crate::integrations::web_reader::ReadOptions;
use crate::integrations::web_structured::json_path_get;

/// Characters of source material sent to the model per page.
const DEFAULT_SOURCE_CHARS: usize = 12_000;
/// Share of the budget for structured sources; the rest is page text.
const STRUCTURED_SHARE: f64 = 0.7;

const SYSTEM: &str = "You extract data from web page sources for a research agent.\n\
Reply with JSON only: {\"fields\": {\"<field>\": {\"value\": <value or null>, \"source\": \"<source id, e.g. S2>\", \
\"path\": \"<JSONPath such as $.offers.price, for JSON sources>\", \"quote\": \"<exact substring copied from a text source>\"}}}.\n\
Rules: use only the given sources; prefer earlier (structured) sources; copy values as they appear \
(numbers stay numbers); every non-null value must name its source and a path or an exact quote; \
use null when the sources do not contain the field — never guess.";

pub struct WebExtractNode;

/// One numbered source shown to the model.
#[derive(Debug, Clone)]
pub(crate) struct Source {
    pub id: String,
    /// `json_ld` | `microdata` | `embedded_json` | `network_json` | `api_refetch` | `meta` |
    /// `table` | `http_dom` | `browser_dom`
    pub kind: String,
    pub url: String,
    pub content_hash: Option<String>,
    /// Parsed data for JSON sources (path verification).
    pub json: Option<Value>,
    /// What the model sees (may be truncated).
    pub text: String,
}

#[async_trait]
impl NodeHandler for WebExtractNode {
    fn type_id(&self) -> &'static str {
        type_ids::WEB_EXTRACT
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let instruction = match node.config.get("instruction").cloned() {
            Some(raw) => match ctx.resolve_value(raw)? {
                Value::String(s) => s,
                Value::Null => String::new(),
                other => other.to_string(),
            },
            None => String::new(),
        };
        if instruction.trim().is_empty() {
            return Err("web.extract: `instruction` is empty — describe what to extract".into());
        }
        let schema = parse_schema(node.config.get("schema"));
        let strict = node
            .config
            .get("strict")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let raw_url = node.config.get("url").cloned().unwrap_or(Value::Null);
        let mut urls = urls_from_value(&ctx.resolve_value(raw_url)?);
        urls.truncate(config_u64(node, "limit", 3).clamp(1, 5) as usize);
        if urls.is_empty() {
            return Err("web.extract: url resolved to empty text".into());
        }

        let endpoint = openai_from_ctx(ctx, node, "web.extract").await?;
        let model = resolve_model(node, ctx, &endpoint)?;
        let max_tokens = config_u64(node, "max_tokens", 1_500).max(64);
        let budget = config_u64(node, "max_source_chars", DEFAULT_SOURCE_CHARS as u64)
            .clamp(1_000, 60_000) as usize;

        let reader = PageReader::new(
            ctx,
            RenderMode::from_config(node),
            BrowserExtras::from_config(node),
            PAGE_TIMEOUT_MS * 2,
        )
        .with_structured(true);
        let opts = ReadOptions {
            max_chars: budget,
            selector: None,
            include_links: false,
        };

        let pages = join_all(urls.iter().map(|url| reader.read(url, &opts, None))).await;
        let today = Local::now().format("%Y-%m-%d").to_string();
        let jobs = urls.iter().zip(pages).map(|(url, page)| {
            let endpoint = &endpoint;
            let model = &model;
            let instruction = &instruction;
            let schema = &schema;
            let today = &today;
            async move {
                let page = match page {
                    Ok(page) => page,
                    Err(e) => return (e.to_json(url), Value::Null),
                };
                let sources = build_sources(&page, budget);
                let prompt = build_prompt(instruction, schema, &sources, today);
                let (raw, usage, _) = match chat_complete(
                    endpoint, model, SYSTEM, &prompt, 0.0, max_tokens, 60_000,
                )
                .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        return (json!({ "url": url, "error": format!("web.extract: {e}") }), Value::Null)
                    }
                };
                let claims = extract_json_object(&raw)
                    .map(|v| v.get("fields").cloned().unwrap_or(v))
                    .unwrap_or(Value::Null);
                (verify_page(&page, &sources, &claims, schema, strict), usage)
            }
        });
        let results: Vec<(Value, Value)> = join_all(jobs).await;

        let items: Vec<Value> = results.iter().map(|(item, _)| item.clone()).collect();
        let usage: Vec<Value> = results.into_iter().map(|(_, u)| u).collect();
        let Some(first) = items.iter().find(|i| i.get("error").is_none()).cloned() else {
            let errors: Vec<&str> = items
                .iter()
                .filter_map(|i| i.get("error").and_then(Value::as_str))
                .collect();
            return Err(format!("web.extract: {}", errors.join("; ")));
        };
        Ok(NodeOutput::data(json!({
            "data": first["data"],
            "evidence": first["evidence"],
            "unverified": first["unverified"],
            "url": first["url"],
            "items": items,
            "model": model,
            "usage": usage,
        })))
    }
}

/// `schema`: object `{name: description}` or text lines `name: description`.
pub(crate) fn parse_schema(value: Option<&Value>) -> Vec<(String, String)> {
    match value {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
            .collect(),
        Some(Value::String(text)) => text
            .lines()
            .filter_map(|line| {
                let line = line.trim().trim_start_matches(['-', '*']).trim();
                if line.is_empty() {
                    return None;
                }
                let (name, desc) = line.split_once(':').unwrap_or((line, ""));
                let name = name.trim();
                (!name.is_empty()).then(|| (name.to_string(), desc.trim().to_string()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Numbered sources, most reliable first, within a character budget.
pub(crate) fn build_sources(page: &Value, budget: usize) -> Vec<Source> {
    let page_url = page["url"].as_str().unwrap_or_default().to_string();
    let page_hash = page["provenance"]["content_hash"].as_str().map(str::to_string);
    let render = page["render"].as_str().unwrap_or("http");
    let structured = &page["structured"];
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();

    let mut candidates: Vec<(String, String, Option<String>, Value)> = Vec::new();
    for item in list(&structured["json_ld"]) {
        candidates.push(("json_ld".into(), page_url.clone(), page_hash.clone(), item));
    }
    for item in list(&structured["microdata"]) {
        candidates.push(("microdata".into(), page_url.clone(), page_hash.clone(), item));
    }
    for item in list(&structured["embedded_json"]) {
        candidates.push((
            "embedded_json".into(),
            page_url.clone(),
            page_hash.clone(),
            item["data"].clone(),
        ));
    }
    for c in list(&page["api_candidates"]) {
        if c["json"].is_null() {
            continue;
        }
        candidates.push((
            c["source"]["method"].as_str().unwrap_or("network_json").to_string(),
            c["url"].as_str().unwrap_or_default().to_string(),
            c["source"]["content_hash"].as_str().map(str::to_string),
            c["json"].clone(),
        ));
    }
    if structured["meta"].as_object().is_some_and(|m| !m.is_empty()) {
        candidates.push(("meta".into(), page_url.clone(), page_hash.clone(), structured["meta"].clone()));
    }
    for table in list(&structured["tables"]) {
        candidates.push(("table".into(), page_url.clone(), page_hash.clone(), table));
    }

    let mut sources = Vec::new();
    let mut left = (budget as f64 * STRUCTURED_SHARE) as usize;
    for (kind, url, hash, data) in candidates {
        if left < 200 {
            break;
        }
        let full = serde_json::to_string(&data).unwrap_or_default();
        let text: String = full.chars().take(left).collect();
        left -= text.chars().count();
        sources.push(Source {
            id: format!("S{}", sources.len() + 1),
            kind,
            url,
            content_hash: hash,
            json: Some(data),
            text,
        });
    }
    let text_budget = budget.saturating_sub((budget as f64 * STRUCTURED_SHARE) as usize) + left;
    let page_text: String = page["text"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(text_budget)
        .collect();
    if !page_text.trim().is_empty() {
        sources.push(Source {
            id: format!("S{}", sources.len() + 1),
            kind: format!("{render}_dom"),
            url: page_url,
            content_hash: page_hash,
            json: None,
            text: page_text,
        });
    }
    sources
}

pub(crate) fn build_prompt(
    instruction: &str,
    schema: &[(String, String)],
    sources: &[Source],
    today: &str,
) -> String {
    let fields = if schema.is_empty() {
        "choose concise snake_case field names that cover the task".to_string()
    } else {
        schema
            .iter()
            .map(|(n, d)| if d.is_empty() { format!("- {n}") } else { format!("- {n}: {d}") })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut out = format!("Task: {instruction}\nToday: {today}\nFields:\n{fields}\n\nSources:\n");
    for s in sources {
        out.push_str(&format!("[{}] kind={} url={}\n{}\n\n", s.id, s.kind, s.url, s.text));
    }
    out
}

/// Check each claimed value against its source; build `data` / `evidence`.
pub(crate) fn verify_page(
    page: &Value,
    sources: &[Source],
    claims: &Value,
    schema: &[(String, String)],
    strict: bool,
) -> Value {
    let fetched_at = page["provenance"]["fetched_at"].clone();
    let mut names: Vec<String> = schema.iter().map(|(n, _)| n.clone()).collect();
    if let Some(map) = claims.as_object() {
        for k in map.keys() {
            if !names.contains(k) {
                names.push(k.clone());
            }
        }
    }

    let mut data = Map::new();
    let mut evidence = Map::new();
    let mut unverified = Vec::new();
    for name in names {
        let claim = &claims[&name];
        let value = claim.get("value").cloned().unwrap_or_else(|| {
            // Tolerate `{"field": value}` replies.
            if claim.is_object() { Value::Null } else { claim.clone() }
        });
        if value.is_null() {
            data.insert(name, Value::Null);
            continue;
        }
        let source_id = claim["source"].as_str().unwrap_or_default();
        let path = claim["path"].as_str().filter(|p| !p.trim().is_empty());
        let quote = claim["quote"].as_str().filter(|q| !q.trim().is_empty());
        let source = sources.iter().find(|s| s.id.eq_ignore_ascii_case(source_id.trim()));
        let verified = source.is_some_and(|s| check_claim(s, &value, path, quote));

        if !verified {
            unverified.push(name.clone());
        }
        data.insert(
            name.clone(),
            if verified || !strict { value.clone() } else { Value::Null },
        );
        let mut src = json!({
            "id": source_id,
            "url": source.map(|s| s.url.clone()),
            "method": source.map(|s| s.kind.clone()),
            "timestamp": fetched_at,
            "content_hash": source.and_then(|s| s.content_hash.clone()),
        });
        if let Some(p) = path {
            src["json_path"] = json!(p);
        }
        if let Some(q) = quote {
            src["quote"] = json!(q);
        }
        evidence.insert(name, json!({ "value": value, "verified": verified, "source": src }));
    }

    json!({
        "url": page["url"],
        "title": page["title"],
        "render": page["render"],
        "data": data,
        "evidence": evidence,
        "unverified": unverified,
        "sources": sources
            .iter()
            .map(|s| json!({ "id": s.id, "kind": s.kind, "url": s.url }))
            .collect::<Vec<_>>(),
    })
}

/// A JSON path must lead to the value; a quote must occur in the source and contain the value.
fn check_claim(source: &Source, value: &Value, path: Option<&str>, quote: Option<&str>) -> bool {
    if let (Some(json), Some(path)) = (&source.json, path) {
        if json_path_get(json, path).is_some_and(|found| loosely_equal(found, value)) {
            return true;
        }
    }
    if let Some(quote) = quote {
        let q = normalize(quote);
        let haystack = normalize(&source.text);
        let v = normalize(&value_text(value));
        return !q.is_empty() && haystack.contains(&q) && (v.is_empty() || q.contains(&v));
    }
    false
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

fn loosely_equal(a: &Value, b: &Value) -> bool {
    let (sa, sb) = (normalize(&value_text(a)), normalize(&value_text(b)));
    if sa == sb {
        return true;
    }
    match (sa.replace(',', ".").parse::<f64>(), sb.replace(',', ".").parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() < 1e-9,
        _ => false,
    }
}

fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Value {
        json!({
            "url": "https://shop.test/p",
            "title": "iPhone 17",
            "render": "http",
            "text": "# iPhone 17\n\nЦена: 999 $. В наличии на складе.",
            "provenance": { "fetched_at": "2026-10-05T12:00:00Z", "content_hash": "sha256:abc" },
            "structured": {
                "json_ld": [{ "@type": "Product", "name": "iPhone 17", "offers": { "price": "999" } }],
                "meta": { "og:title": "iPhone 17" },
                "tables": [], "microdata": [], "embedded_json": []
            },
            "api_candidates": [
                { "url": "https://shop.test/api/stock", "json": { "stock": { "count": 12 } },
                  "source": { "method": "network_json", "content_hash": "sha256:def" } },
                { "url": "https://shop.test/api/none", "json": null, "source": { "method": "network_request" } }
            ]
        })
    }

    #[test]
    fn sources_are_ordered_and_budgeted() {
        let s = build_sources(&page(), 12_000);
        let kinds: Vec<&str> = s.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds, vec!["json_ld", "network_json", "meta", "http_dom"]);
        assert_eq!(s[1].url, "https://shop.test/api/stock");
        assert_eq!(s[1].content_hash.as_deref(), Some("sha256:def"));
        assert_eq!(s[3].id, "S4");

        let tiny = build_sources(&page(), 1_000);
        assert!(tiny.iter().map(|s| s.text.chars().count()).sum::<usize>() <= 1_000);
    }

    #[test]
    fn claims_are_verified_against_sources() {
        let p = page();
        let sources = build_sources(&p, 12_000);
        let claims = json!({
            "price": { "value": 999, "source": "S1", "path": "$.offers.price" },
            "stock": { "value": 12, "source": "S2", "path": "$.stock.count" },
            "availability": { "value": "В наличии", "source": "S4", "quote": "В наличии на складе" },
            "color": { "value": "Black", "source": "S4", "quote": "Цвет: Black" },
            "warranty": { "value": null }
        });
        let schema = parse_schema(Some(&json!("name: product name\nprice\nwarranty")));
        let out = verify_page(&p, &sources, &claims, &schema, false);
        assert_eq!(out["data"]["price"], 999);
        assert_eq!(out["evidence"]["price"]["verified"], true);
        assert_eq!(out["evidence"]["price"]["source"]["method"], "json_ld");
        assert_eq!(out["evidence"]["price"]["source"]["json_path"], "$.offers.price");
        assert_eq!(out["evidence"]["stock"]["source"]["url"], "https://shop.test/api/stock");
        assert_eq!(out["evidence"]["availability"]["verified"], true);
        assert_eq!(out["evidence"]["availability"]["source"]["method"], "http_dom");
        assert_eq!(out["evidence"]["color"]["verified"], false, "quote not in page");
        assert_eq!(out["data"]["name"], Value::Null, "schema field without a claim");
        assert_eq!(out["data"]["warranty"], Value::Null);
        assert_eq!(out["unverified"], json!(["color"]));

        let strict = verify_page(&p, &sources, &claims, &schema, true);
        assert_eq!(strict["data"]["color"], Value::Null);
        assert_eq!(strict["data"]["price"], 999);
    }

    #[test]
    fn wrong_path_is_not_verified() {
        let p = page();
        let sources = build_sources(&p, 12_000);
        let claims = json!({ "price": { "value": 1099, "source": "S1", "path": "$.offers.price" } });
        let out = verify_page(&p, &sources, &claims, &[], false);
        assert_eq!(out["evidence"]["price"]["verified"], false);
    }

    #[test]
    fn schema_parsing() {
        assert_eq!(
            parse_schema(Some(&json!("- name: название\n* price: цена\n\nstock"))),
            vec![
                ("name".to_string(), "название".to_string()),
                ("price".to_string(), "цена".to_string()),
                ("stock".to_string(), String::new())
            ]
        );
        assert_eq!(parse_schema(Some(&json!({ "a": "x" }))), vec![("a".into(), "x".into())]);
        assert!(parse_schema(None).is_empty());
    }
}
