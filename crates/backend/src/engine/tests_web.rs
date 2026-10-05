//! Classify → condition → web.search branch (no live LLM).

use std::sync::Arc;

use boarddo_shared::v2::WebSearchHit;
use boarddo_shared::{Edge, Node, Position, WorkflowDefinition, type_ids};
use serde_json::json;
use uuid::Uuid;

use crate::engine::Engine;
use crate::integrations::web::{HttpWebGateway, StaticSearchProvider};

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
                json!({ "query": "{{nodes.classify.output.query}}", "limit": 5 }),
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
