//! GitHub node / state-store tests (no live GitHub calls).

use std::sync::Arc;

use boarddo_shared::{Node, Position, type_ids};
use serde_json::json;
use uuid::Uuid;

use crate::connections::{ConnectionProvider, ResolvedConnection};
use crate::engine::context::ExecutionContext;
use crate::engine::node_state::{MemoryNodeStateStore, NodeStateStore};
use crate::engine::nodes::GitHubGetLatestRelease;
use crate::engine::registry::{NodeHandler, NodeRegistry};
use crate::engine::validate::validate_workflow;
use boarddo_shared::{Edge, WorkflowDefinition};
use async_trait::async_trait;

struct FakeGithubConnection;

#[async_trait]
impl ConnectionProvider for FakeGithubConnection {
    async fn resolve(&self, connection_id: &str) -> Result<ResolvedConnection, String> {
        Ok(ResolvedConnection {
            id: Uuid::parse_str(connection_id).unwrap_or_else(|_| Uuid::now_v7()),
            name: "gh".into(),
            connection_type: "github".into(),
            config: json!({
                "owner": "acme",
                "repo": "demo",
                "api_base": "http://127.0.0.1:9",
            }),
            credentials: json!({}),
        })
    }

    async fn get_public(
        &self,
        _connection_id: &str,
    ) -> Result<Option<boarddo_shared::Connection>, String> {
        Ok(None)
    }
}

#[tokio::test]
async fn memory_state_store_roundtrip() {
    let store = MemoryNodeStateStore::new();
    assert!(store.get("k").await.unwrap().is_none());
    store.set("k", "v1").await.unwrap();
    assert_eq!(store.get("k").await.unwrap().as_deref(), Some("v1"));
    store.set("k", "v2").await.unwrap();
    assert_eq!(store.get("k").await.unwrap().as_deref(), Some("v2"));
}

#[test]
fn github_node_registered_and_validates_connection_id() {
    let registry = NodeRegistry::with_defaults();
    assert!(registry.contains(type_ids::GITHUB_GET_LATEST_RELEASE));

    let bad = WorkflowDefinition {
        nodes: vec![Node {
            id: "r".into(),
            type_id: type_ids::GITHUB_GET_LATEST_RELEASE.into(),
            category: None,
            position: Position::default(),
            config: json!({}),
        }],
        edges: vec![],
    };
    let errs = validate_workflow(&bad, &registry).unwrap_err();
    assert!(errs.iter().any(|e| e.contains("connection_id")));

    let ok = WorkflowDefinition {
        nodes: vec![
            Node {
                id: "t".into(),
                type_id: type_ids::TRIGGER_MANUAL.into(),
                category: None,
                position: Position::default(),
                config: json!({}),
            },
            Node {
                id: "r".into(),
                type_id: type_ids::GITHUB_GET_LATEST_RELEASE.into(),
                category: None,
                position: Position::default(),
                config: json!({ "connection_id": "00000000-0000-0000-0000-000000000001" }),
            },
        ],
        edges: vec![Edge {
            id: "e".into(),
            source: "t".into(),
            target: "r".into(),
            source_port: None,
            target_port: None,
        }],
    };
    validate_workflow(&ok, &registry).unwrap();
}

#[tokio::test]
async fn get_latest_release_fails_when_api_unreachable() {
    let node = Node {
        id: "release".into(),
        type_id: type_ids::GITHUB_GET_LATEST_RELEASE.into(),
        category: None,
        position: Position::default(),
        config: json!({
            "connection_id": "00000000-0000-0000-0000-000000000001",
            "track_new": true
        }),
    };
    let mut ctx = ExecutionContext::new(Uuid::now_v7(), Uuid::now_v7(), 1, json!({}))
        .with_connections(Arc::new(FakeGithubConnection))
        .with_node_state(Arc::new(MemoryNodeStateStore::new()));

    let err = GitHubGetLatestRelease
        .execute(&node, &mut ctx)
        .await
        .unwrap_err();
    assert!(err.contains("github:"), "{err}");
}
