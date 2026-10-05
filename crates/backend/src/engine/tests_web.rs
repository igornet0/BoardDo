//! Classify → condition → web.search branch and page reading (no live LLM / network).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use boarddo_shared::v2::{
    WebError, WebExtractRequest, WebExtractResult, WebFetchRequest, WebFetchResult,
    WebOpenRequest, WebOpenResult, WebPolicy, WebSearchHit, WebSearchRequest, WebSearchResult,
};
use boarddo_shared::{Edge, Node, Position, WorkflowDefinition, type_ids};
use serde_json::json;
use uuid::Uuid;

use crate::engine::Engine;
use crate::integrations::browser::{
    BrowserRenderer, NullBrowserRenderer, RenderRequest, RenderResult,
};
use crate::integrations::media::MediaStore;
use crate::integrations::web::{HttpWebGateway, StaticSearchProvider, WebGateway};

fn node(id: &str, type_id: &str, config: serde_json::Value) -> Node {
    Node {
        id: id.into(),
        type_id: type_id.into(),
        category: None,
        position: Position::default(),
        config,
    }
}

fn edge(id: &str, source: &str, target: &str, port: Option<&str>) -> Edge {
    Edge {
        id: id.into(),
        source: source.into(),
        target: target.into(),
        source_port: port.map(str::to_string),
        target_port: None,
    }
}

fn engine_with_hits() -> Engine {
    let gw = HttpWebGateway::with_search(Arc::new(StaticSearchProvider {
        hits: vec![WebSearchHit {
            title: "Official docs".into(),
            url: "https://docs.rs/serde".into(),
            snippet: "Serde is a framework".into(),
            source: Some("test".into()),
            published: None,
        }],
    }));
    Engine::new().with_web(Arc::new(gw))
}

#[tokio::test]
async fn search_runs_when_classify_needs_web() {
    let engine = engine_with_hits();
    let definition = WorkflowDefinition {
        nodes: vec![
            node("manual", type_ids::TRIGGER_MANUAL, json!({})),
            node(
                "classify",
                type_ids::DATA_TRANSFORM,
                json!({
                    "mapping": {
                        "needs_web_search": true,
                        "query": "{{trigger.text}}",
                        "label": "needs_web_search"
                    }
                }),
            ),
            node(
                "gate",
                type_ids::LOGIC_CONDITION,
                json!({ "expression": "{{nodes.classify.output.needs_web_search}}" }),
            ),
            node(
                "search",
                type_ids::WEB_SEARCH,
                json!({ "query": "{{nodes.classify.output.query}}", "limit": 5, "read_pages": 0 }),
            ),
            node(
                "log_web",
                type_ids::DEBUG_LOG,
                json!({ "message": "{{nodes.search.output.results}}" }),
            ),
            node(
                "log_skip",
                type_ids::DEBUG_LOG,
                json!({ "message": "no search" }),
            ),
        ],
        edges: vec![
            edge("e1", "manual", "classify", None),
            edge("e2", "classify", "gate", None),
            edge("e3", "gate", "search", Some("true")),
            edge("e4", "search", "log_web", None),
            edge("e5", "gate", "log_skip", Some("false")),
        ],
    };

    let result = engine
        .execute(
            Uuid::now_v7(),
            Uuid::now_v7(),
            1,
            &definition,
            json!({ "text": "best serde docs" }),
            None,
        )
        .await;

    assert!(result.error.is_none(), "{:?}", result.error);
    let ran: Vec<_> = result
        .node_executions
        .iter()
        .map(|n| n.node_id.as_str())
        .collect();
    assert!(ran.contains(&"search"), "{ran:?}");
    assert!(ran.contains(&"log_web"), "{ran:?}");
    assert!(!ran.contains(&"log_skip"), "{ran:?}");

    let search_out = result.context.nodes.get("search").expect("search output");
    assert_eq!(search_out["results"][0]["url"], "https://docs.rs/serde");
    assert_eq!(search_out["query"], "best serde docs");
}

#[tokio::test]
async fn search_skipped_when_knowledge_is_enough() {
    let engine = engine_with_hits();
    let definition = WorkflowDefinition {
        nodes: vec![
            node("manual", type_ids::TRIGGER_MANUAL, json!({})),
            node(
                "classify",
                type_ids::DATA_TRANSFORM,
                json!({
                    "mapping": {
                        "needs_web_search": false,
                        "query": "{{trigger.text}}",
                        "label": "knowledge"
                    }
                }),
            ),
            node(
                "gate",
                type_ids::LOGIC_CONDITION,
                json!({ "expression": "{{nodes.classify.output.needs_web_search}}" }),
            ),
            node(
                "search",
                type_ids::WEB_SEARCH,
                json!({ "query": "should not run" }),
            ),
            node(
                "log_web",
                type_ids::DEBUG_LOG,
                json!({ "message": "searched" }),
            ),
            node(
                "log_skip",
                type_ids::DEBUG_LOG,
                json!({ "message": "direct" }),
            ),
        ],
        edges: vec![
            edge("e1", "manual", "classify", None),
            edge("e2", "classify", "gate", None),
            edge("e3", "gate", "search", Some("true")),
            edge("e4", "search", "log_web", None),
            edge("e5", "gate", "log_skip", Some("false")),
        ],
    };

    let result = engine
        .execute(
            Uuid::now_v7(),
            Uuid::now_v7(),
            1,
            &definition,
            json!({ "text": "what is 2+2" }),
            None,
        )
        .await;

    assert!(result.error.is_none(), "{:?}", result.error);
    let ran: Vec<_> = result
        .node_executions
        .iter()
        .map(|n| n.node_id.as_str())
        .collect();
    assert!(!ran.contains(&"search"), "{ran:?}");
    assert!(ran.contains(&"log_skip"), "{ran:?}");
}

/// Serves canned search hits and HTML pages; unknown URLs answer 404.
/// URLs containing `forbidden` answer 403, `ratelimit-once` answers 429
/// (`Retry-After: 0`) on the first call, `ratelimit-long` always 429 with an hour wait.
#[derive(Default)]
struct FakePagesGateway {
    hits: Vec<WebSearchHit>,
    pages: HashMap<String, String>,
    calls: std::sync::Mutex<HashMap<String, usize>>,
}

#[async_trait]
impl WebGateway for FakePagesGateway {
    async fn search(
        &self,
        _request: WebSearchRequest,
        _policy: &WebPolicy,
    ) -> Result<WebSearchResult, WebError> {
        Ok(WebSearchResult {
            results: self.hits.clone(),
        })
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
        request: WebFetchRequest,
        _policy: &WebPolicy,
    ) -> Result<WebFetchResult, WebError> {
        let calls = {
            let mut calls = self.calls.lock().unwrap();
            let n = calls.entry(request.url.clone()).or_default();
            *n += 1;
            *n
        };
        let body = self.pages.get(&request.url).cloned();
        let (status, retry_after) = if request.url.contains("forbidden") {
            (403, None)
        } else if request.url.contains("ratelimit-long") {
            (429, Some(3600))
        } else if request.url.contains("ratelimit-once") && calls == 1 {
            (429, Some(0))
        } else if body.is_some() {
            (200, None)
        } else {
            (404, None)
        };
        Ok(WebFetchResult {
            status,
            content_type: Some("text/html; charset=utf-8".into()),
            body: body.unwrap_or_default(),
            url: request.url,
            truncated: false,
            retry_after,
        })
    }

    async fn extract(
        &self,
        _request: WebExtractRequest,
        _policy: &WebPolicy,
    ) -> Result<WebExtractResult, WebError> {
        Err(WebError::NotConfigured)
    }
}

fn hit(url: &str, title: &str) -> WebSearchHit {
    WebSearchHit {
        title: title.into(),
        url: url.into(),
        snippet: format!("snippet of {title}"),
        source: Some("test".into()),
        published: None,
    }
}

fn page_with_date(h1: &str, published: &str) -> String {
    format!(
        "<html><head><meta property=\"article:published_time\" content=\"{published}\">\
         </head><body><article><h1>{h1}</h1><p>{h1} body</p></article></body></html>"
    )
}

fn engine_with_pages() -> Engine {
    let page = |h1: &str, body: &str| {
        format!(
            "<html><head><title>{h1}</title></head><body><nav>menu</nav>\
             <main><h1>{h1}</h1><p>{body}</p><span class=\"price\">42 $</span>\
             <a href=\"/next\">next</a></main></body></html>"
        )
    };
    let gw = FakePagesGateway {
        hits: vec![
            hit("https://a.test/1", "A"),
            hit("https://broken.test/", "Broken"),
            hit("https://b.test/2", "B"),
            hit("https://c.test/3", "C"),
        ],
        pages: HashMap::from([
            ("https://a.test/1".to_string(), page("Page A", "Alpha facts")),
            ("https://b.test/2".to_string(), page("Page B", "Beta facts")),
            ("https://c.test/3".to_string(), page("Page C", "Gamma facts")),
        ]),
        ..Default::default()
    };
    Engine::new()
        .with_web(Arc::new(gw))
        .with_browser(Arc::new(NullBrowserRenderer))
}

#[tokio::test]
async fn search_reads_top_pages_and_backfills_failures() {
    let engine = engine_with_pages();
    let definition = WorkflowDefinition {
        nodes: vec![
            node("manual", type_ids::TRIGGER_MANUAL, json!({})),
            node(
                "search",
                type_ids::WEB_SEARCH,
                json!({ "query": "{{trigger.text}}", "read_pages": 2 }),
            ),
        ],
        edges: vec![edge("e1", "manual", "search", None)],
    };
    let result = engine
        .execute(
            Uuid::now_v7(),
            Uuid::now_v7(),
            1,
            &definition,
            json!({ "text": "facts" }),
            None,
        )
        .await;
    assert!(result.error.is_none(), "{:?}", result.error);

    let out = result.context.nodes.get("search").expect("search output");
    // The broken hit fails, so the next hit (B) is read instead; C stays unread.
    assert_eq!(out["pages_read"], 2);
    assert!(out["results"][0]["content"].as_str().unwrap().contains("Alpha facts"));
    assert!(out["results"][1]["content_error"].as_str().unwrap().contains("404"));
    assert!(out["results"][2]["content"].as_str().unwrap().contains("Beta facts"));
    assert!(out["results"][3].get("content").is_none());

    let context = out["context"].as_str().unwrap();
    assert!(
        context.contains("\n\n[1] A\nURL: https://a.test/1\nPublished: unknown\n# Page A"),
        "{context}"
    );
    assert!(context.contains("Period: any time"), "{context}");
    assert!(context.contains("snippet of C"), "{context}");
    assert!(!context.contains("menu"), "{context}");
}

#[tokio::test]
async fn open_reads_pages_from_search_results_with_fields() {
    let engine = engine_with_pages();
    let definition = WorkflowDefinition {
        nodes: vec![
            node("manual", type_ids::TRIGGER_MANUAL, json!({})),
            node(
                "search",
                type_ids::WEB_SEARCH,
                json!({ "query": "{{trigger.text}}", "read_pages": 0 }),
            ),
            node(
                "read",
                type_ids::WEB_OPEN,
                json!({
                    "url": "{{nodes.search.output.results}}",
                    "limit": 3,
                    "fields": "price: .price\nnext: a@href",
                }),
            ),
        ],
        edges: vec![
            edge("e1", "manual", "search", None),
            edge("e2", "search", "read", None),
        ],
    };
    let result = engine
        .execute(
            Uuid::now_v7(),
            Uuid::now_v7(),
            1,
            &definition,
            json!({ "text": "facts" }),
            None,
        )
        .await;
    assert!(result.error.is_none(), "{:?}", result.error);

    let out = result.context.nodes.get("read").expect("read output");
    assert_eq!(out["title"], "Page A");
    assert_eq!(out["data"]["price"], "42 $");
    assert_eq!(out["data"]["next"], "https://a.test/next");
    assert_eq!(out["count"], 2);
    assert_eq!(out["pages"].as_array().unwrap().len(), 3);
    assert!(out["pages"][1]["error"].as_str().unwrap().contains("404"));
    assert!(out["context"].as_str().unwrap().contains("Beta facts"));
}

#[tokio::test]
async fn open_fails_when_single_page_is_unreachable() {
    let engine = engine_with_pages();
    let definition = WorkflowDefinition {
        nodes: vec![
            node("manual", type_ids::TRIGGER_MANUAL, json!({})),
            node(
                "read",
                type_ids::WEB_OPEN,
                json!({ "url": "https://broken.test/" }),
            ),
        ],
        edges: vec![edge("e1", "manual", "read", None)],
    };
    let result = engine
        .execute(Uuid::now_v7(), Uuid::now_v7(), 1, &definition, json!({}), None)
        .await;
    let err = result.error.expect("web.open should fail");
    assert!(err.contains("404"), "{err}");
}

#[tokio::test]
async fn search_drops_pages_published_before_window() {
    let now = chrono::Utc::now();
    let fresh = (now - chrono::Duration::days(1)).to_rfc3339();
    let old = (now - chrono::Duration::days(200)).to_rfc3339();
    let gw = FakePagesGateway {
        hits: vec![
            hit("https://old.test/", "Old"),
            hit("https://fresh.test/", "Fresh"),
            hit("https://fresh2.test/", "Fresh 2"),
        ],
        pages: HashMap::from([
            ("https://old.test/".to_string(), page_with_date("Old news", &old)),
            ("https://fresh.test/".to_string(), page_with_date("Fresh news", &fresh)),
            ("https://fresh2.test/".to_string(), page_with_date("Fresh news 2", &fresh)),
        ]),
        ..Default::default()
    };
    let engine = Engine::new().with_web(Arc::new(gw));
    let definition = WorkflowDefinition {
        nodes: vec![
            node("manual", type_ids::TRIGGER_MANUAL, json!({})),
            node(
                "search",
                type_ids::WEB_SEARCH,
                json!({ "query": "{{trigger.text}}", "read_pages": 2, "freshness": "week" }),
            ),
        ],
        edges: vec![edge("e1", "manual", "search", None)],
    };
    let result = engine
        .execute(
            Uuid::now_v7(),
            Uuid::now_v7(),
            1,
            &definition,
            json!({ "text": "crypto" }),
            None,
        )
        .await;
    assert!(result.error.is_none(), "{:?}", result.error);

    let out = result.context.nodes.get("search").expect("search output");
    assert_eq!(out["freshness"], "week");
    assert_eq!(out["outdated_dropped"], 1);
    // The outdated page was replaced by the next hit, so two fresh pages were read.
    assert_eq!(out["pages_read"], 2);
    let urls: Vec<&str> = out["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["url"].as_str().unwrap())
        .collect();
    assert_eq!(urls, vec!["https://fresh.test/", "https://fresh2.test/"]);
    assert!(out["results"][0]["published"].as_str().is_some());

    let context = out["context"].as_str().unwrap();
    assert!(context.contains("Period: last 7 days"), "{context}");
    assert!(context.contains("(1 day ago)"), "{context}");
    assert!(!context.contains("Old news"), "{context}");
}

/// Renders canned HTML per URL and records every request.
struct FakeBrowser {
    pages: HashMap<String, String>,
    calls: std::sync::Mutex<Vec<RenderRequest>>,
}

#[async_trait]
impl BrowserRenderer for FakeBrowser {
    fn is_configured(&self) -> bool {
        true
    }

    async fn render(
        &self,
        request: RenderRequest,
        _policy: &WebPolicy,
    ) -> Result<RenderResult, WebError> {
        self.calls.lock().unwrap().push(request.clone());
        let html = self
            .pages
            .get(&request.url)
            .cloned()
            .ok_or_else(|| WebError::Upstream("browser: HTTP 502: unknown page".into()))?;
        Ok(RenderResult {
            final_url: request.url.clone(),
            title: "Rendered".into(),
            html,
            load_complete: true,
            wait_for_found: request.wait_for.as_ref().map(|_| true),
            script_result: request.script.as_ref().map(|_| json!({ "items": 3 })),
            // 1×1 transparent PNG.
            network: request.capture_network.then(|| {
                json!({
                    "hook": true,
                    "requests": [
                        { "url": "https://spa.test/api/products", "method": "GET", "destination": "none", "main_frame": false },
                        { "url": "https://spa.test/api/config", "method": "GET", "destination": "none", "main_frame": false }
                    ],
                    "responses": [
                        { "kind": "fetch", "url": "https://spa.test/api/products", "method": "GET", "status": 200.0,
                          "content_type": "application/json", "body": "{\"products\":[{\"name\":\"Widget\",\"price\":99}]}" }
                    ]
                })
            }),
            screenshot_png_base64: request.screenshot.then(|| {
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=".into()
            }),
            ..Default::default()
        })
    }
}

const SPA_SHELL: &str = r#"<html><head><title>App</title></head><body><div id="root"></div><script src="/bundle.js"></script></body></html>"#;

fn rendered_spa() -> String {
    format!(
        "<html><body><main><h1>Prices</h1><p>{}</p><span class=\"price\">99 $</span></main></body></html>",
        "Rendered by JavaScript. ".repeat(20)
    )
}

fn browser_engine() -> (Engine, Arc<FakeBrowser>, std::path::PathBuf) {
    let gw = FakePagesGateway {
        hits: vec![],
        pages: HashMap::from([
            ("https://spa.test/".to_string(), SPA_SHELL.to_string()),
            ("https://spa.test/api/config".to_string(), "{\"currency\":\"USD\"}".to_string()),
            (
                "https://static.test/".to_string(),
                "<html><body><h1>Static</h1><p>Plain server-rendered page.</p></body></html>".to_string(),
            ),
        ]),
        ..Default::default()
    };
    let browser = Arc::new(FakeBrowser {
        pages: HashMap::from([
            ("https://spa.test/".to_string(), rendered_spa()),
            ("https://forbidden.test/".to_string(), rendered_spa()),
        ]),
        calls: Default::default(),
    });
    let media_dir = std::env::temp_dir().join(format!("boarddo-test-media-{}", Uuid::now_v7()));
    let engine = Engine::new()
        .with_web(Arc::new(gw))
        .with_browser(browser.clone())
        .with_media(MediaStore::new(&media_dir));
    (engine, browser, media_dir)
}

async fn run_open(engine: &Engine, config: serde_json::Value) -> crate::engine::ExecutionResult {
    let definition = WorkflowDefinition {
        nodes: vec![
            node("manual", type_ids::TRIGGER_MANUAL, json!({})),
            node("read", type_ids::WEB_OPEN, config),
        ],
        edges: vec![edge("e1", "manual", "read", None)],
    };
    engine
        .execute(Uuid::now_v7(), Uuid::now_v7(), 1, &definition, json!({}), None)
        .await
}

#[tokio::test]
async fn auto_render_uses_browser_only_for_js_shells() {
    let (engine, browser, _) = browser_engine();

    let result = run_open(&engine, json!({ "url": "https://spa.test/", "fields": "price: .price" })).await;
    assert!(result.error.is_none(), "{:?}", result.error);
    let out = result.context.nodes.get("read").unwrap();
    assert_eq!(out["render"], "browser");
    assert!(out["text"].as_str().unwrap().contains("Rendered by JavaScript"));
    assert_eq!(out["data"]["price"], "99 $");
    assert_eq!(browser.calls.lock().unwrap().len(), 1);

    let result = run_open(&engine, json!({ "url": "https://static.test/" })).await;
    let out = result.context.nodes.get("read").unwrap();
    assert_eq!(out["render"], "http");
    assert_eq!(browser.calls.lock().unwrap().len(), 1, "static page stays on HTTP");
}

#[tokio::test]
async fn blocked_response_is_not_retried_in_browser() {
    let (engine, browser, _) = browser_engine();
    let result = run_open(&engine, json!({ "url": "https://forbidden.test/" })).await;
    let err = result.error.expect("403 must fail");
    assert!(err.contains("403"), "{err}");
    assert!(browser.calls.lock().unwrap().is_empty(), "no browser retry after 403");
}

#[tokio::test]
async fn browser_mode_runs_script_and_saves_screenshot() {
    let (engine, browser, media_dir) = browser_engine();
    let result = run_open(
        &engine,
        json!({
            "url": "https://spa.test/",
            "render": "browser",
            "wait_for": "main h1",
            "script": "document.querySelectorAll('.item').length",
            "screenshot": true,
        }),
    )
    .await;
    assert!(result.error.is_none(), "{:?}", result.error);
    let out = result.context.nodes.get("read").unwrap();
    assert_eq!(out["render"], "browser");
    assert_eq!(out["wait_for_found"], true);
    assert_eq!(out["script_result"]["items"], 3);
    let uri = out["screenshot"].as_str().expect("screenshot uri");
    assert!(uri.starts_with("/api/media/") && uri.ends_with(".png"), "{uri}");
    assert!(out.get("screenshot_png_base64").is_none());
    let saved = std::fs::read_dir(&media_dir).unwrap().count();
    assert_eq!(saved, 1);

    let call = browser.calls.lock().unwrap()[0].clone();
    assert_eq!(call.wait_for.as_deref(), Some("main h1"));
    assert!(call.screenshot);
    let _ = std::fs::remove_dir_all(media_dir);
}

#[tokio::test]
async fn browser_mode_without_browser_explains_setup() {
    let engine = engine_with_pages();
    let result = run_open(&engine, json!({ "url": "https://a.test/1", "render": "browser" })).await;
    let err = result.error.expect("must fail without a browser");
    assert!(err.contains("BROWSER_AUTOMATION_URL"), "{err}");
}


const PRODUCT_PAGE: &str = r#"<html><head><title>iPhone 17</title>
  <meta property="og:title" content="iPhone 17">
  <script type="application/ld+json">{"@type":"Product","name":"iPhone 17","offers":{"price":"999"}}</script>
  </head><body><h1>iPhone 17</h1><span class="price">999 $</span></body></html>"#;

fn status_engine() -> Engine {
    let gw = FakePagesGateway {
        pages: HashMap::from([
            ("https://shop.test/p".to_string(), PRODUCT_PAGE.to_string()),
            ("https://ratelimit-once.test/".to_string(), PRODUCT_PAGE.to_string()),
            (
                "https://article.test/".to_string(),
                format!(
                    "<html><head><script src=\"https://www.google.com/recaptcha/api.js\"></script></head>\
                     <body><article><h1>News</h1><p>{}</p></article></body></html>",
                    "Long article text. ".repeat(200)
                ),
            ),
            (
                "https://challenge.test/".to_string(),
                "<html><head><title>Just a moment...</title></head><body>\
                 <script src=\"/cdn-cgi/challenge-platform/h/b/orchestrate.js\"></script>\
                 Checking your browser</body></html>"
                    .to_string(),
            ),
        ]),
        ..Default::default()
    };
    Engine::new()
        .with_web(Arc::new(gw))
        .with_browser(Arc::new(NullBrowserRenderer))
}

#[tokio::test]
async fn open_returns_structured_data_provenance_and_evidence() {
    let engine = status_engine();
    let result = run_open(
        &engine,
        json!({ "url": "https://shop.test/p", "fields": "price: .price\nmissing: .nope" }),
    )
    .await;
    assert!(result.error.is_none(), "{:?}", result.error);
    let out = result.context.nodes.get("read").unwrap();
    assert_eq!(out["structured"]["json_ld"][0]["offers"]["price"], "999");
    assert_eq!(out["structured"]["meta"]["og:title"], "iPhone 17");
    assert_eq!(out["provenance"]["method"], "http");
    assert_eq!(out["provenance"]["http_status"], 200);
    let hash = out["provenance"]["content_hash"].as_str().unwrap();
    assert!(hash.starts_with("sha256:") && hash.len() == 7 + 64, "{hash}");

    let ev = &out["evidence"]["price"];
    assert_eq!(ev["value"], "999 $");
    assert_eq!(ev["source"]["method"], "http_dom");
    assert_eq!(ev["source"]["selector"], ".price");
    assert_eq!(ev["source"]["content_hash"], hash);
    assert!(out["evidence"].get("missing").is_none(), "no evidence for absent values");

    let off = run_open(&engine, json!({ "url": "https://shop.test/p", "structured": false })).await;
    assert!(off.context.nodes.get("read").unwrap().get("structured").is_none());
}

#[tokio::test]
async fn access_problems_are_classified() {
    let engine = status_engine();
    let result = run_open(
        &engine,
        json!({
            "url": "https://forbidden.test/\nhttps://ratelimit-long.test/\nhttps://challenge.test/\nhttps://missing.test/\nhttps://shop.test/p",
        }),
    )
    .await;
    assert!(result.error.is_none(), "{:?}", result.error);
    let pages = result.context.nodes.get("read").unwrap()["pages"].as_array().unwrap().clone();
    let by_url = |u: &str| pages.iter().find(|p| p["source_url"] == u).unwrap().clone();

    let blocked = by_url("https://forbidden.test/");
    assert_eq!(blocked["status"], "blocked");
    assert_eq!(blocked["http_status"], 403);
    assert_eq!(blocked["retryable"], false);

    let limited = by_url("https://ratelimit-long.test/");
    assert_eq!(limited["status"], "rate_limited");
    assert_eq!(limited["retryable"], true);
    assert_eq!(limited["retry_after"], 3600);

    assert_eq!(by_url("https://challenge.test/")["status"], "challenge");
    assert_eq!(by_url("https://missing.test/")["status"], "not_found");
    assert!(by_url("https://shop.test/p").get("error").is_none());
}

#[tokio::test]
async fn rate_limit_with_short_retry_after_is_retried() {
    let engine = status_engine();
    let result = run_open(&engine, json!({ "url": "https://ratelimit-once.test/" })).await;
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.context.nodes.get("read").unwrap()["title"], "iPhone 17");
}

#[tokio::test]
async fn network_capture_yields_api_candidates_with_refetch() {
    let (engine, browser, _) = browser_engine();
    let result = run_open(
        &engine,
        json!({
            "url": "https://spa.test/",
            "capture_network": true,
            "network_idle_ms": 500,
            "fetch_api": true,
        }),
    )
    .await;
    assert!(result.error.is_none(), "{:?}", result.error);
    let out = result.context.nodes.get("read").unwrap();
    assert_eq!(out["render"], "browser", "network capture needs the browser");
    assert_eq!(out["network"]["fetch_requests"], 2);

    let c = out["api_candidates"].as_array().unwrap();
    assert_eq!(c[0]["url"], "https://spa.test/api/products");
    assert_eq!(c[0]["json"]["products"][0]["price"], 99);
    assert_eq!(c[0]["source"]["method"], "network_json");
    assert_eq!(c[1]["url"], "https://spa.test/api/config");
    assert_eq!(c[1]["json"]["currency"], "USD", "uncaptured GET endpoint was refetched");
    assert_eq!(c[1]["source"]["method"], "api_refetch");

    let call = browser.calls.lock().unwrap()[0].clone();
    assert!(call.capture_network);
    assert_eq!(call.network_idle_ms, Some(500));
}

struct FakeOpenAiConnection {
    base_url: String,
}

#[async_trait]
impl crate::connections::ConnectionProvider for FakeOpenAiConnection {
    async fn resolve(&self, _id: &str) -> Result<crate::connections::ResolvedConnection, String> {
        Ok(crate::connections::ResolvedConnection {
            id: Uuid::now_v7(),
            name: "mock-openai".into(),
            connection_type: "openai".into(),
            config: json!({ "base_url": self.base_url, "default_model": "mock-model" }),
            credentials: json!({ "api_key": "test-key" }),
        })
    }

    async fn get_public(&self, _id: &str) -> Result<Option<boarddo_shared::Connection>, String> {
        Ok(None)
    }
}

/// OpenAI-compatible `/chat/completions` that answers `reply` and records prompts.
async fn mock_openai(reply: serde_json::Value) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = prompts.clone();
    let content = reply.to_string();
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let content = content.clone();
            let seen = seen.clone();
            async move {
                let user = body["messages"]
                    .as_array()
                    .and_then(|m| m.last())
                    .and_then(|m| m["content"].as_str())
                    .unwrap_or_default()
                    .to_string();
                seen.lock().unwrap().push(user);
                axum::Json(json!({
                    "model": "mock-model",
                    "choices": [{ "message": { "role": "assistant", "content": content } }],
                    "usage": { "total_tokens": 42 }
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), prompts)
}

#[tokio::test]
async fn extract_verifies_llm_claims_against_sources() {
    let (base_url, prompts) = mock_openai(json!({
        "fields": {
            "name": { "value": "iPhone 17", "source": "S2", "path": "$[\"og:title\"]" },
            "price": { "value": "999", "source": "S1", "path": "$.offers.price" },
            "availability": { "value": "in stock", "source": "S3", "quote": "Available: in stock" }
        }
    }))
    .await;
    let gw = FakePagesGateway {
        pages: HashMap::from([("https://shop.test/p".to_string(), PRODUCT_PAGE.to_string())]),
        ..Default::default()
    };
    let engine = Engine::with_connections(Arc::new(FakeOpenAiConnection { base_url }))
        .with_web(Arc::new(gw))
        .with_browser(Arc::new(NullBrowserRenderer));
    let definition = WorkflowDefinition {
        nodes: vec![
            node("manual", type_ids::TRIGGER_MANUAL, json!({})),
            node(
                "extract",
                type_ids::WEB_EXTRACT,
                json!({
                    "connection_id": "mock",
                    "url": "https://shop.test/p",
                    "instruction": "Название товара, цена и наличие",
                    "schema": "name: название\nprice: цена\navailability: наличие",
                }),
            ),
        ],
        edges: vec![edge("e1", "manual", "extract", None)],
    };
    let result = engine
        .execute(Uuid::now_v7(), Uuid::now_v7(), 1, &definition, json!({}), None)
        .await;
    assert!(result.error.is_none(), "{:?}", result.error);
    let out = result.context.nodes.get("extract").unwrap();

    assert_eq!(out["data"]["name"], "iPhone 17");
    assert_eq!(out["data"]["price"], "999");
    assert_eq!(out["evidence"]["price"]["verified"], true);
    assert_eq!(out["evidence"]["price"]["source"]["method"], "json_ld");
    assert_eq!(out["evidence"]["name"]["source"]["method"], "meta");
    assert_eq!(out["evidence"]["availability"]["verified"], false, "quote is not on the page");
    assert_eq!(out["unverified"], json!(["availability"]));
    assert!(
        out["evidence"]["price"]["source"]["content_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );

    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("Task: Название товара, цена и наличие"), "{prompt}");
    assert!(prompt.contains("[S1] kind=json_ld url=https://shop.test/p"), "{prompt}");
    assert!(prompt.contains("[S3] kind=http_dom"), "{prompt}");
}


#[tokio::test]
async fn long_page_mentioning_captcha_is_not_a_challenge() {
    let engine = status_engine();
    // max_chars clips the text; the page itself is long, so this is a normal article.
    let result = run_open(&engine, json!({ "url": "https://article.test/", "max_chars": 200 })).await;
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.context.nodes.get("read").unwrap()["title"], "News");
}
