use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use boarddo_shared::{ExecutionEvent, ExecutionStatus, Node, NodeExecution, WorkflowDefinition};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use super::context::ExecutionContext;
use super::registry::{NodeOutput, NodeRegistry};
use super::validate::validate_workflow;

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

            match handler.execute(node, &mut ctx).await {
                Ok(output) => {
                    let duration_ms = timer.elapsed().as_millis() as u64;
                    let finished_at = Utc::now();
                    ctx.set_node_output(&node.id, output.data.clone());

                    node_executions.push(NodeExecution {
                        id: node_exec_id,
                        execution_id: ctx.execution_id,
                        node_id: node.id.clone(),
                        status: ExecutionStatus::Completed,
                        input,
                        output: Some(output.data.clone()),
                        error: None,
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

        emit(
            on_event,
            ExecutionEvent::ExecutionCompleted {
                execution_id: ctx.execution_id,
                status: ExecutionStatus::Completed,
                error: None,
            },
        );

        ExecutionResult {
            status: ExecutionStatus::Completed,
            error: None,
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
