#[cfg(test)]
mod tests {
    use boarddo_shared::{Edge, Node, Position, WorkflowDefinition, type_ids};
    use serde_json::json;
    use uuid::Uuid;

    use crate::engine::{Engine, ExecutionContext};

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

    #[tokio::test]
    async fn test_condition_true() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("t1", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "s1",
                    type_ids::DATA_SET,
                    json!({"name": "amount", "value": 150}),
                ),
                node(
                    "c1",
                    type_ids::LOGIC_CONDITION,
                    json!({"left": "{{amount}}", "operator": ">", "right": 100}),
                ),
                node("log_true", type_ids::DEBUG_LOG, json!({"message": "yes"})),
                node("log_false", type_ids::DEBUG_LOG, json!({"message": "no"})),
            ],
            edges: vec![
                edge("e1", "t1", "s1", None),
                edge("e2", "s1", "c1", None),
                edge("e3", "c1", "log_true", Some("true")),
                edge("e4", "c1", "log_false", Some("false")),
            ],
        };

        let result = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({}),
                None,
            )
            .await;

        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);

        let ran: Vec<_> = result
            .node_executions
            .iter()
            .map(|n| n.node_id.as_str())
            .collect();
        assert!(ran.contains(&"log_true"));
        assert!(!ran.contains(&"log_false"));
    }

    #[tokio::test]
    async fn test_condition_false() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("t1", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "s1",
                    type_ids::DATA_SET,
                    json!({"name": "amount", "value": 50}),
                ),
                node(
                    "c1",
                    type_ids::LOGIC_CONDITION,
                    json!({"left": "{{amount}}", "operator": ">", "right": 100}),
                ),
                node("log_true", type_ids::DEBUG_LOG, json!({"message": "yes"})),
                node("log_false", type_ids::DEBUG_LOG, json!({"message": "no"})),
            ],
            edges: vec![
                edge("e1", "t1", "s1", None),
                edge("e2", "s1", "c1", None),
                edge("e3", "c1", "log_true", Some("true")),
                edge("e4", "c1", "log_false", Some("false")),
            ],
        };

        let result = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({}),
                None,
            )
            .await;

        let ran: Vec<_> = result
            .node_executions
            .iter()
            .map(|n| n.node_id.as_str())
            .collect();
        assert!(ran.contains(&"log_false"));
        assert!(!ran.contains(&"log_true"));
    }

    #[tokio::test]
    async fn test_invalid_graph_missing_trigger() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![node("log1", type_ids::DEBUG_LOG, json!({"message": "x"}))],
            edges: vec![],
        };
        let err = engine.validate(&definition).unwrap_err();
        assert!(err.iter().any(|e| e.contains("trigger")));
    }

    #[tokio::test]
    async fn test_missing_node_on_edge() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![node("t1", type_ids::TRIGGER_MANUAL, json!({}))],
            edges: vec![edge("e1", "t1", "ghost", None)],
        };
        let err = engine.validate(&definition).unwrap_err();
        assert!(err.iter().any(|e| e.contains("ghost")));
    }

    #[tokio::test]
    async fn test_execution_failure_missing_param() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("t1", type_ids::TRIGGER_MANUAL, json!({})),
                node("s1", type_ids::DATA_SET, json!({})),
            ],
            edges: vec![edge("e1", "t1", "s1", None)],
        };
        // Validation should catch missing params
        assert!(engine.validate(&definition).is_err());
    }

    #[tokio::test]
    async fn test_context_templates() {
        let mut ctx = ExecutionContext::new(Uuid::now_v7(), Uuid::now_v7(), 1, json!({}));
        ctx.set_variable("name", json!("Igor"));
        assert_eq!(ctx.resolve_template("Hello {{name}}"), json!("Hello Igor"));
        assert_eq!(ctx.resolve_template("{{name}}"), json!("Igor"));
    }

    #[tokio::test]
    async fn test_delay_and_transform() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("t1", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "s1",
                    type_ids::DATA_SET,
                    json!({"name": "name", "value": "Igor"}),
                ),
                node(
                    "tr1",
                    type_ids::DATA_TRANSFORM,
                    json!({"mapping": {"message": "Hello {{name}}"}}),
                ),
                node("d1", type_ids::LOGIC_DELAY, json!({"ms": 10})),
                node(
                    "log1",
                    type_ids::DEBUG_LOG,
                    json!({"message": "{{message}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "t1", "s1", None),
                edge("e2", "s1", "tr1", None),
                edge("e3", "tr1", "d1", None),
                edge("e4", "d1", "log1", None),
            ],
        };

        let result = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({}),
                None,
            )
            .await;
        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);
        let log = result
            .node_executions
            .iter()
            .find(|n| n.node_id == "log1")
            .unwrap();
        assert_eq!(log.output.as_ref().unwrap()["message"], "Hello Igor");
    }

    #[tokio::test]
    async fn test_expression_condition() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("t1", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "s1",
                    type_ids::DATA_SET,
                    json!({"name": "amount", "value": "{{trigger.amount}}"}),
                ),
                node(
                    "c1",
                    type_ids::LOGIC_CONDITION,
                    json!({"expression": "{{amount > 100}}"}),
                ),
                node("log_true", type_ids::DEBUG_LOG, json!({"message": "yes"})),
                node("log_false", type_ids::DEBUG_LOG, json!({"message": "no"})),
            ],
            edges: vec![
                edge("e1", "t1", "s1", None),
                edge("e2", "s1", "c1", None),
                edge("e3", "c1", "log_true", Some("true")),
                edge("e4", "c1", "log_false", Some("false")),
            ],
        };

        let result = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"amount": 150}),
                None,
            )
            .await;
        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);
        let ran: Vec<_> = result
            .node_executions
            .iter()
            .map(|n| n.node_id.as_str())
            .collect();
        assert!(ran.contains(&"log_true"));
        assert!(!ran.contains(&"log_false"));
    }

    #[tokio::test]
    async fn test_http_request_node() {
        use axum::{Json, Router, routing::post};
        use tokio::net::TcpListener;

        let app = Router::new().route(
            "/echo",
            post(|Json(body): Json<serde_json::Value>| async move {
                Json(json!({ "ok": true, "echo": body }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("http://{addr}/echo");
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("t1", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "http1",
                    type_ids::HTTP_REQUEST,
                    json!({
                        "method": "POST",
                        "url": url,
                        "body": { "name": "{{trigger.name}}" },
                        "timeout_ms": 3000
                    }),
                ),
                node(
                    "log1",
                    type_ids::DEBUG_LOG,
                    json!({"message": "{{nodes.http1.body.echo.name}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "t1", "http1", None),
                edge("e2", "http1", "log1", None),
            ],
        };

        let result = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"name": "BoardDo"}),
                None,
            )
            .await;

        assert!(
            result.error.is_none(),
            "http execution failed: {:?}",
            result.error
        );
        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);
        let log = result
            .node_executions
            .iter()
            .find(|n| n.node_id == "log1")
            .unwrap();
        assert_eq!(log.output.as_ref().unwrap()["message"], "BoardDo");
    }

    #[tokio::test]
    async fn test_webhook_trigger_source_filter() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("manual", type_ids::TRIGGER_MANUAL, json!({})),
                node("hook", type_ids::TRIGGER_WEBHOOK, json!({})),
                node(
                    "log_manual",
                    type_ids::DEBUG_LOG,
                    json!({"message": "from-manual"}),
                ),
                node(
                    "log_hook",
                    type_ids::DEBUG_LOG,
                    json!({"message": "from-webhook"}),
                ),
            ],
            edges: vec![
                edge("e1", "manual", "log_manual", None),
                edge("e2", "hook", "log_hook", None),
            ],
        };

        let result = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"x": 1}),
                Some("webhook"),
                None,
            )
            .await;

        let ran: Vec<_> = result
            .node_executions
            .iter()
            .map(|n| n.node_id.as_str())
            .collect();
        assert!(ran.contains(&"hook"));
        assert!(ran.contains(&"log_hook"));
        assert!(!ran.contains(&"manual"));
        assert!(!ran.contains(&"log_manual"));
    }

    #[tokio::test]
    async fn test_schedule_trigger_source_filter() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("manual", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "sched",
                    type_ids::TRIGGER_SCHEDULE,
                    json!({"every": {"minutes": 5}}),
                ),
                node(
                    "log_sched",
                    type_ids::DEBUG_LOG,
                    json!({"message": "from-schedule"}),
                ),
                node(
                    "log_manual",
                    type_ids::DEBUG_LOG,
                    json!({"message": "from-manual"}),
                ),
            ],
            edges: vec![
                edge("e1", "sched", "log_sched", None),
                edge("e2", "manual", "log_manual", None),
            ],
        };

        let result = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({}),
                Some("schedule"),
                None,
            )
            .await;

        let ran: Vec<_> = result
            .node_executions
            .iter()
            .map(|n| n.node_id.as_str())
            .collect();
        assert!(ran.contains(&"sched"));
        assert!(ran.contains(&"log_sched"));
        assert!(!ran.contains(&"manual"));
    }
}
