//! Phase 2 E2E: Webhook path → HTTP → Transform → Condition → Telegram
//! with connection credentials and secret-leak assertions.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::extract::Json;
    use axum::routing::{get, post};
    use boarddo_shared::{
        CreateConnectionRequest, Edge, Node, Position, WorkflowDefinition, type_ids,
    };
    use serde_json::{Value, json};
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::net::TcpListener;
    use uuid::Uuid;

    use crate::connections::{ConnectionProvider, StorageConnectionProvider};
    use crate::engine::Engine;
    use crate::secrets::EncryptedSqliteSecretStore;
    use crate::storage::Storage;

    const LEAK_TOKEN: &str = "TELEGRAM_BOT_TOKEN_SHOULD_NEVER_LEAK_999";

    fn node(id: &str, type_id: &str, config: Value) -> Node {
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

    fn assert_no_token_leak(label: &str, value: &Value) {
        let s = value.to_string();
        assert!(!s.contains(LEAK_TOKEN), "{label} leaked bot token: {s}");
    }

    async fn setup_provider() -> Arc<StorageConnectionProvider> {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE connections (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                type TEXT NOT NULL,
                config TEXT NOT NULL DEFAULT '{}',
                secret_ref TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let secrets = Arc::new(EncryptedSqliteSecretStore::new(pool.clone(), [3u8; 32]));
        secrets.migrate().await.unwrap();
        let storage = Storage::from_pool(pool);
        Arc::new(StorageConnectionProvider::new(storage, secrets))
    }

    #[tokio::test]
    async fn e2e_webhook_http_condition_telegram_no_secret_leak() {
        let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        // Mock upstream HTTP API
        let http_app = Router::new().route(
            "/sales",
            get(|| async { Json(json!({ "amount": 150, "currency": "USD" })) }),
        );
        let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = http_listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(http_listener, http_app).await.unwrap();
        });

        // Mock Telegram Bot API: /bot{token}/sendMessage and getMe
        let tg_app = Router::new()
            .route(
                "/bot{token}/sendMessage",
                post({
                    let captured = captured_clone.clone();
                    move |axum::extract::Path(token): axum::extract::Path<String>,
                          Json(body): Json<Value>| {
                        let captured = captured.clone();
                        async move {
                            assert_eq!(token, LEAK_TOKEN);
                            captured.lock().unwrap().push(body.clone());
                            Json(json!({
                                "ok": true,
                                "result": {
                                    "message_id": 42,
                                    "chat": { "id": body["chat_id"] },
                                    "text": body["text"]
                                }
                            }))
                        }
                    }
                }),
            )
            .route(
                "/bot{token}/getMe",
                post(
                    |axum::extract::Path(token): axum::extract::Path<String>| async move {
                        assert_eq!(token, LEAK_TOKEN);
                        Json(json!({
                            "ok": true,
                            "result": { "id": 1, "is_bot": true, "username": "boarddo_bot" }
                        }))
                    },
                ),
            );
        let tg_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tg_addr = tg_listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(tg_listener, tg_app).await.unwrap();
        });

        let provider = setup_provider().await;
        let tg_conn = provider
            .create(CreateConnectionRequest {
                name: "Production Telegram".into(),
                connection_type: "telegram".into(),
                config: json!({ "api_base": format!("http://{tg_addr}") }),
                secret: Some(json!({ "bot_token": LEAK_TOKEN })),
                enabled: true,
            })
            .await
            .unwrap();

        // Public connection must not contain token
        let public = serde_json::to_value(&tg_conn).unwrap();
        assert_no_token_leak("connection API", &public);
        assert!(public.get("secret").is_none());

        let http_conn = provider
            .create(CreateConnectionRequest {
                name: "Sales API".into(),
                connection_type: "http".into(),
                config: json!({ "base_url": format!("http://{http_addr}") }),
                secret: None,
                enabled: true,
            })
            .await
            .unwrap();

        let definition = WorkflowDefinition {
            nodes: vec![
                node("hook", type_ids::TRIGGER_WEBHOOK, json!({})),
                node(
                    "http1",
                    type_ids::HTTP_REQUEST,
                    json!({
                        "connection_id": http_conn.id.to_string(),
                        "method": "GET",
                        "url": "/sales"
                    }),
                ),
                node(
                    "tr1",
                    type_ids::DATA_TRANSFORM,
                    json!({
                        "mapping": {
                            "amount": "{{nodes.http1.body.amount}}",
                            "chat_id": "{{trigger.chat_id}}",
                            "message": "Alert: sales={{nodes.http1.body.amount}}"
                        }
                    }),
                ),
                node(
                    "c1",
                    type_ids::LOGIC_CONDITION,
                    json!({ "expression": "{{amount > 100}}" }),
                ),
                node(
                    "tg1",
                    type_ids::TELEGRAM_SEND_MESSAGE,
                    json!({
                        "connection_id": tg_conn.id.to_string(),
                        "chat_id": "{{chat_id}}",
                        "text": "{{message}}"
                    }),
                ),
                node(
                    "log_false",
                    type_ids::DEBUG_LOG,
                    json!({ "message": "below threshold" }),
                ),
            ],
            edges: vec![
                edge("e1", "hook", "http1", None),
                edge("e2", "http1", "tr1", None),
                edge("e3", "tr1", "c1", None),
                edge("e4", "c1", "tg1", Some("true")),
                edge("e5", "c1", "log_false", Some("false")),
            ],
        };

        // Workflow JSON must not embed the token
        let wf_json = serde_json::to_value(&definition).unwrap();
        assert_no_token_leak("workflow definition", &wf_json);

        let engine = Engine::with_connections(provider.clone());
        let result = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({ "chat_id": "12345", "amount": 150 }),
                Some("webhook"),
                None,
            )
            .await;

        assert!(
            result.error.is_none(),
            "execution failed: {:?}",
            result.error
        );
        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);

        let ran: Vec<_> = result
            .node_executions
            .iter()
            .map(|n| n.node_id.as_str())
            .collect();
        assert!(ran.contains(&"tg1"));
        assert!(!ran.contains(&"log_false"));

        // Telegram mock received the message
        let msgs = captured.lock().unwrap().clone();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["chat_id"], "12345");
        assert!(msgs[0]["text"].as_str().unwrap().contains("150"));

        // No token in node I/O (debugger foundation)
        for ne in &result.node_executions {
            assert_no_token_leak(&format!("node {} input", ne.node_id), &ne.input);
            if let Some(out) = &ne.output {
                assert_no_token_leak(&format!("node {} output", ne.node_id), out);
            }
            if let Some(err) = &ne.error {
                assert!(
                    !err.contains(LEAK_TOKEN),
                    "node {} error leaked token",
                    ne.node_id
                );
            }
        }

        // Context snapshot should be clean
        assert_no_token_leak("context snapshot", &result.context.snapshot());

        // ResolvedConnection Debug redacts
        let resolved = provider.resolve(&tg_conn.id.to_string()).await.unwrap();
        let dbg = format!("{resolved:?}");
        assert!(!dbg.contains(LEAK_TOKEN));
        assert!(dbg.contains("REDACTED"));
    }

    #[tokio::test]
    async fn telegram_condition_false_goes_to_log() {
        let provider = setup_provider().await;
        // Minimal telegram connection not needed if we take false branch
        let definition = WorkflowDefinition {
            nodes: vec![
                node("hook", type_ids::TRIGGER_WEBHOOK, json!({})),
                node(
                    "set1",
                    type_ids::DATA_SET,
                    json!({ "name": "amount", "value": "{{trigger.amount}}" }),
                ),
                node(
                    "c1",
                    type_ids::LOGIC_CONDITION,
                    json!({ "expression": "{{amount > 100}}" }),
                ),
                node(
                    "log_false",
                    type_ids::DEBUG_LOG,
                    json!({ "message": "low" }),
                ),
            ],
            edges: vec![
                edge("e1", "hook", "set1", None),
                edge("e2", "set1", "c1", None),
                edge("e3", "c1", "log_false", Some("false")),
            ],
        };

        let engine = Engine::with_connections(provider);
        let result = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({ "amount": 50 }),
                Some("webhook"),
                None,
            )
            .await;

        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);
        assert!(
            result
                .node_executions
                .iter()
                .any(|n| n.node_id == "log_false")
        );
    }
}
