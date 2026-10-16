use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use boarddo_shared::{ExecutionEvent, ExecutionStatus, Node, NodeExecution, WorkflowDefinition};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use super::context::ExecutionContext;
use super::registry::{NodeOutput, NodeRegistry};
use super::validate::validate_workflow;
use crate::storage::JournalClaim;

/// Callback for live execution events (WebSocket fan-out).
pub type EventSink = Box<dyn Fn(ExecutionEvent) + Send + Sync>;

pub struct ExecutionResult {
    pub status: ExecutionStatus,
    pub error: Option<String>,
    pub node_executions: Vec<NodeExecution>,
    pub context: ExecutionContext,
}

/// Walks the workflow graph and runs nodes via the registry.
pub struct Executor {
    registry: NodeRegistry,
}

impl Executor {
    pub fn new(registry: NodeRegistry) -> Self {
        Self { registry }
    }

    pub async fn run(
        &self,
        definition: &WorkflowDefinition,
        mut ctx: ExecutionContext,
        on_event: Option<&EventSink>,
    ) -> ExecutionResult {
        if let Err(errors) = validate_workflow(definition, &self.registry) {
            let msg = errors.join("; ");
            return ExecutionResult {
                status: ExecutionStatus::Failed,
                error: Some(msg),
                node_executions: vec![],
                context: ctx,
            };
        }

        let nodes: HashMap<&str, &Node> = definition
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n))
            .collect();

        // adjacency: source -> edges
        let mut outgoing: HashMap<&str, Vec<&boarddo_shared::Edge>> = HashMap::new();
        for edge in &definition.edges {
            outgoing.entry(edge.source.as_str()).or_default().push(edge);
        }

        let all_triggers: Vec<&Node> = definition
            .nodes
            .iter()
            .filter(|n| n.type_id.starts_with("trigger."))
            .collect();

        let triggers: Vec<&Node> = match ctx.trigger_source.as_deref() {
            Some("webhook") => {
                let filtered: Vec<_> = all_triggers
                    .iter()
                    .copied()
                    .filter(|n| n.type_id == boarddo_shared::type_ids::TRIGGER_WEBHOOK)
                    .collect();
                if filtered.is_empty() {
                    all_triggers
                } else {
                    filtered
                }
            }
            Some("manual") => {
                let filtered: Vec<_> = all_triggers
                    .iter()
                    .copied()
                    .filter(|n| n.type_id == boarddo_shared::type_ids::TRIGGER_MANUAL)
                    .collect();
                if !filtered.is_empty() {
                    filtered
                } else if all_triggers.iter().all(|n| {
                    n.type_id == boarddo_shared::type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED
                }) {
                    // Telegram-only graphs: Run with `{}` used to fall through and
                    // fail later with opaque `chat_id null` / empty prompt.
                    let has_chat = ctx
                        .trigger
                        .get("chat_id")
                        .and_then(|v| {
                            v.as_i64()
                                .map(|_| ())
                                .or_else(|| v.as_str().filter(|s| !s.is_empty()).map(|_| ()))
                        })
                        .is_some();
                    if !ctx.is_sandbox() && !has_chat {
                        return ExecutionResult {
                            status: ExecutionStatus::Failed,
                            error: Some(
                                "this scenario waits for a Telegram user message — \
                                 write in Telegram (e.g. @ai …), or Run with trigger \
                                 {\"account_id\":\"…\",\"chat_id\":123,\"text\":\"…\"}"
                                    .into(),
                            ),
                            node_executions: vec![],
                            context: ctx,
                        };
                    }
                    all_triggers
                } else {
                    all_triggers
                }
            }
            Some("schedule") => {
                let filtered: Vec<_> = all_triggers
                    .iter()
                    .copied()
                    .filter(|n| n.type_id == boarddo_shared::type_ids::TRIGGER_SCHEDULE)
                    .collect();
                if filtered.is_empty() {
                    all_triggers
                } else {
                    filtered
                }
            }
            Some("telegram.user") => {
                let filtered: Vec<_> = all_triggers
                    .iter()
                    .copied()
                    .filter(|n| {
                        n.type_id
                            == boarddo_shared::type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED
                    })
                    .collect();
                if filtered.is_empty() {
                    all_triggers
                } else {
                    filtered
                }
            }
            _ => all_triggers,
        };

        if triggers.is_empty() {
            return ExecutionResult {
                status: ExecutionStatus::Failed,
                error: Some("no matching trigger node for this run".into()),
                node_executions: vec![],
                context: ctx,
            };
        }

        let mut node_executions = Vec::new();
        let mut degraded_nodes: Vec<String> = Vec::new();
        let mut executed = HashSet::new();
        let mut queue: VecDeque<&Node> = triggers.into_iter().collect();

        emit(
            on_event,
            ExecutionEvent::ExecutionStarted {
                execution_id: ctx.execution_id,
                workflow_id: ctx.workflow_id,
            },
        );

        while let Some(node) = queue.pop_front() {
            if !executed.insert(node.id.clone()) {
                continue;
            }

            emit(
                on_event,
                ExecutionEvent::NodeStarted {
                    execution_id: ctx.execution_id,
                    node_id: node.id.clone(),
                },
            );

            let input = ctx.snapshot();
            let started_at = Utc::now();
            let timer = Instant::now();
            let node_exec_id = Uuid::now_v7();

            let handler = match self.registry.get(&node.type_id) {
                Some(h) => h,
                None => {
                    let err = format!("unknown node type `{}`", node.type_id);
                    let finished_at = Utc::now();
                    let duration_ms = timer.elapsed().as_millis() as u64;
                    node_executions.push(NodeExecution {
                        id: node_exec_id,
                        execution_id: ctx.execution_id,
                        node_id: node.id.clone(),
                        status: ExecutionStatus::Failed,
                        input,
                        output: None,
                        error: Some(err.clone()),
                        started_at,
                        finished_at: Some(finished_at),
                        duration_ms: Some(duration_ms),
                    });
                    emit(
                        on_event,
                        ExecutionEvent::NodeFailed {
                            execution_id: ctx.execution_id,
                            node_id: node.id.clone(),
                            error: err.clone(),
                        },
                    );
                    emit(
                        on_event,
                        ExecutionEvent::ExecutionCompleted {
                            execution_id: ctx.execution_id,
                            status: ExecutionStatus::Failed,
                            error: Some(err.clone()),
                        },
                    );
                    return ExecutionResult {
                        status: ExecutionStatus::Failed,
                        error: Some(err),
                        node_executions,
                        context: ctx,
                    };
                }
            };

            let descriptor = boarddo_shared::v2::lookup_tool(&node.type_id);
            // A handler that returned Ok with a result missing its declared
            // shape has not succeeded: route it down the failure path.
            // Single-execution nodes are claimed in the call journal first: a
            // node that already acted is not run again, and one whose earlier
            // attempt may have acted is refused instead of repeated.
            let guard = match (descriptor, ctx.journal.clone()) {
                (Some(d), Some(journal)) if boarddo_shared::v2::requires_single_execution(d) => {
                    let key = crate::agent_runtime::execution::idempotency_key(
                        ctx.execution_id,
                        &node.type_id,
                        &serde_json::json!({ "node": node.id, "config": node.config }),
                    );
                    Some((journal, key))
                }
                _ => None,
            };
            let settled = match &guard {
                None => None,
                Some((journal, key)) => {
                    match journal.claim(key, ctx.execution_id, &node.type_id).await {
                        Err(e) => Some(Err(format!(
                            "call journal unavailable, `{}` not executed: {e}",
                            node.type_id
                        ))),
                        Ok(JournalClaim::Done(result)) => Some(Ok(NodeOutput::data(result))),
                        Ok(JournalClaim::InFlight) => Some(Err(format!(
                            "`{}` already in progress or its outcome is unknown; not repeating",
                            node.type_id
                        ))),
                        Ok(JournalClaim::FailedUncertain(prev)) => Some(Err(format!(
                            "`{}` failed earlier with an uncertain outcome ({prev}); not repeating",
                            node.type_id
                        ))),
                        Ok(JournalClaim::Fresh | JournalClaim::Reclaimed) => None,
                    }
                }
            };
            let outcome = match settled {
                Some(outcome) => outcome,
                None => {
                    let outcome = handler.execute(node, &mut ctx).await.and_then(|output| {
                        match descriptor {
                            Some(d) => boarddo_shared::v2::verify_result(d, &output.data)
                                .map(|()| output)
                                .map_err(|reason| format!("verification failed: {reason}")),
                            None => Ok(output),
                        }
                    });
                    if let Some((journal, key)) = &guard {
                        let recorded = outcome.as_ref().map(|o| o.data.clone()).map_err(Clone::clone);
                        let retry_safe = match &recorded {
                            Ok(_) => false,
                            Err(e) => boarddo_shared::v2::failure_had_no_effect(
                                boarddo_shared::v2::classify_failure(e),
                            ),
                        };
                        if let Err(e) = journal.finish(key, &recorded, retry_safe).await {
                            // The row stays `started`, so any repeat is refused.
                            tracing::error!(
                                execution_id = %ctx.execution_id,
                                node_id = %node.id,
                                error = %e,
                                "failed to record node call outcome"
                            );
                        }
                    }
                    outcome
                }
            };
            match outcome {
                Ok(output) => {
                    let duration_ms = timer.elapsed().as_millis() as u64;
                    let finished_at = Utc::now();
                    ctx.set_node_output(&node.id, output.data.clone());
                    // The node was told to continue on error and reported a
                    // failure in its output: keep the run going, but record it.
                    let degraded = descriptor
                        .filter(|d| d.verification != boarddo_shared::v2::Verification::None)
                        .and_then(|_| boarddo_shared::v2::reported_failure(&output.data))
                        .map(|detail| format!("degraded: {detail}"));
                    if let Some(warning) = &degraded {
                        tracing::warn!(
                            execution_id = %ctx.execution_id,
                            node_id = %node.id,
                            type_id = %node.type_id,
                            "{warning}"
                        );
                        degraded_nodes.push(format!("node `{}`: {warning}", node.id));
                    }

                    node_executions.push(NodeExecution {
                        id: node_exec_id,
                        execution_id: ctx.execution_id,
                        node_id: node.id.clone(),
                        status: ExecutionStatus::Completed,
                        input,
                        output: Some(output.data.clone()),
                        error: degraded,
                        started_at,
                        finished_at: Some(finished_at),
                        duration_ms: Some(duration_ms),
                    });

                    emit(
                        on_event,
                        ExecutionEvent::NodeCompleted {
                            execution_id: ctx.execution_id,
                            node_id: node.id.clone(),
                            output: output.data.clone(),
                            duration_ms,
                        },
                    );

                    enqueue_next(&mut queue, &outgoing, &nodes, node, &output, &executed);
                }
                Err(err) => {
                    let duration_ms = timer.elapsed().as_millis() as u64;
                    let finished_at = Utc::now();
                    node_executions.push(NodeExecution {
                        id: node_exec_id,
                        execution_id: ctx.execution_id,
                        node_id: node.id.clone(),
                        status: ExecutionStatus::Failed,
                        input,
                        output: None,
                        error: Some(err.clone()),
                        started_at,
                        finished_at: Some(finished_at),
                        duration_ms: Some(duration_ms),
                    });
                    emit(
                        on_event,
                        ExecutionEvent::NodeFailed {
                            execution_id: ctx.execution_id,
                            node_id: node.id.clone(),
                            error: err.clone(),
                        },
                    );
                    emit(
                        on_event,
                        ExecutionEvent::ExecutionCompleted {
                            execution_id: ctx.execution_id,
                            status: ExecutionStatus::Failed,
                            error: Some(err.clone()),
                        },
                    );
                    return ExecutionResult {
                        status: ExecutionStatus::Failed,
                        error: Some(err),
                        node_executions,
                        context: ctx,
                    };
                }
            }
        }

        // Completed, but not clean: consumers must not count it as success.
        let summary = (!degraded_nodes.is_empty()).then(|| degraded_nodes.join("; "));
        emit(
            on_event,
            ExecutionEvent::ExecutionCompleted {
                execution_id: ctx.execution_id,
                status: ExecutionStatus::Completed,
                error: summary.clone(),
            },
        );

        ExecutionResult {
            status: ExecutionStatus::Completed,
            error: summary,
            node_executions,
            context: ctx,
        }
    }
}

fn enqueue_next<'a>(
    queue: &mut VecDeque<&'a Node>,
    outgoing: &HashMap<&str, Vec<&'a boarddo_shared::Edge>>,
    nodes: &HashMap<&str, &'a Node>,
    node: &Node,
    output: &NodeOutput,
    executed: &HashSet<String>,
) {
    let Some(edges) = outgoing.get(node.id.as_str()) else {
        return;
    };

    let follow: Vec<&&boarddo_shared::Edge> = if let Some(ref branch) = output.branch {
        let ported: Vec<_> = edges
            .iter()
            .filter(|e| e.source_port.as_ref() == Some(branch))
            .collect();
        if !ported.is_empty() {
            ported
        } else {
            // Fallback: unported edges when no matching port exists.
            edges.iter().filter(|e| e.source_port.is_none()).collect()
        }
    } else {
        edges.iter().collect()
    };

    for edge in follow {
        if let Some(next) = nodes.get(edge.target.as_str()) {
            if !executed.contains(next.id.as_str()) {
                queue.push_back(next);
            }
        }
    }
}

fn emit(sink: Option<&EventSink>, event: ExecutionEvent) {
    if let Some(cb) = sink {
        cb(event);
    }
}

/// Used by unit tests without full Engine wiring.
#[allow(dead_code)]
pub fn empty_trigger() -> Value {
    Value::Object(Default::default())
}

#[cfg(test)]
mod verification_tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use boarddo_shared::{Edge, Position};
    use serde_json::json;

    use super::*;
    use crate::engine::registry::NodeHandler;

    /// Stands in for a real handler and returns a fixed output.
    struct Fixed(&'static str, Value);

    #[async_trait]
    impl NodeHandler for Fixed {
        fn type_id(&self) -> &'static str {
            self.0
        }
        async fn execute(&self, _: &Node, _: &mut ExecutionContext) -> Result<NodeOutput, String> {
            Ok(NodeOutput::data(self.1.clone()))
        }
    }

    fn node(id: &str, type_id: &str, config: Value) -> Node {
        Node {
            id: id.into(),
            type_id: type_id.into(),
            category: None,
            position: Position::default(),
            config,
        }
    }

    fn edge(source: &str, target: &str) -> Edge {
        Edge {
            id: format!("{source}-{target}"),
            source: source.into(),
            target: target.into(),
            source_port: None,
            target_port: None,
        }
    }

    async fn run(stub: Fixed, action: Node) -> ExecutionResult {
        let ctx = ExecutionContext::new(Uuid::now_v7(), Uuid::now_v7(), 1, empty_trigger());
        run_in(Arc::new(stub), action, ctx).await
    }

    async fn run_in(
        stub: Arc<dyn NodeHandler>,
        action: Node,
        ctx: ExecutionContext,
    ) -> ExecutionResult {
        let mut registry = NodeRegistry::with_defaults();
        registry.register(stub);
        let mut definition: WorkflowDefinition =
            serde_json::from_value(json!({ "nodes": [], "edges": [] })).unwrap();
        definition.nodes = vec![
            node("t", "trigger.manual", json!({})),
            action,
            node("after", "debug.log", json!({ "message": "after" })),
        ];
        definition.edges = vec![edge("t", "a"), edge("a", "after")];
        Executor::new(registry).run(&definition, ctx, None).await
    }

    fn exec<'a>(r: &'a ExecutionResult, id: &str) -> &'a NodeExecution {
        r.node_executions.iter().find(|n| n.node_id == id).unwrap()
    }

    #[tokio::test]
    async fn malformed_result_fails_the_node() {
        let r = run(
            Fixed("http.request", json!({ "body": "" })),
            node("a", "http.request", json!({ "url": "https://example.com" })),
        )
        .await;
        assert_eq!(r.status, ExecutionStatus::Failed);
        let a = exec(&r, "a");
        assert_eq!(a.status, ExecutionStatus::Failed);
        let err = a.error.as_deref().unwrap();
        assert!(err.starts_with("verification failed:") && err.contains("status"), "{err}");
        assert!(r.node_executions.iter().all(|n| n.node_id != "after"));
    }

    #[tokio::test]
    async fn reported_failure_is_recorded_but_run_continues() {
        let r = run(
            Fixed("alertwim.alert", json!({ "delivered": false, "error": "window closed" })),
            node("a", "alertwim.alert", json!({ "message": "hi" })),
        )
        .await;
        assert_eq!(r.status, ExecutionStatus::Completed, "{:?}", r.error);
        assert_eq!(r.error.as_deref(), Some("node `a`: degraded: window closed"));
        let a = exec(&r, "a");
        assert_eq!(a.status, ExecutionStatus::Completed);
        assert_eq!(a.error.as_deref(), Some("degraded: window closed"));
        assert_eq!(exec(&r, "after").status, ExecutionStatus::Completed);
    }

    #[tokio::test]
    async fn valid_result_has_no_error() {
        let r = run(
            Fixed("http.request", json!({ "status": 200, "body": "" })),
            node("a", "http.request", json!({ "url": "https://example.com" })),
        )
        .await;
        assert_eq!(r.status, ExecutionStatus::Completed, "{:?}", r.error);
        assert_eq!(r.error, None);
        assert_eq!(exec(&r, "a").error, None);
    }

    /// Counts calls so tests can see whether the action actually ran.
    struct Counted(&'static str, Value, Arc<std::sync::atomic::AtomicU32>);

    #[async_trait]
    impl NodeHandler for Counted {
        fn type_id(&self) -> &'static str {
            self.0
        }
        async fn execute(&self, _: &Node, _: &mut ExecutionContext) -> Result<NodeOutput, String> {
            self.2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(NodeOutput::data(self.1.clone()))
        }
    }

    async fn journal() -> crate::storage::Storage {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let s = crate::storage::Storage::from_pool(pool);
        s.migrate_tool_journal().await.unwrap();
        s
    }

    fn journaled_ctx(exec_id: Uuid, s: &crate::storage::Storage) -> ExecutionContext {
        ExecutionContext::new(exec_id, Uuid::now_v7(), 1, empty_trigger())
            .with_journal(Some(Arc::new(s.clone())))
    }

    const SEND: &str = "telegram.send_message";

    fn send_cfg() -> Value {
        json!({ "connection_id": "c1", "chat_id": 1, "text": "hi" })
    }

    #[tokio::test]
    async fn send_node_is_journaled_and_not_repeated_in_same_execution() {
        let s = journal().await;
        let exec_id = Uuid::now_v7();
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let out = json!({ "ok": true, "message_id": 7 });

        let r = run_in(
            Arc::new(Counted(SEND, out.clone(), calls.clone())),
            node("a", SEND, send_cfg()),
            journaled_ctx(exec_id, &s),
        )
        .await;
        assert_eq!(r.status, ExecutionStatus::Completed, "{:?}", r.error);

        // Same execution replays the node: the recorded result is reused.
        let r = run_in(
            Arc::new(Counted(SEND, out.clone(), calls.clone())),
            node("a", SEND, send_cfg()),
            journaled_ctx(exec_id, &s),
        )
        .await;
        assert_eq!(r.status, ExecutionStatus::Completed, "{:?}", r.error);
        assert_eq!(exec(&r, "a").output, Some(out));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn send_node_with_unfinished_claim_is_refused() {
        let s = journal().await;
        let exec_id = Uuid::now_v7();
        let key = crate::agent_runtime::execution::idempotency_key(
            exec_id,
            SEND,
            &json!({ "node": "a", "config": send_cfg() }),
        );
        assert_eq!(
            s.tool_journal_claim(&key, exec_id, SEND).await.unwrap(),
            JournalClaim::Fresh
        );

        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let r = run_in(
            Arc::new(Counted(SEND, json!({ "ok": true }), calls.clone())),
            node("a", SEND, send_cfg()),
            journaled_ctx(exec_id, &s),
        )
        .await;
        assert_eq!(r.status, ExecutionStatus::Failed);
        let err = exec(&r, "a").error.as_deref().unwrap();
        assert!(err.contains("not repeating"), "{err}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn idempotent_node_bypasses_journal() {
        let s = journal().await;
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        for _ in 0..2 {
            let r = run_in(
                Arc::new(Counted("web.search", json!({ "results": [] }), calls.clone())),
                node("a", "web.search", json!({ "query": "x" })),
                journaled_ctx(Uuid::nil(), &s),
            )
            .await;
            assert_eq!(r.status, ExecutionStatus::Completed, "{:?}", r.error);
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
