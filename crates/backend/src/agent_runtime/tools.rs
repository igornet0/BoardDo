//! Execute planned tool calls via workflow meta-tools or SmartDo node handlers.

use std::time::Duration;

use boarddo_shared::v2::{
    GoalRun, GoalSpec, PlannedToolCall, RunContext, WorkflowPlan, capability_for_tool,
    resolve_tool_alias,
};
use boarddo_shared::{
    AgentGraphOp, AgentWorkflowSnapshot, ExecutionStatus, Node, Position, WorkflowDefinition,
    WorkflowRecord, WorkflowStatus,
};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api::run::spawn_execution;
use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeOutput, NodeRegistry};
use crate::state::SharedState;
use crate::workflow::{
    apply_ops, create_workflow, definition_from_plan, propose_stub_plan, snapshot_from_record,
    stub_scenario_graph, update_workflow, validate_record,
};
use boarddo_telegram::TelegramUserGateway;

pub struct ToolGateway {
    registry: NodeRegistry,
}

impl ToolGateway {
    pub fn new() -> Self {
        Self {
            registry: NodeRegistry::with_defaults(),
        }
    }

    pub async fn execute(
        &self,
        state: &SharedState,
        spec: &GoalSpec,
        run: &GoalRun,
        call: &PlannedToolCall,
    ) -> Result<Value, String> {
        let canonical = resolve_tool_alias(&call.type_id).to_string();
        let call = PlannedToolCall {
            id: call.id.clone(),
            type_id: canonical.clone(),
            config: call.config.clone(),
        };
        let cap = capability_for_tool(&call.type_id);
        let run_ctx = RunContext::new(
            Uuid::now_v7(),
            spec.id,
            1,
            json!({ "goal_id": spec.id, "run_id": run.id }),
            spec.budget.clone(),
        );
        if !run_ctx.can(cap) {
            return Err(format!("capability `{cap}` not allowed"));
        }

        match call.type_id.as_str() {
            "memory.write" => memory_write(state, run, &call.config).await,
            "memory.read" => memory_read(state, run, &call.config).await,
            "analytics.record_lead" => {
                super::record_lead(state, spec, delta_from(&call.config, 1.0))
                    .await
                    .map(|r| json!({ "ok": true, "current": r.current, "status": r.status }))
                    .map_err(|e| e.to_string())
            }
            "analytics.record_progress" => {
                super::record_lead(state, spec, delta_from(&call.config, 1.0))
                    .await
                    .map(|r| json!({ "ok": true, "current": r.current, "status": r.status }))
                    .map_err(|e| e.to_string())
            }
            "workflow.list" => workflow_list(state).await,
            "workflow.get" => workflow_get(state, &call.config).await,
            "workflow.create" => workflow_create(state, spec, run, &call.config).await,
            "workflow.update" => workflow_update(state, spec, run, &call.config).await,
            "workflow.validate" => workflow_validate(state, &call.config).await,
            "workflow.run" => workflow_run(state, spec, run, &call.config).await,
            "workflow.activate" => workflow_activate(state, &call.config).await,
            "workflow.get_execution" => workflow_get_execution(state, &call.config).await,
            "workflow.list_executions" => workflow_list_executions(state, &call.config).await,
            "scenario.apply_ops" => scenario_apply_ops(state, spec, run, &call.config).await,
            "goal.propose_plan" => goal_propose_plan(state, spec, run).await,
            "goal.materialize_plan" => {
                goal_materialize_plan(state, spec, run, &call.config).await
            }
            "campaign.bootstrap_plan" => campaign_bootstrap_plan(state, spec, run).await,
            "connection.list" => connection_list(state).await,
            "connection.test" => connection_test(state, &call.config).await,
            "telegram.accounts.list" => telegram_accounts_list(state).await,
            "research.search" | "research.fetch" | "research.note" | "research.competitors"
            | "research.audience" | "research.pains" => {
                marketing_research(state, spec, run, &call.type_id, &call.config).await
            }
            "content.create_draft" | "content.rewrite" | "content.variant" => {
                marketing_content(state, spec, run, &call.type_id, &call.config).await
            }
            "publisher.telegram.prepare" | "publisher.telegram.send" | "publish.prepare"
            | "publish.send" | "publish.schedule" | "idea.generate" | "idea.expand"
            | "idea.combine" | "idea.validate" | "idea.research" | "strategy.create"
            | "strategy.update" | "brief.create" | "content.create" | "content.adapt"
            | "content.critique" | "content.fact_check" | "content.expand"
            | "content.shorten" | "content.translate" | "insight.create" | "analytics.read" => {
                crate::content::tools::execute(state, spec, run, &call.type_id, &call.config).await
            }
            "lead.create" | "lead.update" | "lead.score" | "conversation.classify"
            | "marketing.simulate_inbound" | "analytics.campaign_metrics" => {
                marketing_leads(state, spec, run, &call.type_id, &call.config).await
            }
            "tool.create"
            | "tool.update"
            | "tool.test"
            | "tool.run"
            | "tool.list"
            | "tool.get"
            | "tool.find"
            | "tool.delete" => {
                crate::tools::agent::execute(state, &call.type_id, &call.config).await
            }
            other => self.execute_node(state, spec, run, &call, other).await,
        }
    }

    pub(crate) async fn execute_node(
        &self,
        state: &SharedState,
        spec: &GoalSpec,
        run: &GoalRun,
        call: &PlannedToolCall,
        type_id: &str,
    ) -> Result<Value, String> {
        let handler = self
            .registry
            .get(type_id)
            .ok_or_else(|| format!("unknown tool `{type_id}`"))?;

        let node = Node {
            id: format!("tool-{}", call.id),
            type_id: type_id.into(),
            category: None,
            position: Position::default(),
            config: enrich_config(spec, type_id, &call.config),
        };

        let trigger = json!({
            "goal_id": spec.id,
            "run_id": run.id,
            "text": spec.text,
            "metric": spec.metric,
        });

        let mut ctx = ExecutionContext::with_source(
            Uuid::now_v7(),
            spec.id,
            1,
            trigger,
            Some("goal.agent".into()),
        )
        .with_connections(state.connections.clone())
        .with_telegram_user({
            let gw: std::sync::Arc<dyn boarddo_telegram::TelegramUserGateway> =
                state.telegram.clone();
            gw
        })
        .with_web(std::sync::Arc::new(
            crate::integrations::web::HttpWebGateway::from_env(),
        ))
        .with_browser(crate::integrations::browser::shared())
        .with_custom_tools(state.custom_tools.clone());

        let NodeOutput { data, .. } = handler.execute(&node, &mut ctx).await?;
        Ok(data)
    }
}

impl Default for ToolGateway {
    fn default() -> Self {
        Self::new()
    }
}

fn enrich_config(spec: &GoalSpec, type_id: &str, config: &Value) -> Value {
    let mut obj = match config {
        Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    if type_id.starts_with("ai.")
        && !obj
            .get("connection_id")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    {
        if let Some(id) = spec.openai_connection_id {
            obj.insert("connection_id".into(), json!(id.to_string()));
        }
    }
    Value::Object(obj)
}

pub fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

fn delta_from(config: &Value, default: f64) -> f64 {
    config
        .get("delta")
        .and_then(Value::as_f64)
        .unwrap_or(default)
}

fn uuid_field(config: &Value, key: &str) -> Result<Uuid, String> {
    config
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing `{key}`"))
        .and_then(|s| Uuid::parse_str(s).map_err(|e| e.to_string()))
}

async fn memory_write(state: &SharedState, run: &GoalRun, config: &Value) -> Result<Value, String> {
    let key = config
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("note");
    let value = config.get("value").cloned().unwrap_or(Value::Null);
    let kind = config
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("long");
    state
        .storage
        .upsert_memory(run.id, key, &value, kind)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true, "key": key }))
}

async fn memory_read(state: &SharedState, run: &GoalRun, config: &Value) -> Result<Value, String> {
    let key = config
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("note");
    let entry = state
        .storage
        .get_memory(run.id, key)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "key": key, "value": entry.map(|e| e.value) }))
}

async fn workflow_list(state: &SharedState) -> Result<Value, String> {
    let list = state
        .storage
        .list_workflows()
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "workflows": list }))
}

async fn workflow_get(state: &SharedState, config: &Value) -> Result<Value, String> {
    let id = uuid_field(config, "workflow_id")?;
    let wf = state
        .storage
        .get_workflow(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workflow not found".to_string())?;
    Ok(serde_json::to_value(wf).unwrap_or(Value::Null))
}

fn definition_from_config(config: &Value, fallback_title: &str, fallback_text: &str) -> WorkflowDefinition {
    if let Some(ops_val) = config.get("ops")
        && let Ok(ops) = serde_json::from_value::<Vec<AgentGraphOp>>(ops_val.clone())
    {
        let snap = AgentWorkflowSnapshot {
            name: fallback_title.into(),
            description: fallback_text.into(),
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        if let Ok(applied) = apply_ops(&snap, ops) {
            return WorkflowDefinition {
                nodes: applied.nodes,
                edges: applied.edges,
            };
        }
    }
    if let (Some(nodes), Some(edges)) = (
        config
            .get("nodes")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        config
            .get("edges")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
    ) {
        return WorkflowDefinition { nodes, edges };
    }
    stub_scenario_graph(fallback_title, fallback_text)
}

async fn maybe_validate_run(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    wf: &WorkflowRecord,
    config: &Value,
) -> Result<Value, String> {
    let mut out = json!({ "workflow": wf });
    let do_validate = config
        .get("validate")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if do_validate {
        let validation = validate_record(state, wf);
        out["validation"] = json!(validation);
        if !validation.valid {
            return Ok(out);
        }
    }
    let do_run = config.get("run").and_then(Value::as_bool).unwrap_or(false);
    if do_run {
        let trigger = config.get("trigger").cloned().unwrap_or(json!({
            "goal_id": spec.id,
            "text": spec.text,
        }));
        let wait = config.get("wait").and_then(Value::as_bool).unwrap_or(true);
        let exec = run_and_maybe_wait(state, wf, trigger, wait).await?;
        out["execution"] = exec;
    }
    let _ = crate::workflow::agent_ops::link_workflow(state, run.id, wf.id).await;
    Ok(out)
}

async fn workflow_create(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let name = config
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&spec.title)
        .to_string();
    let description = config
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or(&spec.text)
        .to_string();
    let definition = definition_from_config(config, &name, &description);
    let wf = create_workflow(state, name, description, definition)
        .await
        .map_err(|e| e.to_string())?;
    crate::workflow::agent_ops::link_workflow(state, run.id, wf.id)
        .await
        .map_err(|e| e.to_string())?;
    maybe_validate_run(state, spec, run, &wf, config).await
}

async fn workflow_update(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let id = uuid_field(config, "workflow_id")?;
    let existing = state
        .storage
        .get_workflow(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workflow not found".to_string())?;
    let name = config
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&existing.name)
        .to_string();
    let description = config
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or(&existing.description)
        .to_string();
    let status = config
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status)
        .unwrap_or(existing.status);
    let definition = if config.get("nodes").is_some() || config.get("ops").is_some() {
        definition_from_config(config, &name, &description)
    } else {
        existing.definition.clone()
    };
    let wf = update_workflow(state, id, name, description, status, definition)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workflow not found".to_string())?;
    crate::workflow::agent_ops::link_workflow(state, run.id, wf.id)
        .await
        .map_err(|e| e.to_string())?;
    maybe_validate_run(state, spec, run, &wf, config).await
}

fn parse_status(s: &str) -> WorkflowStatus {
    match s {
        "active" => WorkflowStatus::Active,
        "archived" => WorkflowStatus::Archived,
        _ => WorkflowStatus::Draft,
    }
}

async fn workflow_validate(state: &SharedState, config: &Value) -> Result<Value, String> {
    let id = uuid_field(config, "workflow_id")?;
    let wf = state
        .storage
        .get_workflow(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workflow not found".to_string())?;
    Ok(json!(validate_record(state, &wf)))
}

async fn workflow_run(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let id = uuid_field(config, "workflow_id")?;
    let wf = state
        .storage
        .get_workflow(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workflow not found".to_string())?;
    crate::workflow::agent_ops::link_workflow(state, run.id, wf.id)
        .await
        .map_err(|e| e.to_string())?;
    let trigger = config.get("trigger").cloned().unwrap_or(json!({
        "goal_id": spec.id,
        "text": spec.text,
    }));
    let wait = config.get("wait").and_then(Value::as_bool).unwrap_or(true);
    run_and_maybe_wait(state, &wf, trigger, wait).await
}

async fn workflow_activate(state: &SharedState, config: &Value) -> Result<Value, String> {
    let id = uuid_field(config, "workflow_id")?;
    let wf = state
        .storage
        .set_workflow_status(id, WorkflowStatus::Active)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workflow not found".to_string())?;
    if let Err(err) = state.runtime.sync_workflow(state, &wf).await {
        tracing::warn!(error = %err, workflow_id = %wf.id, "runtime.sync_after_activate");
    }
    Ok(json!({ "ok": true, "workflow": wf }))
}

async fn run_and_maybe_wait(
    state: &SharedState,
    wf: &WorkflowRecord,
    trigger: Value,
    wait: bool,
) -> Result<Value, String> {
    let resp = spawn_execution(state, wf, trigger, "manual")
        .await
        .map_err(|e| e.to_string())?;
    if !wait {
        return Ok(json!(resp));
    }
    let exec = wait_execution(state, resp.execution_id, 12_000).await?;
    Ok(json!({
        "execution_id": exec.id,
        "status": exec.status,
        "error": exec.error,
        "workflow_id": exec.workflow_id,
    }))
}

async fn wait_execution(
    state: &SharedState,
    id: Uuid,
    timeout_ms: u64,
) -> Result<boarddo_shared::Execution, String> {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if let Some(ex) = state
            .storage
            .get_execution(id)
            .await
            .map_err(|e| e.to_string())?
        {
            match ex.status {
                ExecutionStatus::Completed
                | ExecutionStatus::Failed
                | ExecutionStatus::Cancelled => return Ok(ex),
                _ => {}
            }
        }
        if std::time::Instant::now() >= deadline {
            return state
                .storage
                .get_execution(id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "execution not found".to_string());
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

async fn workflow_get_execution(state: &SharedState, config: &Value) -> Result<Value, String> {
    let id = uuid_field(config, "execution_id")?;
    let ex = state
        .storage
        .get_execution(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "execution not found".to_string())?;
    let nodes = state
        .storage
        .get_node_executions(id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "execution": ex, "nodes": nodes }))
}

async fn workflow_list_executions(state: &SharedState, config: &Value) -> Result<Value, String> {
    let id = uuid_field(config, "workflow_id")?;
    let list = state
        .storage
        .list_executions_for_workflow(id, 10)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "executions": list }))
}

async fn scenario_apply_ops(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let ops: Vec<AgentGraphOp> = config
        .get("ops")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .ok_or_else(|| "missing `ops`".to_string())?;
    if let Ok(id) = uuid_field(config, "workflow_id") {
        let existing = state
            .storage
            .get_workflow(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "workflow not found".to_string())?;
        let applied = apply_ops(&snapshot_from_record(&existing), ops)?;
        let wf = update_workflow(
            state,
            id,
            applied.name.unwrap_or(existing.name),
            applied.description.unwrap_or(existing.description),
            existing.status,
            WorkflowDefinition {
                nodes: applied.nodes,
                edges: applied.edges,
            },
        )
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "workflow not found".to_string())?;
        crate::workflow::agent_ops::link_workflow(state, run.id, wf.id)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(json!({ "workflow": wf, "validation": validate_record(state, &wf) }));
    }
    let applied = apply_ops(
        &AgentWorkflowSnapshot {
            name: spec.title.clone(),
            description: spec.text.clone(),
            nodes: vec![],
            edges: vec![],
        },
        ops,
    )?;
    let wf = create_workflow(
        state,
        applied.name.unwrap_or_else(|| spec.title.clone()),
        applied.description.unwrap_or_else(|| spec.text.clone()),
        WorkflowDefinition {
            nodes: applied.nodes,
            edges: applied.edges,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    crate::workflow::agent_ops::link_workflow(state, run.id, wf.id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "workflow": wf, "validation": validate_record(state, &wf) }))
}

async fn goal_propose_plan(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
) -> Result<Value, String> {
    let plan = propose_stub_plan(spec.id, &spec.title, &spec.text);
    persist_plan(state, run, &plan).await?;
    Ok(serde_json::to_value(plan).unwrap_or(Value::Null))
}

async fn campaign_bootstrap_plan(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
) -> Result<Value, String> {
    let brief = boarddo_shared::v2::brief_from_constraints(&spec.constraints);
    let plan = boarddo_shared::v2::CampaignPlan::bootstrap_from_brief(&brief);
    let value = serde_json::to_value(&plan).unwrap_or(Value::Null);
    state
        .storage
        .upsert_memory(run.id, "campaign.plan", &value, "long")
        .await
        .map_err(|e| e.to_string())?;
    state
        .storage
        .upsert_memory(run.id, "campaign.brief", &json!(brief), "long")
        .await
        .map_err(|e| e.to_string())?;
    Ok(value)
}

async fn persist_plan(
    state: &SharedState,
    run: &GoalRun,
    plan: &WorkflowPlan,
) -> Result<(), String> {
    let value = serde_json::to_value(plan).unwrap_or(Value::Null);
    state
        .storage
        .upsert_memory(run.id, "plan.current", &value, "long")
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn goal_materialize_plan(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let plan: WorkflowPlan = if let Some(raw) = config.get("plan") {
        serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?
    } else if let Some(entry) = state
        .storage
        .get_memory(run.id, "plan.current")
        .await
        .map_err(|e| e.to_string())?
    {
        serde_json::from_value(entry.value).map_err(|e| e.to_string())?
    } else {
        propose_stub_plan(spec.id, &spec.title, &spec.text)
    };
    persist_plan(state, run, &plan).await?;
    let definition = definition_from_plan(&plan);
    let mut create_cfg = json!({
        "name": plan.title,
        "description": plan.summary.clone().unwrap_or_else(|| spec.text.clone()),
        "validate": config.get("validate").cloned().unwrap_or(json!(true)),
        "run": config.get("run").cloned().unwrap_or(json!(true)),
        "wait": true,
    });
    if let Some(obj) = create_cfg.as_object_mut() {
        obj.insert("nodes".into(), json!(definition.nodes));
        obj.insert("edges".into(), json!(definition.edges));
    }
    workflow_create(state, spec, run, &create_cfg).await
}

async fn connection_list(state: &SharedState) -> Result<Value, String> {
    let list = state
        .connections
        .list()
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "connections": list.into_iter().map(|c| json!({
            "id": c.id,
            "name": c.name,
            "type": c.connection_type,
            "enabled": c.enabled,
            "has_secret": c.has_secret,
        })).collect::<Vec<_>>()
    }))
}

async fn connection_test(state: &SharedState, config: &Value) -> Result<Value, String> {
    let id = uuid_field(config, "connection_id")?;
    let (ok, message) = state
        .connections
        .test(id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "ok": ok, "message": message }))
}

async fn telegram_accounts_list(state: &SharedState) -> Result<Value, String> {
    let accounts = state
        .telegram
        .list_accounts()
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "accounts": accounts }))
}

async fn marketing_research(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    type_id: &str,
    config: &Value,
) -> Result<Value, String> {
    use boarddo_shared::v2::{ResearchNote, ResearchNoteType, product_from_constraints};
    let now = Utc::now();
    let product = product_from_constraints(&spec.constraints);

    match type_id {
        "research.search" => {
            let query = config
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or(spec.text.as_str());
            // Reuse web.search infrastructure via node handler.
            let gateway = ToolGateway::new();
            let call = PlannedToolCall {
                id: "research-search-inner".into(),
                type_id: "web.search".into(),
                config: json!({ "query": query, "limit": config.get("limit").cloned().unwrap_or(json!(6)) }),
            };
            let search = gateway.execute_node(state, spec, run, &call, "web.search").await?;
            let results = search
                .get("results")
                .cloned()
                .or_else(|| search.get("items").cloned())
                .unwrap_or(search.clone());
            let mut created = Vec::new();
            if let Some(arr) = results.as_array() {
                for (i, item) in arr.iter().take(5).enumerate() {
                    let url = item
                        .get("url")
                        .or_else(|| item.get("href"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let title = item
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("source")
                        .to_string();
                    let snippet = item
                        .get("snippet")
                        .or_else(|| item.get("body"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let note = ResearchNote {
                        id: Uuid::now_v7(),
                        goal_id: spec.id,
                        run_id: run.id,
                        campaign_id: Some(run.id),
                        note_type: ResearchNoteType::MarketSignal,
                        content: if snippet.is_empty() {
                            format!("Signal from {title}")
                        } else {
                            snippet
                        },
                        source_url: url,
                        source_title: Some(title),
                        confidence: 0.55 + (i as f32) * 0.05,
                        created_at: now,
                        updated_at: now,
                        metadata: Default::default(),
                    };
                    state
                        .storage
                        .insert_research_note(&note)
                        .await
                        .map_err(|e| e.to_string())?;
                    created.push(note.id);
                }
            }
            if created.is_empty() {
                let note = ResearchNote {
                    id: Uuid::now_v7(),
                    goal_id: spec.id,
                    run_id: run.id,
                    campaign_id: Some(run.id),
                    note_type: ResearchNoteType::MarketSignal,
                    content: format!(
                        "Market interest around {product} workflow automation and agent runtimes."
                    ),
                    source_url: None,
                    source_title: Some("synthetic-analysis".into()),
                    confidence: 0.5,
                    created_at: now,
                    updated_at: now,
                    metadata: Default::default(),
                };
                state
                    .storage
                    .insert_research_note(&note)
                    .await
                    .map_err(|e| e.to_string())?;
                created.push(note.id);
            }
            Ok(json!({ "ok": true, "notes_created": created.len(), "ids": created }))
        }
        "research.fetch" => {
            let url = config
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing url".to_string())?;
            let gateway = ToolGateway::new();
            let call = PlannedToolCall {
                id: "research-fetch-inner".into(),
                type_id: "web.open".into(),
                config: json!({ "url": url, "max_chars": 4000, "include_links": false }),
            };
            let fetched = gateway.execute_node(state, spec, run, &call, "web.open").await?;
            let title = fetched
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(url)
                .to_string();
            let extract = fetched
                .get("text")
                .or_else(|| fetched.get("content"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(2000)
                .collect::<String>();
            let note = ResearchNote {
                id: Uuid::now_v7(),
                goal_id: spec.id,
                run_id: run.id,
                campaign_id: Some(run.id),
                note_type: ResearchNoteType::ContentPattern,
                content: if extract.is_empty() {
                    format!("Fetched source {title}")
                } else {
                    extract
                },
                source_url: Some(url.into()),
                source_title: Some(title),
                confidence: 0.7,
                created_at: now,
                updated_at: now,
                metadata: Default::default(),
            };
            state
                .storage
                .insert_research_note(&note)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "note_id": note.id }))
        }
        "research.note" => {
            let content = config
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing content".to_string())?;
            let note_type = config
                .get("type")
                .and_then(Value::as_str)
                .map(ResearchNoteType::parse)
                .unwrap_or(ResearchNoteType::MarketSignal);
            let note = ResearchNote {
                id: Uuid::now_v7(),
                goal_id: spec.id,
                run_id: run.id,
                campaign_id: Some(run.id),
                note_type,
                content: content.into(),
                source_url: config
                    .get("source_url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                source_title: config
                    .get("source_title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                confidence: config
                    .get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.7) as f32,
                created_at: now,
                updated_at: now,
                metadata: Default::default(),
            };
            state
                .storage
                .insert_research_note(&note)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "note": note }))
        }
        "research.competitors" => {
            let notes = [
                (
                    ResearchNoteType::Competitor,
                    format!("Competitors position themselves as Zapier/Make clones; {product} differentiates via goal runtime."),
                    0.72,
                ),
                (
                    ResearchNoteType::Offer,
                    format!("{product} offer: zero-budget autonomous marketing loop with Telegram."),
                    0.68,
                ),
            ];
            let mut ids = Vec::new();
            for (note_type, content, confidence) in notes {
                let note = ResearchNote {
                    id: Uuid::now_v7(),
                    goal_id: spec.id,
                    run_id: run.id,
                    campaign_id: Some(run.id),
                    note_type,
                    content,
                    source_url: None,
                    source_title: Some("competitor-analysis".into()),
                    confidence,
                    created_at: now,
                    updated_at: now,
                    metadata: Default::default(),
                };
                state
                    .storage
                    .insert_research_note(&note)
                    .await
                    .map_err(|e| e.to_string())?;
                ids.push(note.id);
            }
            Ok(json!({ "ok": true, "ids": ids }))
        }
        "research.audience" => {
            let audiences = [
                "Indie hackers shipping solo SaaS",
                "Small automation agencies",
                "Founders tired of manual lead triage",
            ];
            let mut ids = Vec::new();
            for a in audiences {
                let note = ResearchNote {
                    id: Uuid::now_v7(),
                    goal_id: spec.id,
                    run_id: run.id,
                    campaign_id: Some(run.id),
                    note_type: ResearchNoteType::Audience,
                    content: a.into(),
                    source_url: None,
                    source_title: Some("audience-model".into()),
                    confidence: 0.8,
                    created_at: now,
                    updated_at: now,
                    metadata: Default::default(),
                };
                state
                    .storage
                    .insert_research_note(&note)
                    .await
                    .map_err(|e| e.to_string())?;
                ids.push(note.id);
            }
            state
                .storage
                .upsert_memory(run.id, "audience", &json!(audiences), "long")
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "ids": ids, "audience": audiences }))
        }
        "research.pains" => {
            let pains = [
                (
                    "Small businesses spend significant time manually processing inbound leads.",
                    0.82,
                ),
                (
                    "Teams glue many tools together without a durable goal loop.",
                    0.78,
                ),
                (
                    "Content is published without experiment attribution or learning.",
                    0.74,
                ),
            ];
            let mut ids = Vec::new();
            for (content, confidence) in pains {
                let note = ResearchNote {
                    id: Uuid::now_v7(),
                    goal_id: spec.id,
                    run_id: run.id,
                    campaign_id: Some(run.id),
                    note_type: ResearchNoteType::Pain,
                    content: content.into(),
                    source_url: None,
                    source_title: Some("pain-model".into()),
                    confidence,
                    created_at: now,
                    updated_at: now,
                    metadata: Default::default(),
                };
                state
                    .storage
                    .insert_research_note(&note)
                    .await
                    .map_err(|e| e.to_string())?;
                ids.push(note.id);
            }
            state
                .storage
                .upsert_memory(
                    run.id,
                    "pains",
                    &json!(pains.iter().map(|(c, _)| *c).collect::<Vec<_>>()),
                    "long",
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "ids": ids }))
        }
        other => Err(format!("unknown research tool `{other}`")),
    }
}

async fn marketing_content(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    type_id: &str,
    config: &Value,
) -> Result<Value, String> {
    use boarddo_shared::v2::{
        ContentDraft, ContentDraftStatus, draft_idempotency_key, product_from_constraints,
    };
    let now = Utc::now();
    let product = product_from_constraints(&spec.constraints);
    let experiment_id = config
        .get("experiment_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok());
    let channel = config
        .get("channel")
        .and_then(Value::as_str)
        .unwrap_or("telegram")
        .to_string();

    let body = match type_id {
        "content.create_draft" => config
            .get("body")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!(
                    "Promote {product}: goal-driven marketing without ad spend. Reply INTERESTED."
                )
            }),
        "content.rewrite" | "content.variant" => {
            let base = if let Some(id) = config
                .get("content_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
            {
                state
                    .storage
                    .get_content_draft(id)
                    .await
                    .map_err(|e| e.to_string())?
                    .map(|d| d.body)
                    .unwrap_or_default()
            } else {
                config
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };
            format!("Variant: {base}\n\n— rewritten for clarity around {product}.")
        }
        _ => return Err("unknown content tool".into()),
    };

    let id = Uuid::now_v7();
    let key = draft_idempotency_key(spec.id, Some(run.id), experiment_id, id);
    if let Some(existing) = state
        .storage
        .get_draft_by_idempotency(&key)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(json!({ "ok": true, "draft": existing, "idempotent": true }));
    }

    let mut metadata = serde_json::Map::new();
    if let Some(angle) = config.get("angle").and_then(Value::as_str) {
        metadata.insert("angle".into(), json!(angle));
    }
    let draft = ContentDraft {
        id,
        goal_id: spec.id,
        run_id: run.id,
        campaign_id: Some(run.id),
        experiment_id,
        channel,
        body,
        status: ContentDraftStatus::Draft,
        idempotency_key: key,
        telegram_message_id: None,
        account_id: None,
        chat_id: None,
        created_at: now,
        updated_at: now,
        metadata,
        simulated: false,
    };
    state
        .storage
        .insert_content_draft(&draft)
        .await
        .map_err(|e| e.to_string())?;
    let _ = crate::content::tools::sync_draft_to_content_item(state, spec, run, &draft).await;
    Ok(json!({ "ok": true, "draft": draft, "content_id": draft.id }))
}

async fn marketing_publisher(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    type_id: &str,
    config: &Value,
) -> Result<Value, String> {
    use boarddo_shared::v2::ContentDraftStatus;
    use tglib::{TelegramAccountId, TelegramChatId};

    let content_id = config
        .get("content_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| "missing content_id".to_string())?;
    let mut draft = state
        .storage
        .get_content_draft(content_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "draft not found".to_string())?;

    // Idempotency: never double-publish.
    if draft.status == ContentDraftStatus::Published {
        return Ok(json!({
            "ok": true,
            "idempotent": true,
            "draft": draft,
            "message": "already published"
        }));
    }

    match type_id {
        "publisher.telegram.prepare" => {
            let accounts = state
                .telegram
                .list_accounts()
                .await
                .map_err(|e| e.to_string())?;
            let account_id = config
                .get("account_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    spec.constraints
                        .get("telegram_account_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .or_else(|| accounts.first().map(|a| a.id.to_string()));
            let chat_id = config
                .get("chat_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    spec.constraints
                        .get("telegram_chat_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .or_else(|| {
                    spec.constraints
                        .pointer("/telegram_channels/0")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "pending-approval".into());

            draft.account_id = account_id.clone();
            draft.chat_id = Some(chat_id.clone());
            draft.status = ContentDraftStatus::PendingApproval;
            if let Some(text) = config.get("text").and_then(Value::as_str) {
                draft.body = text.into();
            }
            draft.updated_at = Utc::now();
            state
                .storage
                .update_content_draft(&draft)
                .await
                .map_err(|e| e.to_string())?;

            Ok(json!({
                "ok": true,
                "draft": draft,
                "resolved_account_id": account_id,
                "resolved_chat_id": chat_id,
                "next": "approval_required"
            }))
        }
        "publisher.telegram.send" => {
            if draft.status != ContentDraftStatus::Approved
                && draft.status != ContentDraftStatus::PendingApproval
                && draft.status != ContentDraftStatus::Draft
            {
                return Err(format!(
                    "draft status `{}` cannot be published",
                    draft.status.as_str()
                ));
            }
            let account_id = draft
                .account_id
                .as_deref()
                .or_else(|| config.get("account_id").and_then(Value::as_str))
                .or_else(|| {
                    spec.constraints
                        .get("telegram_account_id")
                        .and_then(Value::as_str)
                });
            let chat_id = draft
                .chat_id
                .as_deref()
                .or_else(|| config.get("chat_id").and_then(Value::as_str))
                .or_else(|| {
                    spec.constraints
                        .get("telegram_chat_id")
                        .and_then(Value::as_str)
                })
                .unwrap_or("pending-approval");

            let demo = spec
                .constraints
                .get("demo")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || chat_id == "pending-approval"
                || account_id.is_none();

            if demo {
                draft.status = ContentDraftStatus::Published;
                draft.telegram_message_id = Some(format!("sim-{}", Uuid::now_v7()));
                draft.simulated = true;
                draft.updated_at = Utc::now();
                draft
                    .metadata
                    .insert("publish_mode".into(), json!("SIMULATED"));
                state
                    .storage
                    .update_content_draft(&draft)
                    .await
                    .map_err(|e| e.to_string())?;
                return Ok(json!({
                    "ok": true,
                    "simulated": true,
                    "draft": draft,
                    "message": "SIMULATED publish (no live Telegram account/chat)"
                }));
            }

            let account_uuid = Uuid::parse_str(account_id.unwrap()).map_err(|e| e.to_string())?;
            let chat = TelegramChatId(
                chat_id
                    .parse::<i64>()
                    .map_err(|_| format!("invalid chat_id `{chat_id}`"))?,
            );
            let message_id = state
                .telegram
                .send_message(
                    TelegramAccountId(account_uuid),
                    chat,
                    draft.body.clone(),
                    Some("markdown".into()),
                    None,
                    None,
                    Some(spec.id),
                )
                .await
                .map_err(|e| e.to_string())?;
            draft.status = ContentDraftStatus::Published;
            draft.telegram_message_id = Some(message_id.0.to_string());
            draft.updated_at = Utc::now();
            state
                .storage
                .update_content_draft(&draft)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "draft": draft, "message_id": message_id.0 }))
        }
        other => Err(format!("unknown publisher tool `{other}`")),
    }
}

async fn marketing_leads(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    type_id: &str,
    config: &Value,
) -> Result<Value, String> {
    use boarddo_shared::v2::{Lead, LeadStage, classify_intent, score_lead_signals, stage_from_score};

    match type_id {
        "lead.create" => {
            let chat_id = config
                .get("chat_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let text = config.get("text").and_then(Value::as_str).unwrap_or("");
            let intent = classify_intent(text);
            let (score, reasons) = score_lead_signals(&[intent]);
            let now = Utc::now();
            let lead = Lead {
                id: Uuid::now_v7(),
                goal_id: spec.id,
                run_id: run.id,
                campaign_id: Some(run.id),
                experiment_id: config
                    .get("experiment_id")
                    .and_then(Value::as_str)
                    .and_then(|s| Uuid::parse_str(s).ok()),
                source: config
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("telegram")
                    .into(),
                source_reference: Some(chat_id.into()),
                chat_id: Some(chat_id.into()),
                account_id: config
                    .get("account_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                stage: stage_from_score(score, intent),
                score,
                score_reasons: reasons,
                first_seen_at: now,
                last_activity_at: now,
                metadata: Default::default(),
                simulated: config
                    .get("simulated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            state
                .storage
                .insert_lead(&lead)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "lead": lead }))
        }
        "lead.update" => {
            let id = uuid_field(config, "lead_id")?;
            let mut lead = state
                .storage
                .get_lead(id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "lead not found".to_string())?;
            if let Some(stage) = config.get("stage").and_then(Value::as_str) {
                lead.stage = LeadStage::parse(stage);
            }
            if let Some(score) = config.get("score").and_then(Value::as_i64) {
                lead.score = score as i32;
            }
            lead.last_activity_at = Utc::now();
            state
                .storage
                .update_lead(&lead)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "lead": lead }))
        }
        "lead.score" => {
            let id = uuid_field(config, "lead_id")?;
            let mut lead = state
                .storage
                .get_lead(id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "lead not found".to_string())?;
            let conv = if let Some(chat) = lead.chat_id.as_deref() {
                state
                    .storage
                    .get_conversation_by_chat(run.id, chat)
                    .await
                    .map_err(|e| e.to_string())?
            } else {
                None
            };
            let intents: Vec<_> = conv
                .as_ref()
                .map(|c| {
                    c.messages
                        .iter()
                        .map(|m| classify_intent(&m.text))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec![classify_intent("")]);
            let (score, reasons) = score_lead_signals(&intents);
            let intent = intents.last().copied().unwrap_or(boarddo_shared::v2::ConversationIntent::Unknown);
            lead.score = score;
            lead.score_reasons = reasons;
            lead.stage = stage_from_score(score, intent);
            lead.last_activity_at = Utc::now();
            state
                .storage
                .update_lead(&lead)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "lead": lead }))
        }
        "conversation.classify" => {
            let text = config.get("text").and_then(Value::as_str).unwrap_or("");
            let intent = classify_intent(text);
            Ok(json!({ "intent": intent.as_str() }))
        }
        "marketing.simulate_inbound" => {
            let text = config
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("Interested — what's the price?");
            let chat_id = config
                .get("chat_id")
                .and_then(Value::as_str)
                .unwrap_or("demo-chat-1");
            let experiment_id = config
                .get("experiment_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok());
            let (lead, conv) = super::marketing::process_inbound(
                state,
                spec,
                run,
                chat_id,
                None,
                text,
                experiment_id,
                true,
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({
                "ok": true,
                "simulated": true,
                "lead": lead,
                "conversation": conv
            }))
        }
        "analytics.campaign_metrics" => {
            let snap = state
                .storage
                .marketing_snapshot(spec.id, run.id, &spec.constraints)
                .await
                .map_err(|e| e.to_string())?;
            let experiments = state
                .storage
                .list_experiments(run.id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({
                "ok": true,
                "funnel": snap.funnel,
                "experiments": experiments,
                "strategy_deltas": snap.strategy_deltas
            }))
        }
        other => Err(format!("unknown lead tool `{other}`")),
    }
}
