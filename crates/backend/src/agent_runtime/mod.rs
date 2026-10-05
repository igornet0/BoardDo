//! Goal-driven agent runtime (control plane). SmartDo remains the tool executor.

mod marketing;
mod planner;
mod policy;
mod supervisor;
mod tick;
mod tools;

pub use supervisor::spawn_arc;
pub use tick::execute_approved_tool;
pub use marketing::process_inbound;

#[cfg(test)]
mod tests;

use boarddo_shared::v2::{
    AutonomyLevel, CampaignBrief, CampaignCreatedResponse, CampaignPlan, CampaignProduct,
    CreateCampaignRequest, CreateGoalRequest, GoalRun, GoalRunStatus, GoalSpec, GoalRuntimeEvent,
    default_growth_strategy,
};
use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

use crate::state::SharedState;

pub fn apply_template(mut req: CreateGoalRequest) -> CreateGoalRequest {
    if let Some(tid) = req.template_id.clone() {
        if let Some(t) = boarddo_shared::v2::templates::get(&tid) {
            if req.title.trim().is_empty() {
                req.title = t.title.clone();
            }
            if req.text.trim().is_empty() {
                req.text = t.description.clone();
            }
            if req.instructions.trim().is_empty() {
                req.instructions = t.instructions.clone();
            }
            if req.tools.is_empty() {
                req.tools = t.tools.clone();
            }
            if req.budget.allow.is_empty() && req.budget.deny.is_empty() {
                req.budget = t.budget.clone();
            }
            if req.agent_type_id == "agent.growth" || req.agent_type_id.is_empty() {
                req.agent_type_id = t.agent_type_id.clone();
            }
            if req.metric == "leads_interested" {
                req.metric = t.metric.clone();
            }
            req.policy.max_actions_per_day = t.policy.max_actions_per_day;
            if req.policy.require_approval.is_empty() {
                req.policy.require_approval = t.policy.require_approval.clone();
            }
            if req.policy.budget_usd == 0.0 && t.policy.budget_usd > 0.0 {
                req.policy.budget_usd = t.policy.budget_usd;
            }
        }
    }
    req
}

pub fn spec_from_request(req: CreateGoalRequest) -> GoalSpec {
    let mut req = apply_template(req);
    if req.tools.is_empty() || (req.budget.allow.is_empty() && req.budget.deny.is_empty()) {
        if let Some(t) = boarddo_shared::v2::templates::get(&req.agent_type_id) {
            if req.tools.is_empty() {
                req.tools = t.tools.clone();
            }
            if req.budget.allow.is_empty() && req.budget.deny.is_empty() {
                req.budget = t.budget.clone();
            }
            if req.instructions.trim().is_empty() {
                req.instructions = t.instructions.clone();
            }
        }
    }
    let now = Utc::now();
    let deadline = req.deadline.or_else(|| Some(now + Duration::days(7)));
    let level = boarddo_shared::v2::autonomy_from_constraints(&req.constraints);
    req.policy = policy_for_autonomy(req.policy, level);
    GoalSpec {
        id: Uuid::now_v7(),
        title: if req.title.trim().is_empty() {
            "Untitled goal".into()
        } else {
            req.title
        },
        text: req.text,
        metric: req.metric,
        target: req.target,
        deadline,
        constraints: if req.constraints.is_null() {
            json!({ "product": "BoardDo" })
        } else {
            req.constraints
        },
        budget: req.budget,
        policy: req.policy,
        agent_type_id: req.agent_type_id,
        instructions: req.instructions,
        tools: req.tools,
        tick_interval_secs: req.tick_interval_secs.unwrap_or(60).max(5),
        openai_connection_id: req.openai_connection_id,
        created_at: now,
        updated_at: now,
    }
}

/// Map autonomy level onto require_approval + daily spend defaults.
pub fn policy_for_autonomy(
    mut policy: boarddo_shared::v2::PolicyBundle,
    level: AutonomyLevel,
) -> boarddo_shared::v2::PolicyBundle {
    match level.0 {
        1 | 2 => {
            if !policy
                .require_approval
                .iter()
                .any(|c| c == boarddo_shared::v2::caps::TELEGRAM_WRITE || c == "telegram.write")
            {
                policy
                    .require_approval
                    .push(boarddo_shared::v2::caps::TELEGRAM_WRITE.into());
            }
        }
        3 => {
            policy.require_approval.retain(|c| {
                c != boarddo_shared::v2::caps::TELEGRAM_WRITE
                    && c != "telegram.write"
                    && c != "publisher.telegram.send"
            });
            for cap in AutonomyLevel::full_autonomous_require_approval() {
                if !policy.require_approval.contains(&cap) {
                    policy.require_approval.push(cap);
                }
            }
            if policy.max_daily_spend_usd <= 0.0 && policy.budget_usd > 0.0 {
                policy.max_daily_spend_usd = (policy.budget_usd / 15.0).max(1.0);
            }
        }
        _ => {
            policy.require_approval = AutonomyLevel::full_autonomous_require_approval();
            if policy.max_daily_spend_usd <= 0.0 && policy.budget_usd > 0.0 {
                policy.max_daily_spend_usd = (policy.budget_usd / 15.0).max(1.0);
            }
        }
    }
    policy
}

pub async fn create_campaign(
    state: &SharedState,
    req: CreateCampaignRequest,
) -> anyhow::Result<CampaignCreatedResponse> {
    let level = AutonomyLevel::from_mode(&req.autonomy);
    let brief = CampaignBrief {
        product: CampaignProduct {
            name: if req.product_name.trim().is_empty() {
                "Untitled product".into()
            } else {
                req.product_name.trim().to_string()
            },
            description: req.product_description,
            url: req.product_url.filter(|u| !u.trim().is_empty()),
        },
        goal_metric: if req.goal_metric.trim().is_empty() {
            "leads_interested".into()
        } else {
            req.goal_metric
        },
        target: req.target.max(1.0),
        budget_usd: req.budget_usd.max(0.0),
        market: if req.market.trim().is_empty() {
            "global".into()
        } else {
            req.market
        },
        deadline_days: req.deadline_days.max(1),
        autonomy: level.mode_label().into(),
    };

    let constraints = json!({
        "product": {
            "name": brief.product.name,
            "description": brief.product.description,
            "url": brief.product.url,
        },
        "product_name": brief.product.name,
        "goal_metric": brief.goal_metric,
        "target": brief.target,
        "budget_usd": brief.budget_usd,
        "budget": brief.budget_usd,
        "market": brief.market,
        "deadline_days": brief.deadline_days,
        "autonomy": brief.autonomy,
        "autonomy_level": level.0,
        "demo": req.demo,
        "channel": "telegram",
        "campaign": true,
    });

    let create = CreateGoalRequest {
        title: format!("Campaign: {}", brief.product.name),
        text: format!(
            "Promote {} in {} and reach {:.0} {} within {} days. Budget ${:.0}.\n{}",
            brief.product.name,
            brief.market,
            brief.target,
            brief.goal_metric,
            brief.deadline_days,
            brief.budget_usd,
            brief.product.description
        ),
        metric: brief.goal_metric.clone(),
        target: brief.target,
        deadline: Some(Utc::now() + Duration::days(brief.deadline_days as i64)),
        constraints,
        budget: Default::default(),
        policy: boarddo_shared::v2::PolicyBundle {
            max_actions_per_day: 80,
            budget_usd: brief.budget_usd,
            max_daily_spend_usd: if brief.budget_usd > 0.0 {
                (brief.budget_usd / 15.0).max(1.0)
            } else {
                0.0
            },
            require_approval: vec![],
            allowed_domains: vec![],
            blocked_domains: vec![],
            kill_switch: false,
        },
        agent_type_id: "agent.marketing".into(),
        instructions: String::new(),
        tools: vec![],
        tick_interval_secs: Some(60),
        openai_connection_id: req.openai_connection_id,
        template_id: Some("marketing".into()),
    };

    let mut spec = spec_from_request(create);
    state.storage.insert_goal_spec(&spec).await?;

    let now = Utc::now();
    let run = GoalRun {
        id: Uuid::now_v7(),
        goal_id: spec.id,
        status: GoalRunStatus::Draft,
        current: 0.0,
        target: spec.target,
        strategy: boarddo_shared::v2::default_marketing_strategy(),
        actions_today: 0,
        actions_day: now.format("%Y-%m-%d").to_string(),
        tick_count: 0,
        last_tick_at: None,
        next_tick_at: None,
        last_observe: Some("Campaign created — awaiting AI Plan review.".into()),
        last_think: Some("Building campaign plan from product brief.".into()),
        error: None,
        started_at: now,
        finished_at: None,
    };
    state.storage.insert_goal_run(&run).await?;

    let plan = CampaignPlan::bootstrap_from_brief(&brief);
    let mem_entries = [
        ("campaign.brief", json!(brief), "long"),
        ("campaign.plan", json!(plan), "long"),
        ("campaign.learnings", json!([]), "long"),
        ("marketing.stage", json!("research"), "long"),
        (
            "current_objective",
            json!("Review AI Plan and press Start Campaign"),
            "short",
        ),
    ];
    for (key, value, kind) in mem_entries {
        state
            .storage
            .upsert_memory(run.id, key, &value, kind)
            .await?;
    }

    state.emit_goal(GoalRuntimeEvent::StatusChanged {
        run_id: run.id,
        status: run.status,
    });

    if spec.openai_connection_id.is_none() {
        if let Ok(list) = state.storage.list_connections_by_type("openai").await {
            if let Some(c) = list.into_iter().find(|c| c.enabled) {
                spec.openai_connection_id = Some(c.id);
                spec.updated_at = Utc::now();
                state.storage.update_goal_spec(&spec).await?;
            }
        }
    }

    Ok(CampaignCreatedResponse { spec, run, plan })
}

pub async fn start_run(
    state: &SharedState,
    spec: &mut GoalSpec,
    openai_connection_id: Option<Uuid>,
) -> anyhow::Result<GoalRun> {
    if let Some(id) = openai_connection_id {
        spec.openai_connection_id = Some(id);
        spec.updated_at = Utc::now();
        state.storage.update_goal_spec(spec).await?;
    } else if spec.openai_connection_id.is_none() {
        if let Ok(list) = state.storage.list_connections_by_type("openai").await {
            if let Some(c) = list.into_iter().find(|c| c.enabled) {
                spec.openai_connection_id = Some(c.id);
                spec.updated_at = Utc::now();
                state.storage.update_goal_spec(spec).await?;
            }
        }
    }

    if let Some(mut existing) = state.storage.latest_run_for_goal(spec.id).await? {
        if existing.status.is_active() {
            return Ok(existing);
        }
        if matches!(
            existing.status,
            GoalRunStatus::Draft | GoalRunStatus::Paused | GoalRunStatus::Stopped
        ) {
            let now = Utc::now();
            existing.status = GoalRunStatus::Running;
            existing.next_tick_at = Some(now);
            existing.finished_at = None;
            existing.error = None;
            existing.last_think = Some("Campaign started — beginning research loop.".into());
            existing.last_observe = Some("User approved AI Plan; runtime active.".into());
            if existing.strategy.is_empty() {
                existing.strategy = if spec.agent_type_id == "agent.growth" {
                    default_growth_strategy()
                } else if spec.agent_type_id == "agent.marketing" {
                    boarddo_shared::v2::default_marketing_strategy()
                } else {
                    Default::default()
                };
            }
            state.storage.update_goal_run(&existing).await?;
            let _ = state
                .storage
                .upsert_memory(
                    existing.id,
                    "current_objective",
                    &json!("Research product, audience, and competitors"),
                    "short",
                )
                .await;
            state.emit_goal(GoalRuntimeEvent::StatusChanged {
                run_id: existing.id,
                status: existing.status,
            });
            return Ok(existing);
        }
    }

    let now = Utc::now();
    let run = GoalRun {
        id: Uuid::now_v7(),
        goal_id: spec.id,
        status: GoalRunStatus::Running,
        current: 0.0,
        target: spec.target,
        strategy: if spec.agent_type_id == "agent.growth" {
            default_growth_strategy()
        } else if spec.agent_type_id == "agent.marketing" {
            boarddo_shared::v2::default_marketing_strategy()
        } else {
            Default::default()
        },
        actions_today: 0,
        actions_day: now.format("%Y-%m-%d").to_string(),
        tick_count: 0,
        last_tick_at: None,
        next_tick_at: Some(now),
        last_observe: None,
        last_think: None,
        error: None,
        started_at: now,
        finished_at: None,
    };
    state.storage.insert_goal_run(&run).await?;
    state.emit_goal(GoalRuntimeEvent::StatusChanged {
        run_id: run.id,
        status: run.status,
    });
    Ok(run)
}

pub async fn pause_run(state: &SharedState, run: &mut GoalRun) -> anyhow::Result<()> {
    run.status = GoalRunStatus::Paused;
    run.next_tick_at = None;
    state.storage.update_goal_run(run).await?;
    state.emit_goal(GoalRuntimeEvent::StatusChanged {
        run_id: run.id,
        status: run.status,
    });
    Ok(())
}

pub async fn stop_run(state: &SharedState, run: &mut GoalRun) -> anyhow::Result<()> {
    run.status = GoalRunStatus::Stopped;
    run.finished_at = Some(Utc::now());
    state.storage.update_goal_run(run).await?;
    state.emit_goal(GoalRuntimeEvent::StatusChanged {
        run_id: run.id,
        status: run.status,
    });
    Ok(())
}

pub async fn record_lead(state: &SharedState, spec: &GoalSpec, delta: f64) -> anyhow::Result<GoalRun> {
    let mut run = state
        .storage
        .latest_run_for_goal(spec.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no active run"))?;
    run.current += delta;
    if run.current >= spec.target {
        run.status = GoalRunStatus::Succeeded;
        run.finished_at = Some(Utc::now());
    }
    state.storage.update_goal_run(&run).await?;
    state.emit_goal(GoalRuntimeEvent::LeadRecorded {
        run_id: run.id,
        current: run.current,
    });
    Ok(run)
}

pub async fn get_campaign_plan(
    state: &SharedState,
    run_id: Uuid,
) -> anyhow::Result<Option<CampaignPlan>> {
    Ok(state
        .storage
        .get_memory(run_id, "campaign.plan")
        .await?
        .and_then(|m| serde_json::from_value(m.value).ok()))
}
