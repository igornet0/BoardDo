//! Goal / GoalRun / approvals / templates API (separate from editor `/api/agent`).

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use boarddo_shared::v2::{
    AgentApproval, CampaignCreatedResponse, CampaignPlan, CreateCampaignRequest, CreateGoalRequest,
    GoalDetail, GoalRun, GoalSpec, PlannedToolCall, RecordGoalEventRequest, StartGoalRequest,
    UpdateGoalRequest, templates,
};
use boarddo_shared::v2::ApprovalDecision;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::agent_runtime;
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/goals", get(list_goals).post(create_goal))
        .route("/goals/campaigns", post(create_campaign))
        .route(
            "/goals/{id}",
            get(get_goal).put(update_goal).delete(delete_goal),
        )
        .route("/goals/{id}/start", post(start_goal))
        .route("/goals/{id}/stop", post(stop_goal))
        .route("/goals/{id}/pause", post(pause_goal))
        .route("/goals/{id}/plan", get(get_plan))
        .route("/goals/{id}/events", post(record_event))
        .route("/goal-runs", get(list_runs))
        .route("/goal-runs/{id}", get(get_run))
        .route("/goal-runs/{id}/activity", get(activity))
        .route("/goal-runs/{id}/experiments", get(experiments))
        .route("/goal-runs/{id}/memory", get(memory))
        .route("/goal-runs/{id}/marketing", get(marketing_snapshot))
        .route("/goal-runs/{id}/content-engine", get(content_engine_snapshot))
        .route("/goal-runs/{id}/plan", get(get_run_plan))
        .route("/approvals", get(list_approvals))
        .route("/approvals/{id}/approve", post(approve))
        .route("/approvals/{id}/reject", post(reject))
        .route("/agent-templates", get(list_templates))
}

fn internal(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = %err, "goals api error");
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

async fn list_goals(
    State(state): State<SharedState>,
) -> Result<Json<Vec<GoalDetail>>, (StatusCode, String)> {
    let specs = state.storage.list_goal_specs().await.map_err(internal)?;
    let mut out = Vec::new();
    for spec in specs {
        let run = state
            .storage
            .latest_run_for_goal(spec.id)
            .await
            .map_err(internal)?;
        let linked_workflows = linked_workflows_for(&state, run.as_ref())
            .await
            .map_err(internal)?;
        out.push(GoalDetail {
            spec,
            run,
            linked_workflows,
        });
    }
    Ok(Json(out))
}

async fn create_goal(
    State(state): State<SharedState>,
    Json(req): Json<CreateGoalRequest>,
) -> Result<(StatusCode, Json<GoalSpec>), (StatusCode, String)> {
    let spec = agent_runtime::spec_from_request(req);
    state.storage.insert_goal_spec(&spec).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(spec)))
}

async fn create_campaign(
    State(state): State<SharedState>,
    Json(req): Json<CreateCampaignRequest>,
) -> Result<(StatusCode, Json<CampaignCreatedResponse>), (StatusCode, String)> {
    let created = agent_runtime::create_campaign(&state, req)
        .await
        .map_err(internal)?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn get_plan(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CampaignPlan>, (StatusCode, String)> {
    let run = state
        .storage
        .latest_run_for_goal(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no run".into()))?;
    let plan = agent_runtime::get_campaign_plan(&state, run.id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "plan not found".into()))?;
    Ok(Json(plan))
}

async fn get_run_plan(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CampaignPlan>, (StatusCode, String)> {
    let plan = agent_runtime::get_campaign_plan(&state, id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "plan not found".into()))?;
    Ok(Json(plan))
}

async fn get_goal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GoalDetail>, (StatusCode, String)> {
    let spec = state
        .storage
        .get_goal_spec(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "goal not found".into()))?;
    let run = state
        .storage
        .latest_run_for_goal(id)
        .await
        .map_err(internal)?;
    let linked_workflows = linked_workflows_for(&state, run.as_ref())
        .await
        .map_err(internal)?;
    Ok(Json(GoalDetail {
        spec,
        run,
        linked_workflows,
    }))
}

async fn update_goal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateGoalRequest>,
) -> Result<Json<GoalSpec>, (StatusCode, String)> {
    let mut spec = state
        .storage
        .get_goal_spec(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "goal not found".into()))?;
    spec.title = req.title;
    spec.text = req.text;
    spec.metric = req.metric;
    spec.target = req.target;
    spec.deadline = req.deadline;
    spec.constraints = req.constraints;
    spec.budget = req.budget;
    spec.policy = req.policy;
    spec.agent_type_id = req.agent_type_id;
    spec.instructions = req.instructions;
    spec.tools = req.tools;
    spec.tick_interval_secs = req.tick_interval_secs.max(5);
    spec.openai_connection_id = req.openai_connection_id;
    spec.updated_at = Utc::now();
    state.storage.update_goal_spec(&spec).await.map_err(internal)?;
    Ok(Json(spec))
}

async fn delete_goal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let ok = state.storage.delete_goal_spec(id).await.map_err(internal)?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "goal not found".into()))
    }
}

async fn start_goal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    body: Option<Json<StartGoalRequest>>,
) -> Result<Json<GoalRun>, (StatusCode, String)> {
    let mut spec = state
        .storage
        .get_goal_spec(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "goal not found".into()))?;
    let openai = body.and_then(|Json(b)| b.openai_connection_id);
    let run = agent_runtime::start_run(&state, &mut spec, openai)
        .await
        .map_err(internal)?;
    Ok(Json(run))
}

async fn stop_goal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GoalRun>, (StatusCode, String)> {
    let mut run = state
        .storage
        .latest_run_for_goal(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no run".into()))?;
    agent_runtime::stop_run(&state, &mut run)
        .await
        .map_err(internal)?;
    Ok(Json(run))
}

async fn pause_goal(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GoalRun>, (StatusCode, String)> {
    let mut run = state
        .storage
        .latest_run_for_goal(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no run".into()))?;
    agent_runtime::pause_run(&state, &mut run)
        .await
        .map_err(internal)?;
    Ok(Json(run))
}

async fn record_event(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<RecordGoalEventRequest>,
) -> Result<Json<GoalRun>, (StatusCode, String)> {
    let spec = state
        .storage
        .get_goal_spec(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "goal not found".into()))?;
    match req.event_type.as_str() {
        "lead.interested" => {
            let delta = req
                .payload
                .get("delta")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let run = agent_runtime::record_lead(&state, &spec, delta)
                .await
                .map_err(internal)?;
            Ok(Json(run))
        }
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("unsupported event `{other}`"),
        )),
    }
}

async fn list_runs(
    State(state): State<SharedState>,
) -> Result<Json<Vec<GoalRun>>, (StatusCode, String)> {
    state
        .storage
        .list_goal_runs()
        .await
        .map(Json)
        .map_err(internal)
}

async fn get_run(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GoalRun>, (StatusCode, String)> {
    match state.storage.get_goal_run(id).await.map_err(internal)? {
        Some(r) => Ok(Json(r)),
        None => Err((StatusCode::NOT_FOUND, "run not found".into())),
    }
}

async fn activity(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = state
        .storage
        .list_audit(id, 80)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "entries": items })))
}

async fn experiments(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = state.storage.list_experiments(id).await.map_err(internal)?;
    Ok(Json(json!({ "experiments": items })))
}

async fn memory(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = state.storage.list_memory(id).await.map_err(internal)?;
    Ok(Json(json!({ "memory": items })))
}

async fn content_engine_snapshot(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let run = state
        .storage
        .get_goal_run(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "run not found".into()))?;
    let spec = state
        .storage
        .get_goal_spec(run.goal_id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "goal not found".into()))?;
    let snap = state
        .storage
        .content_engine_snapshot(spec.id, run.id)
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::to_value(snap).unwrap_or(json!({}))))
}

async fn marketing_snapshot(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let run = state
        .storage
        .get_goal_run(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "run not found".into()))?;
    let spec = state
        .storage
        .get_goal_spec(run.goal_id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "goal not found".into()))?;
    let snap = state
        .storage
        .marketing_snapshot(spec.id, run.id, &spec.constraints)
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::to_value(snap).unwrap_or(json!({}))))
}

#[derive(Deserialize)]
struct ApprovalQuery {
    run_id: Option<Uuid>,
    pending: Option<bool>,
}

async fn list_approvals(
    State(state): State<SharedState>,
    Query(q): Query<ApprovalQuery>,
) -> Result<Json<Vec<AgentApproval>>, (StatusCode, String)> {
    state
        .storage
        .list_approvals(q.run_id, q.pending.unwrap_or(false))
        .await
        .map(Json)
        .map_err(internal)
}

async fn approve(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AgentApproval>, (StatusCode, String)> {
    resolve_approval(state, id, ApprovalDecision::Approved).await
}

async fn reject(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AgentApproval>, (StatusCode, String)> {
    resolve_approval(state, id, ApprovalDecision::Rejected).await
}

async fn resolve_approval(
    state: SharedState,
    id: Uuid,
    decision: ApprovalDecision,
) -> Result<Json<AgentApproval>, (StatusCode, String)> {
    let mut a = state
        .storage
        .get_approval(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "approval not found".into()))?;
    if a.status != ApprovalDecision::Pending {
        return Err((StatusCode::CONFLICT, "already resolved".into()));
    }
    a.status = decision;
    a.resolved_at = Some(Utc::now());
    state.storage.update_approval(&a).await.map_err(internal)?;

    if decision == ApprovalDecision::Approved {
        if let (Some(spec), Some(run)) = (
            state.storage.get_goal_spec(a.goal_id).await.map_err(internal)?,
            state.storage.get_goal_run(a.run_id).await.map_err(internal)?,
        ) {
            let call = PlannedToolCall {
                id: a
                    .payload
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("approved")
                    .to_string(),
                type_id: a.tool_type_id.clone(),
                config: a.payload.get("config").cloned().unwrap_or(json!({})),
            };
            match agent_runtime::execute_approved_tool(&state, &spec, &run, &call).await {
                Ok(_) => {
                    if call.type_id == "publisher.telegram.send" {
                        if let Some(cid) = call
                            .config
                            .get("content_id")
                            .and_then(Value::as_str)
                            .and_then(|s| Uuid::parse_str(s).ok())
                        {
                            if let Ok(Some(mut draft)) = state.storage.get_content_draft(cid).await {
                                if draft.status
                                    == boarddo_shared::v2::ContentDraftStatus::PendingApproval
                                {
                                    // send tool already published; keep status if published
                                } else if draft.status
                                    == boarddo_shared::v2::ContentDraftStatus::Draft
                                {
                                    draft.status =
                                        boarddo_shared::v2::ContentDraftStatus::Approved;
                                    draft.updated_at = Utc::now();
                                    let _ = state.storage.update_content_draft(&draft).await;
                                }
                            }
                        }
                    }
                    let mut run = run;
                    run.status = boarddo_shared::v2::GoalRunStatus::Running;
                    run.next_tick_at = Some(Utc::now());
                    let _ = state.storage.update_goal_run(&run).await;
                }
                Err(err) => tracing::warn!(error = %err, "approved tool failed"),
            }
        }
    } else if let Some(mut run) = state
        .storage
        .get_goal_run(a.run_id)
        .await
        .map_err(internal)?
    {
        run.status = boarddo_shared::v2::GoalRunStatus::Running;
        run.next_tick_at = Some(Utc::now());
        let _ = state.storage.update_goal_run(&run).await;
    }

    Ok(Json(a))
}

async fn list_templates() -> Json<Value> {
    Json(json!({ "templates": templates::all() }))
}

async fn linked_workflows_for(
    state: &SharedState,
    run: Option<&GoalRun>,
) -> anyhow::Result<Vec<boarddo_shared::WorkflowSummary>> {
    let Some(run) = run else {
        return Ok(Vec::new());
    };
    let ids = state.storage.list_goal_run_workflow_ids(run.id).await?;
    let mut out = Vec::new();
    for id in ids {
        if let Some(wf) = state.storage.get_workflow(id).await? {
            out.push(boarddo_shared::WorkflowSummary {
                id: wf.id,
                name: wf.name,
                description: wf.description,
                version: wf.version,
                status: wf.status,
                created_at: wf.created_at,
                updated_at: wf.updated_at,
            });
        }
    }
    Ok(out)
}
