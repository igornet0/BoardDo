//! Scenario suites: simple linear flows and multi-step action pipelines.
//!
//! These tests exercise how BoardDo builds and runs action scenarios
//! (workflow graphs of trigger → data → logic → action nodes).

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::extract::Json;
    use axum::routing::{get, post};
    use boarddo_shared::{Edge, Node, Position, WorkflowDefinition, type_ids};
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use uuid::Uuid;

    use crate::engine::Engine;

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

    fn ran_ids(result: &crate::engine::ExecutionResult) -> Vec<&str> {
        result
            .node_executions
            .iter()
            .map(|n| n.node_id.as_str())
            .collect()
    }

    fn log_message(result: &crate::engine::ExecutionResult, node_id: &str) -> String {
        result
            .node_executions
            .iter()
            .find(|n| n.node_id == node_id)
            .and_then(|n| n.output.as_ref())
            .and_then(|o| o.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }

    // ─── Simple scenarios ───────────────────────────────────────────────

    /// S1: Manual trigger → set greeting → log.
    #[tokio::test]
    async fn simple_set_and_log() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("start", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "greet",
                    type_ids::DATA_SET,
                    json!({"name": "greeting", "value": "Hello BoardDo"}),
                ),
                node(
                    "out",
                    type_ids::DEBUG_LOG,
                    json!({"message": "{{greeting}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "start", "greet", None),
                edge("e2", "greet", "out", None),
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
        assert_eq!(log_message(&result, "out"), "Hello BoardDo");
        assert_eq!(ran_ids(&result), vec!["start", "greet", "out"]);
    }

    /// S2: Trigger payload → arithmetic transform (tax / discount).
    #[tokio::test]
    async fn simple_arithmetic_transform() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("start", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "calc",
                    type_ids::DATA_TRANSFORM,
                    json!({
                        "mapping": {
                            "subtotal": "{{trigger.price}}",
                            "tax": "{{trigger.price * 0.2}}",
                            "total": "{{trigger.price + trigger.price * 0.2}}"
                        }
                    }),
                ),
                node(
                    "out",
                    type_ids::DEBUG_LOG,
                    json!({"message": "total={{total}} tax={{tax}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "start", "calc", None),
                edge("e2", "calc", "out", None),
            ],
        };

        let result = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"price": 100}),
                None,
            )
            .await;

        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);
        let calc = result
            .node_executions
            .iter()
            .find(|n| n.node_id == "calc")
            .unwrap()
            .output
            .as_ref()
            .unwrap();
        assert_eq!(calc["subtotal"], 100);
        assert_eq!(calc["tax"], 20.0);
        assert_eq!(calc["total"], 120.0);
        assert!(log_message(&result, "out").contains("total=120"));
    }

    /// S3: String `contains` condition on order status.
    #[tokio::test]
    async fn simple_contains_condition() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("start", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "status",
                    type_ids::DATA_SET,
                    json!({"name": "status", "value": "{{trigger.status}}"}),
                ),
                node(
                    "check",
                    type_ids::LOGIC_CONDITION,
                    json!({
                        "left": "{{status}}",
                        "operator": "contains",
                        "right": "paid"
                    }),
                ),
                node("ok", type_ids::DEBUG_LOG, json!({"message": "paid-ok"})),
                node("skip", type_ids::DEBUG_LOG, json!({"message": "unpaid"})),
            ],
            edges: vec![
                edge("e1", "start", "status", None),
                edge("e2", "status", "check", None),
                edge("e3", "check", "ok", Some("true")),
                edge("e4", "check", "skip", Some("false")),
            ],
        };

        let result = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"status": "order-paid-v2"}),
                None,
            )
            .await;

        let ran = ran_ids(&result);
        assert!(ran.contains(&"ok"));
        assert!(!ran.contains(&"skip"));
    }

    /// S4: Short delay then continue (async wait in pipeline).
    #[tokio::test]
    async fn simple_delay_pipeline() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("start", type_ids::TRIGGER_MANUAL, json!({})),
                node("wait", type_ids::LOGIC_DELAY, json!({"ms": 25})),
                node(
                    "done",
                    type_ids::DEBUG_LOG,
                    json!({"message": "after-delay"}),
                ),
            ],
            edges: vec![
                edge("e1", "start", "wait", None),
                edge("e2", "wait", "done", None),
            ],
        };

        let started = std::time::Instant::now();
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
        let elapsed = started.elapsed();

        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);
        assert!(elapsed.as_millis() >= 20, "delay too short: {elapsed:?}");
        assert_eq!(log_message(&result, "done"), "after-delay");
    }

    // ─── Complex scenarios ──────────────────────────────────────────────

    /// C1: Order intake — webhook → normalize → tier condition → branch logs.
    ///
    /// ```text
    /// webhook → set amount → transform (tier fields)
    ///        → condition amount >= 500
    ///            true  → vip log
    ///            false → standard log → delay → followup log
    /// ```
    #[tokio::test]
    async fn complex_order_tier_branching() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("hook", type_ids::TRIGGER_WEBHOOK, json!({})),
                node(
                    "amount",
                    type_ids::DATA_SET,
                    json!({"name": "amount", "value": "{{trigger.amount}}"}),
                ),
                node(
                    "norm",
                    type_ids::DATA_TRANSFORM,
                    json!({
                        "mapping": {
                            "customer": "{{trigger.customer}}",
                            "currency": "{{trigger.currency}}",
                            "label": "Order {{trigger.order_id}} for {{trigger.customer}}"
                        }
                    }),
                ),
                node(
                    "tier",
                    type_ids::LOGIC_CONDITION,
                    json!({"expression": "{{amount >= 500}}"}),
                ),
                node(
                    "vip",
                    type_ids::DEBUG_LOG,
                    json!({"message": "VIP: {{label}} total={{amount}} {{currency}}"}),
                ),
                node(
                    "std",
                    type_ids::DEBUG_LOG,
                    json!({"message": "STD: {{label}}"}),
                ),
                node("pause", type_ids::LOGIC_DELAY, json!({"ms": 10})),
                node(
                    "followup",
                    type_ids::DEBUG_LOG,
                    json!({"message": "followup-{{customer}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "hook", "amount", None),
                edge("e2", "amount", "norm", None),
                edge("e3", "norm", "tier", None),
                edge("e4", "tier", "vip", Some("true")),
                edge("e5", "tier", "std", Some("false")),
                edge("e6", "std", "pause", None),
                edge("e7", "pause", "followup", None),
            ],
        };

        // High-value path
        let vip = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({
                    "order_id": "A-100",
                    "customer": "Igor",
                    "amount": 750,
                    "currency": "USD"
                }),
                Some("webhook"),
                None,
            )
            .await;
        assert_eq!(vip.status, boarddo_shared::ExecutionStatus::Completed);
        let vip_ran = ran_ids(&vip);
        assert!(vip_ran.contains(&"vip"));
        assert!(!vip_ran.contains(&"std"));
        assert!(!vip_ran.contains(&"followup"));
        assert!(log_message(&vip, "vip").contains("VIP:"));
        assert!(log_message(&vip, "vip").contains("750"));

        // Standard path with follow-up delay
        let std = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({
                    "order_id": "B-200",
                    "customer": "Anna",
                    "amount": 120,
                    "currency": "EUR"
                }),
                Some("webhook"),
                None,
            )
            .await;
        assert_eq!(std.status, boarddo_shared::ExecutionStatus::Completed);
        let std_ran = ran_ids(&std);
        assert!(std_ran.contains(&"std"));
        assert!(std_ran.contains(&"followup"));
        assert!(!std_ran.contains(&"vip"));
        assert_eq!(log_message(&std, "followup"), "followup-Anna");
    }

    /// C2: Nested conditions — bronze / silver / gold loyalty tiers.
    ///
    /// ```text
    /// manual → set points
    ///       → gold? (>=1000)
    ///           true  → gold log
    ///           false → silver? (>=500)
    ///               true  → silver log
    ///               false → bronze log
    /// ```
    #[tokio::test]
    async fn complex_nested_loyalty_tiers() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("start", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "pts",
                    type_ids::DATA_SET,
                    json!({"name": "points", "value": "{{trigger.points}}"}),
                ),
                node(
                    "is_gold",
                    type_ids::LOGIC_CONDITION,
                    json!({"left": "{{points}}", "operator": ">=", "right": 1000}),
                ),
                node(
                    "is_silver",
                    type_ids::LOGIC_CONDITION,
                    json!({"left": "{{points}}", "operator": ">=", "right": 500}),
                ),
                node("gold", type_ids::DEBUG_LOG, json!({"message": "tier=gold"})),
                node(
                    "silver",
                    type_ids::DEBUG_LOG,
                    json!({"message": "tier=silver"}),
                ),
                node(
                    "bronze",
                    type_ids::DEBUG_LOG,
                    json!({"message": "tier=bronze"}),
                ),
            ],
            edges: vec![
                edge("e1", "start", "pts", None),
                edge("e2", "pts", "is_gold", None),
                edge("e3", "is_gold", "gold", Some("true")),
                edge("e4", "is_gold", "is_silver", Some("false")),
                edge("e5", "is_silver", "silver", Some("true")),
                edge("e6", "is_silver", "bronze", Some("false")),
            ],
        };

        async fn run(engine: &Engine, def: &WorkflowDefinition, points: i64) -> String {
            let result = engine
                .execute(
                    Uuid::now_v7(),
                    Uuid::now_v7(),
                    1,
                    def,
                    json!({"points": points}),
                    None,
                )
                .await;
            assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);
            let ran = ran_ids(&result);
            if ran.contains(&"gold") {
                "gold".into()
            } else if ran.contains(&"silver") {
                "silver".into()
            } else if ran.contains(&"bronze") {
                "bronze".into()
            } else {
                "none".into()
            }
        }

        assert_eq!(run(&engine, &definition, 1500).await, "gold");
        assert_eq!(run(&engine, &definition, 750).await, "silver");
        assert_eq!(run(&engine, &definition, 100).await, "bronze");
    }

    /// C3: Fan-out — one set feeds two independent action branches.
    #[tokio::test]
    async fn complex_fan_out_parallel_branches() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("start", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "payload",
                    type_ids::DATA_SET,
                    json!({"name": "event", "value": "shipment.created"}),
                ),
                node(
                    "notify_ops",
                    type_ids::DEBUG_LOG,
                    json!({"message": "ops:{{event}}"}),
                ),
                node(
                    "notify_audit",
                    type_ids::DEBUG_LOG,
                    json!({"message": "audit:{{event}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "start", "payload", None),
                edge("e2", "payload", "notify_ops", None),
                edge("e3", "payload", "notify_audit", None),
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
        let ran = ran_ids(&result);
        assert!(ran.contains(&"notify_ops"));
        assert!(ran.contains(&"notify_audit"));
        assert_eq!(log_message(&result, "notify_ops"), "ops:shipment.created");
        assert_eq!(
            log_message(&result, "notify_audit"),
            "audit:shipment.created"
        );
    }

    /// C4: HTTP orchestration — fetch catalog → enrich → POST order → confirm.
    ///
    /// ```text
    /// manual → GET /catalog/:sku → transform line item
    ///       → condition stock > 0
    ///           true  → POST /orders → log confirmation
    ///           false → log out-of-stock
    /// ```
    #[tokio::test]
    async fn complex_http_order_orchestration() {
        let orders: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let orders_clone = orders.clone();

        let catalog = json!({
            "sku": "BOARD-1",
            "title": "BoardDo Starter",
            "price": 49,
            "stock": 12
        });

        let app = Router::new()
            .route(
                "/catalog/{sku}",
                get({
                    let catalog = catalog.clone();
                    move |axum::extract::Path(sku): axum::extract::Path<String>| {
                        let catalog = catalog.clone();
                        async move {
                            assert_eq!(sku, "BOARD-1");
                            Json(catalog)
                        }
                    }
                }),
            )
            .route(
                "/orders",
                post({
                    let orders = orders_clone;
                    move |Json(body): Json<Value>| {
                        let orders = orders.clone();
                        async move {
                            orders.lock().unwrap().push(body.clone());
                            Json(json!({
                                "ok": true,
                                "order_id": "ORD-9",
                                "echo": body
                            }))
                        }
                    }
                }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{addr}");

        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("start", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "catalog",
                    type_ids::HTTP_REQUEST,
                    json!({
                        "method": "GET",
                        "url": format!("{base}/catalog/{{{{trigger.sku}}}}"),
                        "timeout_ms": 3000
                    }),
                ),
                node(
                    "line",
                    type_ids::DATA_TRANSFORM,
                    json!({
                        "mapping": {
                            "sku": "{{nodes.catalog.body.sku}}",
                            "title": "{{nodes.catalog.body.title}}",
                            "unit_price": "{{nodes.catalog.body.price}}",
                            "qty": "{{trigger.qty}}",
                            "line_total": "{{nodes.catalog.body.price * trigger.qty}}",
                            "stock": "{{nodes.catalog.body.stock}}"
                        }
                    }),
                ),
                node(
                    "in_stock",
                    type_ids::LOGIC_CONDITION,
                    json!({"expression": "{{stock > 0}}"}),
                ),
                node(
                    "place",
                    type_ids::HTTP_REQUEST,
                    json!({
                        "method": "POST",
                        "url": format!("{base}/orders"),
                        "body": {
                            "sku": "{{sku}}",
                            "qty": "{{qty}}",
                            "total": "{{line_total}}",
                            "buyer": "{{trigger.buyer}}"
                        },
                        "timeout_ms": 3000
                    }),
                ),
                node(
                    "confirm",
                    type_ids::DEBUG_LOG,
                    json!({
                        "message": "placed {{nodes.place.body.order_id}} total={{line_total}}"
                    }),
                ),
                node(
                    "oos",
                    type_ids::DEBUG_LOG,
                    json!({"message": "out-of-stock:{{sku}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "start", "catalog", None),
                edge("e2", "catalog", "line", None),
                edge("e3", "line", "in_stock", None),
                edge("e4", "in_stock", "place", Some("true")),
                edge("e5", "in_stock", "oos", Some("false")),
                edge("e6", "place", "confirm", None),
            ],
        };

        let result = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"sku": "BOARD-1", "qty": 2, "buyer": "igor"}),
                None,
            )
            .await;

        assert!(
            result.error.is_none(),
            "orchestration failed: {:?}",
            result.error
        );
        assert_eq!(result.status, boarddo_shared::ExecutionStatus::Completed);

        let ran = ran_ids(&result);
        assert!(ran.contains(&"place"));
        assert!(ran.contains(&"confirm"));
        assert!(!ran.contains(&"oos"));

        let posted = orders.lock().unwrap().clone();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0]["sku"], "BOARD-1");
        assert_eq!(posted[0]["qty"], 2);
        assert_eq!(posted[0]["total"], 98.0);
        assert_eq!(posted[0]["buyer"], "igor");

        let msg = log_message(&result, "confirm");
        assert!(msg.contains("ORD-9"), "{msg}");
        assert!(msg.contains("98"), "{msg}");
    }

    /// C5: Multi-source aware workflow — same graph, different trigger entry.
    #[tokio::test]
    async fn complex_multi_trigger_routing() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("manual", type_ids::TRIGGER_MANUAL, json!({})),
                node("hook", type_ids::TRIGGER_WEBHOOK, json!({})),
                node(
                    "sched",
                    type_ids::TRIGGER_SCHEDULE,
                    json!({"every": {"minutes": 15}}),
                ),
                node(
                    "from_manual",
                    type_ids::DATA_TRANSFORM,
                    json!({"mapping": {"channel": "manual", "note": "{{trigger.note}}"}}),
                ),
                node(
                    "from_hook",
                    type_ids::DATA_TRANSFORM,
                    json!({"mapping": {"channel": "webhook", "note": "{{trigger.note}}"}}),
                ),
                node(
                    "from_sched",
                    type_ids::DATA_TRANSFORM,
                    json!({"mapping": {"channel": "schedule", "note": "tick"}}),
                ),
                node(
                    "out",
                    type_ids::DEBUG_LOG,
                    json!({"message": "{{channel}}:{{note}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "manual", "from_manual", None),
                edge("e2", "hook", "from_hook", None),
                edge("e3", "sched", "from_sched", None),
                edge("e4", "from_manual", "out", None),
                edge("e5", "from_hook", "out", None),
                edge("e6", "from_sched", "out", None),
            ],
        };

        let manual = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"note": "ui-run"}),
                Some("manual"),
                None,
            )
            .await;
        assert_eq!(log_message(&manual, "out"), "manual:ui-run");
        assert!(!ran_ids(&manual).contains(&"hook"));

        let hook = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"note": "api-hit"}),
                Some("webhook"),
                None,
            )
            .await;
        assert_eq!(log_message(&hook, "out"), "webhook:api-hit");
        assert!(!ran_ids(&hook).contains(&"manual"));

        let sched = engine
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
        assert_eq!(log_message(&sched, "out"), "schedule:tick");
        assert!(!ran_ids(&sched).contains(&"manual"));
    }

    /// C6: Expression pipeline — discount eligibility + computed savings.
    #[tokio::test]
    async fn complex_discount_eligibility_pipeline() {
        let engine = Engine::new();
        let definition = WorkflowDefinition {
            nodes: vec![
                node("start", type_ids::TRIGGER_MANUAL, json!({})),
                node(
                    "vars",
                    type_ids::DATA_TRANSFORM,
                    json!({
                        "mapping": {
                            "cart": "{{trigger.cart_total}}",
                            "member": "{{trigger.is_member}}",
                            "discount_rate": "{{trigger.is_member}}"
                        }
                    }),
                ),
                node(
                    "eligible",
                    type_ids::LOGIC_CONDITION,
                    json!({
                        "expression": "{{cart > 200}}"
                    }),
                ),
                node(
                    "apply",
                    type_ids::DATA_TRANSFORM,
                    json!({
                        "mapping": {
                            "savings": "{{cart * 0.15}}",
                            "payable": "{{cart - cart * 0.15}}",
                            "deal": "SAVE15"
                        }
                    }),
                ),
                node(
                    "no_deal",
                    type_ids::DEBUG_LOG,
                    json!({"message": "no-discount cart={{cart}}"}),
                ),
                node(
                    "deal",
                    type_ids::DEBUG_LOG,
                    json!({"message": "{{deal}} pay={{payable}} save={{savings}}"}),
                ),
            ],
            edges: vec![
                edge("e1", "start", "vars", None),
                edge("e2", "vars", "eligible", None),
                edge("e3", "eligible", "apply", Some("true")),
                edge("e4", "eligible", "no_deal", Some("false")),
                edge("e5", "apply", "deal", None),
            ],
        };

        let big = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"cart_total": 400, "is_member": true}),
                None,
            )
            .await;
        assert_eq!(big.status, boarddo_shared::ExecutionStatus::Completed);
        assert!(ran_ids(&big).contains(&"deal"));
        let msg = log_message(&big, "deal");
        assert!(msg.contains("SAVE15"), "{msg}");
        assert!(msg.contains("340"), "{msg}");
        assert!(msg.contains("60"), "{msg}");

        let small = engine
            .execute(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({"cart_total": 80, "is_member": true}),
                None,
            )
            .await;
        assert!(ran_ids(&small).contains(&"no_deal"));
        assert!(!ran_ids(&small).contains(&"deal"));
    }
}
