//! Scripted + LLM planners that emit a [`TickPlan`].

use boarddo_shared::v2::{
    Experiment, ExperimentDecision, ExperimentUpdate, GoalRun, GoalSpec, PlannedToolCall, TickPlan,
};
use serde_json::{Value, json};

use crate::integrations::openai::{OpenAiEndpoint, chat_complete};
use crate::state::SharedState;
use crate::connections::ConnectionProvider;

pub fn scripted_plan(spec: &GoalSpec, run: &GoalRun, experiments: &[Experiment]) -> TickPlan {
    match spec.agent_type_id.as_str() {
        "agent.research" => research_plan(spec, run, experiments),
        "agent.monitoring" => monitoring_plan(spec, run, experiments),
        "agent.scenario" => scenario_plan(spec, run),
        "agent.marketing" => {
            // Async marketing plan is preferred; sync fallback for unit tests.
            marketing_plan_sync(spec, run, experiments)
        }
        _ => growth_plan(spec, run, experiments),
    }
}

pub async fn plan(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    experiments: &[Experiment],
    memory_summary: &str,
) -> TickPlan {
    if spec.agent_type_id == "agent.marketing" {
        // Marketing loop is intentionally scripted (structure not LLM-chosen).
        return super::marketing::marketing_plan(state, spec, run, experiments).await;
    }
    if let Some(plan) = llm_plan(state, spec, run, experiments, memory_summary).await {
        return plan;
    }
    scripted_plan(spec, run, experiments)
}

fn growth_plan(spec: &GoalSpec, run: &GoalRun, experiments: &[Experiment]) -> TickPlan {
    let tick = run.tick_count;
    let remaining = (spec.target - run.current).max(0.0);
    let mut plan = TickPlan {
        observe_notes: format!(
            "Progress {:.0}/{:.0}. {} experiments. Remaining {:.0}.",
            run.current,
            spec.target,
            experiments.len(),
            remaining
        ),
        think: String::new(),
        tool_calls: Vec::new(),
        experiment_updates: Vec::new(),
        strategy_deltas: Default::default(),
        new_hypotheses: Vec::new(),
    };

    if tick == 0 {
        plan.think = "Goal initialized. Researching target audience: indie hackers, small SaaS, automation users, developers.".into();
        plan.new_hypotheses = vec![
            "Developer communities convert better than generic automation forums.".into(),
            "SaaS founder groups show higher qualified interest.".into(),
            "Telegram communities need approval-gated, non-spam outreach.".into(),
            "Problem-led content outperforms feature dumps.".into(),
        ];
        plan.tool_calls.push(PlannedToolCall {
            id: "search-audience".into(),
            type_id: "web.search".into(),
            config: json!({
                "query": format!("{} workflow automation indie hackers", product_name(spec)),
                "limit": 8
            }),
        });
        plan.tool_calls.push(PlannedToolCall {
            id: "mem-audience".into(),
            type_id: "memory.write".into(),
            config: json!({
                "key": "audience",
                "kind": "long",
                "value": ["indie hackers", "small SaaS founders", "automation enthusiasts", "developers"]
            }),
        });
        plan.strategy_deltas = boarddo_shared::v2::default_growth_strategy();
        return plan;
    }

    if experiments.is_empty() {
        plan.think = "No experiments yet — seed hypotheses and start safe research.".into();
        plan.new_hypotheses.push(
            "Public research + content drafts beat cold spam for BoardDo awareness.".into(),
        );
        plan.tool_calls.push(PlannedToolCall {
            id: "log-seed".into(),
            type_id: "debug.log".into(),
            config: json!({ "message": format!("Seeding experiments for {}", spec.title) }),
        });
        return plan;
    }

    // Score experiments with recorded proxies; double down on the strongest.
    let mut best: Option<&Experiment> = None;
    let mut best_score = -1.0;
    for exp in experiments {
        let score = metric_f64(&exp.metrics, "conversion").unwrap_or(0.0);
        if score > best_score {
            best_score = score;
            best = Some(exp);
        }
        if !exp.metrics.is_null() && exp.metrics != json!({}) {
            plan.experiment_updates.push(ExperimentUpdate {
                id: Some(exp.id),
                number: Some(exp.number),
                metrics: None,
                decision: Some(if score >= 0.04 {
                    ExperimentDecision::Continue
                } else if tick > 2 {
                    ExperimentDecision::Pivot
                } else {
                    ExperimentDecision::Pending
                }),
            });
        }
    }

    if let Some(win) = best {
        plan.think = format!(
            "H{} currently strongest. Reduce weak channels, generate a content variation, keep outreach behind approval.",
            win.number
        );
        let mut strategy = boarddo_shared::v2::default_growth_strategy();
        if win.hypothesis.to_lowercase().contains("saas") {
            strategy.insert("community".into(), json!(0.55));
            strategy.insert("content".into(), json!(0.25));
            strategy.insert("outreach".into(), json!(0.05));
            strategy.insert("search".into(), json!(0.15));
        } else if win.hypothesis.to_lowercase().contains("developer") {
            strategy.insert("community".into(), json!(0.35));
            strategy.insert("content".into(), json!(0.4));
            strategy.insert("search".into(), json!(0.2));
            strategy.insert("outreach".into(), json!(0.05));
        }
        plan.strategy_deltas = strategy;
    } else {
        plan.think = "Continue parallel safe experiments; no clear winner yet.".into();
    }

    plan.tool_calls.push(PlannedToolCall {
        id: format!("content-{tick}"),
        type_id: "debug.log".into(),
        config: json!({
            "message": format!(
                "Draft content variation for {}: goal-driven agents vs workflow builders. Tick {}.",
                product_name(spec),
                tick
            )
        }),
    });

    if tick % 3 == 0 {
        plan.tool_calls.push(PlannedToolCall {
            id: format!("search-{tick}"),
            type_id: "web.search".into(),
            config: json!({
                "query": format!("{} AI workflow agent communities", product_name(spec)),
                "limit": 5
            }),
        });
    }

    if tick >= 2 {
        plan.tool_calls.push(PlannedToolCall {
            id: format!("outreach-draft-{tick}"),
            type_id: "telegram.user.send_message".into(),
            config: json!({
                "text": format!(
                    "Hi — building {} as a goal-driven agent runtime (not another Zapier). Would this be useful for your stack?",
                    product_name(spec)
                ),
                "account_id": "pending-approval",
                "chat_id": "pending-approval"
            }),
        });
    }

    plan
}

fn scenario_plan(spec: &GoalSpec, run: &GoalRun) -> TickPlan {
    let tick = run.tick_count;
    if tick == 0 {
        return TickPlan {
            observe_notes: format!("No scenario yet for `{}`.", spec.title),
            think: "Propose a plan, materialize a test workflow, validate and run it.".into(),
            tool_calls: vec![
                PlannedToolCall {
                    id: "propose".into(),
                    type_id: "goal.propose_plan".into(),
                    config: json!({}),
                },
                PlannedToolCall {
                    id: "materialize".into(),
                    type_id: "goal.materialize_plan".into(),
                    config: json!({ "validate": true, "run": true, "wait": true }),
                },
                PlannedToolCall {
                    id: "mem".into(),
                    type_id: "memory.write".into(),
                    config: json!({
                        "key": "bootstrap",
                        "kind": "long",
                        "value": format!("Created and tested a stub scenario for {}", spec.title)
                    }),
                },
            ],
            experiment_updates: vec![],
            strategy_deltas: Default::default(),
            new_hypotheses: vec![
                "A tiny validate+run loop proves the goal can be automated.".into(),
            ],
        };
    }
    TickPlan {
        observe_notes: format!("Scenario tick {tick} — inspect last test run."),
        think: "Read linked execution results and store findings.".into(),
        tool_calls: vec![PlannedToolCall {
            id: format!("mem-tick-{tick}"),
            type_id: "memory.write".into(),
            config: json!({
                "key": format!("tick-{tick}"),
                "kind": "short",
                "value": format!("Tick {tick} observe for {}", spec.text)
            }),
        }],
        experiment_updates: vec![],
        strategy_deltas: Default::default(),
        new_hypotheses: vec![],
    }
}

fn research_plan(spec: &GoalSpec, run: &GoalRun, _experiments: &[Experiment]) -> TickPlan {
    TickPlan {
        observe_notes: format!("Research tick {} for {}", run.tick_count, spec.title),
        think: "Search, fetch nothing unsafe, store a finding.".into(),
        tool_calls: vec![
            PlannedToolCall {
                id: format!("rs-{}", run.tick_count),
                type_id: "web.search".into(),
                config: json!({ "query": spec.text, "limit": 8 }),
            },
            PlannedToolCall {
                id: format!("rm-{}", run.tick_count),
                type_id: "memory.write".into(),
                config: json!({
                    "key": format!("finding-{}", run.tick_count),
                    "kind": "long",
                    "value": format!("Research pass {} on {}", run.tick_count, spec.text)
                }),
            },
        ],
        experiment_updates: vec![],
        strategy_deltas: Default::default(),
        new_hypotheses: if run.tick_count == 0 {
            vec!["Primary sources beat aggregators.".into()]
        } else {
            vec![]
        },
    }
}

fn monitoring_plan(spec: &GoalSpec, run: &GoalRun, _experiments: &[Experiment]) -> TickPlan {
    TickPlan {
        observe_notes: format!("Monitor tick {} — looking for deltas.", run.tick_count),
        think: "Check public web/GitHub; log only if something changed.".into(),
        tool_calls: vec![PlannedToolCall {
            id: format!("mon-{}", run.tick_count),
            type_id: "web.search".into(),
            config: json!({ "query": format!("{} news OR release", spec.text), "limit": 5 }),
        }],
        experiment_updates: vec![],
        strategy_deltas: Default::default(),
        new_hypotheses: vec![],
    }
}

/// Sync fallback used by unit tests that cannot await storage.
fn marketing_plan_sync(spec: &GoalSpec, run: &GoalRun, experiments: &[Experiment]) -> TickPlan {
    let product = product_name(spec);
    if run.tick_count == 0 || experiments.is_empty() {
        return TickPlan {
            observe_notes: format!("Marketing bootstrap for {product}"),
            think: "Research → audience → 3 hypotheses.".into(),
            tool_calls: vec![
                PlannedToolCall {
                    id: "mkt-search".into(),
                    type_id: "research.search".into(),
                    config: json!({ "query": format!("{product} audience"), "limit": 5 }),
                },
                PlannedToolCall {
                    id: "mkt-audience".into(),
                    type_id: "research.audience".into(),
                    config: json!({ "product": product }),
                },
            ],
            experiment_updates: vec![],
            strategy_deltas: boarddo_shared::v2::default_marketing_strategy(),
            new_hypotheses: vec![
                format!("{product} saves time."),
                format!("{product} increases leads."),
                format!("{product} removes manual work."),
            ],
        };
    }
    TickPlan {
        observe_notes: format!("Marketing tick {}", run.tick_count),
        think: "Continue marketing loop.".into(),
        tool_calls: vec![],
        experiment_updates: vec![],
        strategy_deltas: Default::default(),
        new_hypotheses: vec![],
    }
}

fn product_name(spec: &GoalSpec) -> &str {
    spec.constraints
        .get("product")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("BoardDo")
}

fn metric_f64(metrics: &Value, key: &str) -> Option<f64> {
    metrics.get(key).and_then(|v| v.as_f64().or_else(|| v.as_u64().map(|n| n as f64)))
}

async fn llm_plan(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    experiments: &[Experiment],
    memory_summary: &str,
) -> Option<TickPlan> {
    let conn_id = spec.openai_connection_id?;
    let resolved = state.connections.resolve(&conn_id.to_string()).await.ok()?;
    let api_key = resolved
        .credentials
        .get("api_key")
        .or_else(|| resolved.credentials.get("token"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_string();
    let base_url = resolved
        .config
        .get("base_url")
        .or_else(|| resolved.config.get("api_base"))
        .and_then(Value::as_str)
        .unwrap_or("https://api.openai.com/v1")
        .trim_end_matches('/')
        .to_string();
    let model = resolved
        .config
        .get("default_model")
        .and_then(Value::as_str)
        .unwrap_or("gpt-4o-mini")
        .to_string();
    let endpoint = OpenAiEndpoint {
        api_key,
        base_url,
        default_model: model.clone(),
    };

    let tools: Vec<&str> = spec.tools.iter().map(|t| t.type_id.as_str()).collect();
    let system = format!(
        "You are {}. Output JSON only with keys: observe_notes, think, tool_calls, new_hypotheses, strategy_deltas.\n\
tool_calls is an array of {{id, type_id, config}}. Allowed type_id: {}.\n\
You may also use workflow.* / scenario.apply_ops / goal.propose_plan / goal.materialize_plan when those type_ids are allowed.\n\
Honor policy: max {} actions/day, budget ${}, require_approval {:?}. Never spam. Never invent spend. Prefer build→validate→test over live side-effects.",
        spec.agent_type_id,
        tools.join(", "),
        spec.policy.max_actions_per_day,
        spec.policy.budget_usd,
        spec.policy.require_approval
    );
    let user = format!(
        "Goal: {}\nMetric {} = {:.0}/{:.0}\nTick {}\nMemory:\n{}\nExperiments:\n{}\nInstructions:\n{}",
        spec.text,
        spec.metric,
        run.current,
        spec.target,
        run.tick_count,
        memory_summary,
        serde_json::to_string(experiments).unwrap_or_default(),
        spec.instructions
    );

    let (text, _, _) = chat_complete(&endpoint, &model, &system, &user, 0.2, 1200, 20_000)
        .await
        .ok()?;
    parse_tick_plan(&text)
}

fn parse_tick_plan(raw: &str) -> Option<TickPlan> {
    let json_str = extract_json(raw)?;
    serde_json::from_str(&json_str).ok()
}

fn extract_json(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        return Some(trimmed.to_string());
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end > start {
        Some(trimmed[start..=end].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::{CapabilityBudget, PolicyBundle};
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn growth_tick0_seeds_hypotheses() {
        let now = Utc::now();
        let spec = GoalSpec {
            id: Uuid::now_v7(),
            title: "Leads".into(),
            text: "Get 10 users".into(),
            metric: "leads_interested".into(),
            target: 10.0,
            deadline: None,
            constraints: json!({"product": "BoardDo"}),
            budget: CapabilityBudget::default(),
            policy: PolicyBundle::default(),
            agent_type_id: "agent.growth".into(),
            instructions: String::new(),
            tools: vec![],
            tick_interval_secs: 60,
            openai_connection_id: None,
            created_at: now,
            updated_at: now,
        };
        let run = GoalRun {
            id: Uuid::now_v7(),
            goal_id: spec.id,
            status: boarddo_shared::v2::GoalRunStatus::Running,
            current: 0.0,
            target: 10.0,
            strategy: Default::default(),
            actions_today: 0,
            actions_day: now.format("%Y-%m-%d").to_string(),
            tick_count: 0,
            last_tick_at: None,
            next_tick_at: None,
            last_observe: None,
            last_think: None,
            error: None,
            started_at: now,
            finished_at: None,
        };
        let plan = scripted_plan(&spec, &run, &[]);
        assert!(!plan.new_hypotheses.is_empty());
        assert!(plan.tool_calls.iter().any(|c| c.type_id == "web.search"));
    }
}
