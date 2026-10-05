//! Shared graph-ops + workflow CRUD helpers used by `/api/agent` and Goal Runtime.

use boarddo_shared::v2::{
    NodeInstance, NodeKind, PlanStep, WorkflowContract, WorkflowPlan,
};
use boarddo_shared::{
    AgentGraphOp, AgentWorkflowSnapshot, CreateWorkflowRequest, Edge, Node, Position,
    UpdateWorkflowRequest, ValidateResponse, WorkflowDefinition, WorkflowRecord, WorkflowStatus,
    type_ids,
};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::engine::registry::NodeRegistry;
use crate::state::SharedState;

#[derive(Debug, Clone)]
pub struct AppliedGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub name: Option<String>,
    pub description: Option<String>,
}

pub fn registry_knows(type_id: &str) -> bool {
    NodeRegistry::with_defaults().contains(type_id)
}

pub fn apply_ops(
    snapshot: &AgentWorkflowSnapshot,
    ops: Vec<AgentGraphOp>,
) -> Result<AppliedGraph, String> {
    let mut nodes = snapshot.nodes.clone();
    let mut edges = snapshot.edges.clone();
    let mut name = None;
    let mut description = None;

    for op in ops {
        match op {
            AgentGraphOp::AddNode {
                id,
                type_id,
                position,
                config,
            } => {
                if !registry_knows(&type_id) {
                    return Err(format!("unknown node type_id `{type_id}`"));
                }
                let node_id = id.unwrap_or_else(|| format!("node_{}", nodes.len() + 1));
                if nodes.iter().any(|n| n.id == node_id) {
                    return Err(format!("node `{node_id}` already exists"));
                }
                nodes.push(Node {
                    id: node_id,
                    type_id: type_id.clone(),
                    category: Some(type_ids::category_for(&type_id)),
                    position: position.unwrap_or_default(),
                    config,
                });
            }
            AgentGraphOp::UpdateNode {
                id,
                config,
                position,
            } => {
                let node = nodes
                    .iter_mut()
                    .find(|n| n.id == id)
                    .ok_or_else(|| format!("unknown node `{id}`"))?;
                if let Some(cfg) = config {
                    node.config = merge_json(&node.config, cfg);
                }
                if let Some(pos) = position {
                    node.position = pos;
                }
            }
            AgentGraphOp::RemoveNode { id } => {
                if !nodes.iter().any(|n| n.id == id) {
                    return Err(format!("unknown node `{id}`"));
                }
                nodes.retain(|n| n.id != id);
                edges.retain(|e| e.source != id && e.target != id);
            }
            AgentGraphOp::AddEdge {
                id,
                source,
                target,
                source_port,
                target_port,
            } => {
                if !nodes.iter().any(|n| n.id == source) {
                    return Err(format!("add_edge source `{source}` not found"));
                }
                if !nodes.iter().any(|n| n.id == target) {
                    return Err(format!("add_edge target `{target}` not found"));
                }
                edges.push(Edge {
                    id: id.unwrap_or_else(|| format!("e_{}", edges.len() + 1)),
                    source,
                    target,
                    source_port,
                    target_port,
                });
            }
            AgentGraphOp::RemoveEdge { id, source, target } => {
                let before = edges.len();
                edges.retain(|e| {
                    if let Some(eid) = &id {
                        return e.id != *eid;
                    }
                    let src_ok = source.as_ref().is_some_and(|s| e.source == *s);
                    let tgt_ok = target.as_ref().is_some_and(|t| e.target == *t);
                    !(src_ok && tgt_ok)
                });
                if edges.len() == before {
                    return Err("remove_edge did not match an existing edge".into());
                }
            }
            AgentGraphOp::SetMeta {
                name: n,
                description: d,
            } => {
                if n.is_some() {
                    name = n;
                }
                if d.is_some() {
                    description = d;
                }
            }
        }
    }

    Ok(AppliedGraph {
        nodes,
        edges,
        name,
        description,
    })
}

/// Validate ops against a snapshot without requiring the caller to keep the graph.
pub fn validate_ops(
    ops: Vec<AgentGraphOp>,
    workflow: &AgentWorkflowSnapshot,
) -> Result<Vec<AgentGraphOp>, String> {
    let copy = ops.clone();
    apply_ops(workflow, ops)?;
    Ok(copy)
}

fn merge_json(base: &Value, incoming: Value) -> Value {
    match (base, incoming) {
        (Value::Object(base_map), Value::Object(inc)) => {
            let mut out = base_map.clone();
            for (k, v) in inc {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        (_, incoming) => incoming,
    }
}

pub fn snapshot_from_record(wf: &WorkflowRecord) -> AgentWorkflowSnapshot {
    AgentWorkflowSnapshot {
        name: wf.name.clone(),
        description: wf.description.clone(),
        nodes: wf.definition.nodes.clone(),
        edges: wf.definition.edges.clone(),
    }
}

pub fn stub_scenario_graph(title: &str, text: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        nodes: vec![
            Node {
                id: "trigger".into(),
                type_id: type_ids::TRIGGER_MANUAL.into(),
                category: Some(type_ids::category_for(type_ids::TRIGGER_MANUAL)),
                position: Position { x: 80.0, y: 160.0 },
                config: json!({}),
            },
            Node {
                id: "log".into(),
                type_id: type_ids::DEBUG_LOG.into(),
                category: Some(type_ids::category_for(type_ids::DEBUG_LOG)),
                position: Position { x: 320.0, y: 160.0 },
                config: json!({
                    "message": format!("Goal `{title}`: {text}")
                }),
            },
        ],
        edges: vec![Edge {
            id: "e_trigger_log".into(),
            source: "trigger".into(),
            target: "log".into(),
            source_port: None,
            target_port: None,
        }],
    }
}

pub fn propose_stub_plan(goal_id: Uuid, title: &str, text: &str) -> WorkflowPlan {
    let now = Utc::now();
    let def = stub_scenario_graph(title, text);
    let contract = WorkflowContract {
        id: Uuid::now_v7(),
        name: title.to_string(),
        description: text.to_string(),
        version: 1,
        status: WorkflowStatus::Draft,
        goal: Some(text.to_string()),
        io: Default::default(),
        budget: Default::default(),
        nodes: def
            .nodes
            .iter()
            .map(|n| NodeInstance {
                id: n.id.clone(),
                type_id: n.type_id.clone(),
                kind: kind_for(&n.type_id),
                title: None,
                intent: None,
                position: n.position,
                config: n.config.clone(),
                budget: None,
                nested: None,
            })
            .collect(),
        edges: def.edges.clone(),
        published_as: None,
        meta: None,
        created_at: now,
        updated_at: now,
    };
    WorkflowPlan {
        id: Uuid::now_v7(),
        goal_id,
        title: title.to_string(),
        summary: Some(format!("Stub scenario for: {text}")),
        steps: vec![
            PlanStep {
                id: "trigger".into(),
                title: "Manual start".into(),
                intent: Some("Start a test run".into()),
                type_id: type_ids::TRIGGER_MANUAL.into(),
                config: json!({}),
                depends_on: vec![],
            },
            PlanStep {
                id: "log".into(),
                title: "Record goal".into(),
                intent: Some("Log the goal text so the test run is observable".into()),
                type_id: type_ids::DEBUG_LOG.into(),
                config: json!({ "message": text }),
                depends_on: vec!["trigger".into()],
            },
        ],
        workflow: Some(contract),
        created_at: now,
    }
}

fn kind_for(type_id: &str) -> NodeKind {
    if type_id.starts_with("trigger.") {
        NodeKind::Trigger
    } else if type_id.starts_with("ai.") {
        NodeKind::Ai
    } else if type_id.starts_with("logic.condition") {
        NodeKind::Decision
    } else if type_id.starts_with("logic.") {
        NodeKind::Control
    } else if type_id.starts_with("data.") {
        NodeKind::Data
    } else if type_id.starts_with("debug.") {
        NodeKind::Debug
    } else {
        NodeKind::Action
    }
}

pub async fn create_workflow(
    state: &SharedState,
    name: String,
    description: String,
    definition: WorkflowDefinition,
) -> anyhow::Result<WorkflowRecord> {
    state
        .storage
        .create_workflow(CreateWorkflowRequest {
            name,
            description,
            nodes: definition.nodes,
            edges: definition.edges,
        })
        .await
}

pub async fn update_workflow(
    state: &SharedState,
    id: Uuid,
    name: String,
    description: String,
    status: WorkflowStatus,
    definition: WorkflowDefinition,
) -> anyhow::Result<Option<WorkflowRecord>> {
    let record = state
        .storage
        .update_workflow(
            id,
            UpdateWorkflowRequest {
                name,
                description,
                status,
                nodes: definition.nodes,
                edges: definition.edges,
            },
        )
        .await?;
    if let Some(wf) = &record
        && let Err(err) = state.runtime.sync_workflow(state, wf).await
    {
        tracing::warn!(error = %err, workflow_id = %wf.id, "runtime.sync_after_agent_update");
    }
    Ok(record)
}

pub fn validate_record(state: &SharedState, wf: &WorkflowRecord) -> ValidateResponse {
    match state.engine.validate(&wf.definition) {
        Ok(()) => ValidateResponse {
            valid: true,
            errors: vec![],
        },
        Err(errors) => ValidateResponse {
            valid: false,
            errors,
        },
    }
}

pub async fn link_workflow(
    state: &SharedState,
    run_id: Uuid,
    workflow_id: Uuid,
) -> anyhow::Result<()> {
    state.storage.link_goal_run_workflow(run_id, workflow_id).await
}

pub fn definition_from_plan(plan: &WorkflowPlan) -> WorkflowDefinition {
    if let Some(contract) = &plan.workflow {
        return contract.to_phase1_definition();
    }
    stub_scenario_graph(&plan.title, plan.summary.as_deref().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::AgentGraphOp;

    #[test]
    fn apply_add_and_edge() {
        let snap = AgentWorkflowSnapshot {
            name: "t".into(),
            description: String::new(),
            nodes: vec![],
            edges: vec![],
        };
        let ops = vec![
            AgentGraphOp::AddNode {
                id: Some("t1".into()),
                type_id: type_ids::TRIGGER_MANUAL.into(),
                position: None,
                config: json!({}),
            },
            AgentGraphOp::AddNode {
                id: Some("log".into()),
                type_id: type_ids::DEBUG_LOG.into(),
                position: None,
                config: json!({ "message": "hi" }),
            },
            AgentGraphOp::AddEdge {
                id: None,
                source: "t1".into(),
                target: "log".into(),
                source_port: None,
                target_port: None,
            },
        ];
        let applied = apply_ops(&snap, ops).unwrap();
        assert_eq!(applied.nodes.len(), 2);
        assert_eq!(applied.edges.len(), 1);
    }

    #[test]
    fn rejects_unknown_type() {
        let snap = AgentWorkflowSnapshot {
            name: "t".into(),
            description: String::new(),
            nodes: vec![],
            edges: vec![],
        };
        let ops = vec![AgentGraphOp::AddNode {
            id: None,
            type_id: "not.a.type".into(),
            position: None,
            config: json!({}),
        }];
        assert!(apply_ops(&snap, ops).is_err());
    }
}
