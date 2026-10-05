//! Scripted marketing planner + inbound lead processing.

use boarddo_shared::v2::{
    ContentDraftStatus, ConversationIntent, ConversationMessage, Experiment, ExperimentDecision,
    ExperimentUpdate, GoalRun, GoalSpec, Lead, LeadStage, Learning, MarketingConversation,
    MarketingStage, PlannedToolCall, StrategyDelta, TickPlan, autonomy_from_constraints,
    classify_intent, default_marketing_strategy, market_from_constraints, product_from_constraints,
    product_url_from_constraints, score_lead_signals, stage_from_score,
};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::state::SharedState;

pub async fn marketing_plan(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    experiments: &[Experiment],
) -> TickPlan {
    let product = product_from_constraints(&spec.constraints);
    let stage = current_stage(state, run.id).await;
    let notes = state
        .storage
        .list_research_notes(run.id)
        .await
        .unwrap_or_default();
    let drafts = state
        .storage
        .list_content_drafts(run.id)
        .await
        .unwrap_or_default();
    let leads = state
        .storage
        .list_leads(run.id, true)
        .await
        .unwrap_or_default();
    let pending = state
        .storage
        .count_pending_approvals(run.id)
        .await
        .unwrap_or(0);

    let mut plan = TickPlan {
        observe_notes: format!(
            "Marketing stage={}. Notes={}, hypotheses={}, drafts={}, leads={}. Progress {:.0}/{:.0}.",
            stage.as_str(),
            notes.len(),
            experiments.len(),
            drafts.len(),
            leads.len(),
            run.current,
            spec.target
        ),
        think: String::new(),
        tool_calls: Vec::new(),
        experiment_updates: Vec::new(),
        strategy_deltas: Default::default(),
        new_hypotheses: Vec::new(),
    };

    match stage {
        MarketingStage::Research => {
            let market = market_from_constraints(&spec.constraints);
            let url = product_url_from_constraints(&spec.constraints);
            plan.think = format!(
                "Research pass for {product} ({market}): live web search{} then structured notes.",
                if url.is_some() {
                    " + product URL fetch"
                } else {
                    ""
                }
            );
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-obj".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "current_objective",
                    "kind": "short",
                    "value": format!("Research {product} in {market}")
                }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-research-search".into(),
                type_id: "research.search".into(),
                config: json!({
                    "query": format!("{product} {market} competitors audience problems"),
                    "limit": 8
                }),
            });
            if let Some(u) = url {
                plan.tool_calls.push(PlannedToolCall {
                    id: "mkt-research-fetch".into(),
                    type_id: "research.fetch".into(),
                    config: json!({ "url": u }),
                });
            }
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-research-competitors".into(),
                type_id: "research.competitors".into(),
                config: json!({ "product": product, "market": market }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-strategy".into(),
                type_id: "strategy.create".into(),
                config: json!({}),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-stage-audience".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "marketing.stage",
                    "kind": "long",
                    "value": MarketingStage::Audience.as_str()
                }),
            });
        }
        MarketingStage::Audience => {
            plan.think = "Classify audience segments and pains from research notes.".into();
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-audience".into(),
                type_id: "research.audience".into(),
                config: json!({ "product": product }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-pains".into(),
                type_id: "research.pains".into(),
                config: json!({ "product": product }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-ideas".into(),
                type_id: "idea.generate".into(),
                config: json!({ "count": 5 }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-stage-hyp".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "marketing.stage",
                    "kind": "long",
                    "value": MarketingStage::Hypotheses.as_str()
                }),
            });
        }
        MarketingStage::Hypotheses => {
            if experiments.len() < 3 {
                plan.think = "Seed three marketing hypotheses with attribution IDs.".into();
                let deltas = latest_strategy_delta(state, run.id).await;
                let (h1, h2, h3) = hypotheses_for(&product, deltas.as_ref());
                plan.new_hypotheses = vec![h1, h2, h3];
                plan.strategy_deltas = default_marketing_strategy();
            } else {
                plan.think = "Hypotheses ready — move to content drafts.".into();
            }
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-stage-content".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "marketing.stage",
                    "kind": "long",
                    "value": MarketingStage::Content.as_str()
                }),
            });
        }
        MarketingStage::Content => {
            let use_llm = spec.openai_connection_id.is_some();
            plan.think = if use_llm {
                "Create Telegram draft variants (LLM-assisted when available).".into()
            } else {
                "Create one Telegram draft variant per hypothesis.".into()
            };
            let exps = if experiments.is_empty() {
                state
                    .storage
                    .list_experiments(run.id)
                    .await
                    .unwrap_or_default()
            } else {
                experiments.to_vec()
            };
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-obj-content".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "current_objective",
                    "kind": "short",
                    "value": "Produce experiment content drafts"
                }),
            });
            for exp in exps.iter().take(3) {
                let already = drafts.iter().any(|d| d.experiment_id == Some(exp.id));
                if already {
                    continue;
                }
                if use_llm {
                    plan.tool_calls.push(PlannedToolCall {
                        id: format!("mkt-ai-draft-{}", exp.number),
                        type_id: "ai.chat".into(),
                        config: json!({
                            "prompt": format!(
                                "Write a short Telegram marketing post for {product}. Hypothesis: {}. Include a clear CTA (reply INTERESTED).",
                                exp.hypothesis
                            )
                        }),
                    });
                }
                plan.tool_calls.push(PlannedToolCall {
                    id: format!("mkt-draft-{}", exp.number),
                    type_id: "content.create_draft".into(),
                    config: json!({
                        "experiment_id": exp.id.to_string(),
                        "channel": "telegram",
                        "angle": angle_for_hypothesis(&exp.hypothesis),
                        "body": draft_body(&product, &exp.hypothesis, exp.number)
                    }),
                });
            }
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-stage-approval".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "marketing.stage",
                    "kind": "long",
                    "value": MarketingStage::Approval.as_str()
                }),
            });
        }
        MarketingStage::Approval => {
            let draft_pending: Vec<_> = drafts
                .iter()
                .filter(|d| {
                    matches!(
                        d.status,
                        ContentDraftStatus::Draft | ContentDraftStatus::PendingApproval
                    )
                })
                .collect();
            if pending > 0 {
                plan.think =
                    "Waiting for user approval of Telegram publishes (autonomy level 2).".into();
            } else if !draft_pending.is_empty() {
                plan.think =
                    "Prepare drafts and request approval before Telegram publish.".into();
                for d in draft_pending.into_iter().take(3) {
                    if d.status == ContentDraftStatus::Draft {
                        plan.tool_calls.push(PlannedToolCall {
                            id: format!("mkt-prep-{}", d.id),
                            type_id: "publisher.telegram.prepare".into(),
                            config: json!({
                                "content_id": d.id.to_string(),
                                "text": d.body,
                                "experiment_id": d.experiment_id.map(|id| id.to_string()),
                            }),
                        });
                    }
                    plan.tool_calls.push(PlannedToolCall {
                        id: format!("mkt-send-approve-{}", d.id),
                        type_id: "publisher.telegram.send".into(),
                        config: json!({
                            "content_id": d.id.to_string(),
                            "experiment_id": d.experiment_id.map(|id| id.to_string()),
                        }),
                    });
                }
            } else {
                plan.think = "Drafts approved/published — continue loop.".into();
                plan.tool_calls.push(PlannedToolCall {
                    id: "mkt-stage-publish".into(),
                    type_id: "memory.write".into(),
                    config: json!({
                        "key": "marketing.stage",
                        "kind": "long",
                        "value": MarketingStage::Publish.as_str()
                    }),
                });
            }
        }
        MarketingStage::Publish => {
            plan.think = "Publish approved drafts with experiment attribution (idempotent).".into();
            let approved: Vec<_> = drafts
                .iter()
                .filter(|d| d.status == ContentDraftStatus::Approved)
                .collect();
            if approved.is_empty()
                && drafts
                    .iter()
                    .any(|d| d.status == ContentDraftStatus::Published)
            {
                plan.think = "Posts already published — move to engage.".into();
                plan.tool_calls.push(PlannedToolCall {
                    id: "mkt-stage-engage".into(),
                    type_id: "memory.write".into(),
                    config: json!({
                        "key": "marketing.stage",
                        "kind": "long",
                        "value": MarketingStage::Engage.as_str()
                    }),
                });
            } else {
                for d in approved.into_iter().take(3) {
                    plan.tool_calls.push(PlannedToolCall {
                        id: format!("mkt-send-{}", d.id),
                        type_id: "publisher.telegram.send".into(),
                        config: json!({
                            "content_id": d.id.to_string(),
                            "experiment_id": d.experiment_id.map(|id| id.to_string()),
                        }),
                    });
                }
                plan.tool_calls.push(PlannedToolCall {
                    id: "mkt-stage-engage".into(),
                    type_id: "memory.write".into(),
                    config: json!({
                        "key": "marketing.stage",
                        "kind": "long",
                        "value": MarketingStage::Engage.as_str()
                    }),
                });
            }
        }
        MarketingStage::Engage => {
            plan.think =
                "Monitor inbound Telegram; demo mode can simulate interested replies.".into();
            let demo = spec
                .constraints
                .get("demo")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || spec
                    .constraints
                    .get("demo_mode")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            if demo && leads.is_empty() {
                let exp_id = experiments.first().map(|e| e.id);
                plan.tool_calls.push(PlannedToolCall {
                    id: "mkt-sim-inbound".into(),
                    type_id: "marketing.simulate_inbound".into(),
                    config: json!({
                        "text": format!(
                            "Interested in {product} — we waste a lot of time on manual lead processing. What's the price?"
                        ),
                        "chat_id": "demo-chat-1",
                        "experiment_id": exp_id.map(|id| id.to_string()),
                    }),
                });
            }
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-stage-lead".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "marketing.stage",
                    "kind": "long",
                    "value": MarketingStage::Lead.as_str()
                }),
            });
        }
        MarketingStage::Lead => {
            plan.think = "Ensure leads are scored and linked to experiments.".into();
            for lead in leads.iter().take(5) {
                plan.tool_calls.push(PlannedToolCall {
                    id: format!("mkt-score-{}", lead.id),
                    type_id: "lead.score".into(),
                    config: json!({ "lead_id": lead.id.to_string() }),
                });
            }
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-stage-analytics".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "marketing.stage",
                    "kind": "long",
                    "value": MarketingStage::Analytics.as_str()
                }),
            });
        }
        MarketingStage::Analytics => {
            plan.think =
                "Attribute campaign metrics; decide Continue / Pivot / Stop per experiment.".into();
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-obj-analytics".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "current_objective",
                    "kind": "short",
                    "value": "Analyze experiment performance and decide pivots"
                }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-metrics".into(),
                type_id: "analytics.campaign_metrics".into(),
                config: json!({}),
            });
            let mut scored: Vec<(Uuid, u64, u64, String)> = Vec::new();
            for exp in experiments {
                let leads_n = leads
                    .iter()
                    .filter(|l| !l.simulated && l.experiment_id == Some(exp.id))
                    .count() as u64;
                let posts = drafts
                    .iter()
                    .filter(|d| {
                        d.experiment_id == Some(exp.id)
                            && d.status == ContentDraftStatus::Published
                    })
                    .count() as u64;
                let qualified = leads
                    .iter()
                    .filter(|l| {
                        !l.simulated
                            && l.experiment_id == Some(exp.id)
                            && matches!(l.stage, LeadStage::Qualified | LeadStage::Converted)
                    })
                    .count() as u64;
                let views = posts * 10;
                let ctr_proxy = if views > 0 {
                    (leads_n as f64) / (views as f64)
                } else {
                    0.0
                };
                let decision = if qualified > 0 || ctr_proxy >= 0.08 {
                    ExperimentDecision::Continue
                } else if posts > 0 && leads_n == 0 {
                    ExperimentDecision::Pivot
                } else if posts > 0 && ctr_proxy < 0.02 {
                    ExperimentDecision::Stop
                } else {
                    ExperimentDecision::Pending
                };
                scored.push((exp.id, leads_n + qualified * 2, posts, exp.hypothesis.clone()));
                plan.experiment_updates.push(ExperimentUpdate {
                    id: Some(exp.id),
                    number: Some(exp.number),
                    metrics: Some(json!({
                        "posts": posts,
                        "leads": leads_n,
                        "qualified_leads": qualified,
                        "replies": leads_n,
                        "views": views,
                        "reactions": posts * 2,
                        "conversions": 0,
                        "ctr_proxy": ctr_proxy,
                        "channel": "telegram"
                    })),
                    decision: Some(decision),
                });
            }
            // Seed a follow-up hypothesis from the weakest channel when pivoting.
            if let Some((_, _, posts, hyp)) = scored.iter().min_by_key(|s| s.1) {
                if *posts > 0 {
                    plan.new_hypotheses.push(format!(
                        "Pivot from '{}': try problem-first hooks with stronger CTA for {product}.",
                        hyp
                    ));
                }
            }
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-stage-learn".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "marketing.stage",
                    "kind": "long",
                    "value": MarketingStage::Learn.as_str()
                }),
            });
        }
        MarketingStage::Learn => {
            plan.think =
                "Record durable learning, update strategy weights, prepare next iteration.".into();
            let (best, observation, action, confidence) = learn_from(experiments, &leads);
            let delta = StrategyDelta {
                experiment_id: best,
                observation: observation.clone(),
                action: action.clone(),
                confidence,
                created_at: Utc::now(),
            };
            let learning = Learning {
                id: Uuid::now_v7(),
                at: Utc::now(),
                observation: observation.clone(),
                decision: action.clone(),
                reason: format!("Confidence {confidence:.2} from experiment attribution"),
                confidence,
                experiment_id: best,
            };
            let mut strategy = run.strategy.clone();
            if strategy.is_empty() {
                strategy = default_marketing_strategy();
            }
            // Down-weight weak channels / up-weight winners.
            for exp in experiments {
                let decision = exp.decision;
                let angle = angle_for_hypothesis(&exp.hypothesis);
                let bump = match decision {
                    ExperimentDecision::Continue => 0.15,
                    ExperimentDecision::Pivot => -0.05,
                    ExperimentDecision::Stop => -0.12,
                    ExperimentDecision::Pending => 0.0,
                };
                if bump != 0.0 {
                    if let Some(v) = strategy.get_mut(&angle) {
                        if let Some(n) = v.as_f64() {
                            *v = json!((n + bump).clamp(0.05, 0.9));
                        }
                    } else if bump > 0.0 {
                        strategy.insert(angle, json!(0.5));
                    }
                }
            }
            if let Some(exp) = experiments.iter().find(|e| Some(e.id) == best) {
                let angle = angle_for_hypothesis(&exp.hypothesis);
                if let Some(v) = strategy.get_mut(&angle) {
                    if let Some(n) = v.as_f64() {
                        *v = json!((n + 0.1).min(0.9));
                    }
                }
            }
            // Boost telegram if it outperforms (primary channel in MVP).
            if let Some(v) = strategy.get_mut("telegram") {
                if let Some(n) = v.as_f64() {
                    *v = json!((n + 0.05).min(0.85));
                }
            }
            plan.strategy_deltas = strategy;
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-delta".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": format!("strategy_delta.{}", run.tick_count),
                    "kind": "long",
                    "value": delta
                }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-delta-latest".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "strategy_delta",
                    "kind": "long",
                    "value": delta
                }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-learning".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": format!("learning.{}", run.tick_count),
                    "kind": "long",
                    "value": learning
                }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-obj-learn".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "current_objective",
                    "kind": "short",
                    "value": action
                }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-stage-next".into(),
                type_id: "memory.write".into(),
                config: json!({
                    "key": "marketing.stage",
                    "kind": "long",
                    "value": MarketingStage::Research.as_str()
                }),
            });
            plan.tool_calls.push(PlannedToolCall {
                id: "mkt-log-learn".into(),
                type_id: "debug.log".into(),
                config: json!({
                    "message": format!("learning: {observation} → {action} (conf={confidence:.2})")
                }),
            });
        }
    }

    plan
}

fn hypotheses_for(product: &str, delta: Option<&StrategyDelta>) -> (String, String, String) {
    if let Some(d) = delta {
        if d.action.to_lowercase().contains("time") {
            return (
                format!("{product} saves significant time vs manual workflows."),
                format!("{product} increases qualified inbound leads."),
                format!("{product} removes repetitive manual lead processing."),
            );
        }
    }
    (
        format!("{product} saves time for small teams running automations."),
        format!("{product} increases leads by turning goals into live campaigns."),
        format!("{product} removes repetitive manual work from marketing ops."),
    )
}

fn angle_for_hypothesis(hypothesis: &str) -> String {
    let h = hypothesis.to_lowercase();
    if h.contains("time") || h.contains("save") {
        "time_saving".into()
    } else if h.contains("lead") {
        "more_leads".into()
    } else {
        "manual_work".into()
    }
}

fn draft_body(product: &str, hypothesis: &str, number: u32) -> String {
    format!(
        "H{number}: {hypothesis}\n\n\
         If you're tired of glueing tools together, {product} runs the full marketing loop \
         from research to Telegram to leads — zero ad budget.\n\n\
         Reply INTERESTED if you want a walkthrough."
    )
}

fn learn_from(experiments: &[Experiment], leads: &[Lead]) -> (Option<Uuid>, String, String, f32) {
    let mut best: Option<(Uuid, u64, String)> = None;
    for exp in experiments {
        let q = leads
            .iter()
            .filter(|l| {
                !l.simulated
                    && l.experiment_id == Some(exp.id)
                    && matches!(l.stage, LeadStage::Qualified | LeadStage::Interested | LeadStage::Converted)
            })
            .count() as u64;
        let from_metrics = exp
            .metrics
            .get("qualified_leads")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let score = q.max(from_metrics);
        if best.as_ref().map(|b| b.1).unwrap_or(0) <= score {
            best = Some((exp.id, score, exp.hypothesis.clone()));
        }
    }
    if let Some((id, score, hyp)) = best {
        let angle = angle_for_hypothesis(&hyp);
        return (
            Some(id),
            format!("Posts focused on '{hyp}' generated more qualified conversations ({score})."),
            format!("Increase weight of {angle} angle."),
            if score > 0 { 0.71 } else { 0.45 },
        );
    }
    (
        None,
        "Not enough attributed leads yet; keep parallel hypotheses.".into(),
        "Maintain balanced weights; gather more Telegram replies.".into(),
        0.4,
    )
}

async fn current_stage(state: &SharedState, run_id: Uuid) -> MarketingStage {
    state
        .storage
        .get_memory(run_id, "marketing.stage")
        .await
        .ok()
        .flatten()
        .and_then(|m| m.value.as_str().map(MarketingStage::parse))
        .unwrap_or(MarketingStage::Research)
}

async fn latest_strategy_delta(state: &SharedState, run_id: Uuid) -> Option<StrategyDelta> {
    state
        .storage
        .get_memory(run_id, "strategy_delta")
        .await
        .ok()
        .flatten()
        .and_then(|m| serde_json::from_value(m.value).ok())
}

/// Process an inbound Telegram (or simulated) message into conversation + lead.
pub async fn process_inbound(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    chat_id: &str,
    account_id: Option<&str>,
    text: &str,
    experiment_id: Option<Uuid>,
    simulated: bool,
) -> anyhow::Result<(Lead, MarketingConversation)> {
    let intent = classify_intent(text);
    let (score, reasons) = score_lead_signals(&[intent]);
    let stage = stage_from_score(score, intent);
    let now = Utc::now();

    let mut lead = if let Some(existing) = state
        .storage
        .find_lead_by_chat(run.id, chat_id)
        .await?
    {
        let mut existing = existing;
        existing.score = score;
        existing.score_reasons = reasons.clone();
        existing.stage = stage;
        existing.last_activity_at = now;
        if existing.experiment_id.is_none() {
            existing.experiment_id = experiment_id;
        }
        state.storage.update_lead(&existing).await?;
        existing
    } else {
        let lead = Lead {
            id: Uuid::now_v7(),
            goal_id: spec.id,
            run_id: run.id,
            campaign_id: Some(run.id),
            experiment_id,
            source: if simulated {
                "simulated".into()
            } else {
                "telegram".into()
            },
            source_reference: Some(chat_id.into()),
            chat_id: Some(chat_id.into()),
            account_id: account_id.map(str::to_string),
            stage,
            score,
            score_reasons: reasons.clone(),
            first_seen_at: now,
            last_activity_at: now,
            metadata: Default::default(),
            simulated,
        };
        state.storage.insert_lead(&lead).await?;
        lead
    };

    let mut conv = state
        .storage
        .get_conversation_by_chat(run.id, chat_id)
        .await?
        .unwrap_or(MarketingConversation {
            id: Uuid::now_v7(),
            goal_id: spec.id,
            run_id: run.id,
            campaign_id: Some(run.id),
            lead_id: Some(lead.id),
            experiment_id: lead.experiment_id,
            chat_id: chat_id.into(),
            account_id: account_id.map(str::to_string),
            intent,
            messages: Vec::new(),
            last_action: None,
            next_action: Some("reply".into()),
            updated_at: now,
            simulated,
        });
    conv.lead_id = Some(lead.id);
    conv.intent = intent;
    conv.experiment_id = lead.experiment_id.or(experiment_id);
    conv.messages.push(ConversationMessage {
        role: "user".into(),
        text: text.into(),
        at: now,
        simulated,
    });
    conv.last_action = Some("inbound_received".into());
    conv.next_action = Some(match intent {
        ConversationIntent::Pricing | ConversationIntent::Buy => "send_pricing_reply".into(),
        ConversationIntent::Interested | ConversationIntent::Pain => "send_demo_reply".into(),
        ConversationIntent::NotInterested | ConversationIntent::Spam => "close".into(),
        _ => "clarify".into(),
    });
    conv.updated_at = now;
    state.storage.upsert_conversation(&conv).await?;

    // Bump goal metric from real (or demo-counted) interested+ leads.
    if !simulated
        && matches!(
            lead.stage,
            LeadStage::Interested | LeadStage::Qualified | LeadStage::Converted
        )
    {
        let _ = crate::agent_runtime::record_lead(state, spec, 1.0).await;
    } else if simulated
        && matches!(
            lead.stage,
            LeadStage::Interested | LeadStage::Qualified | LeadStage::Converted
        )
    {
        // Demo: still advance the run metric so the loop is visible, but lead is marked SIMULATED.
        let _ = crate::agent_runtime::record_lead(state, spec, 1.0).await;
        lead.metadata
            .insert("metric_note".into(), json!("SIMULATED lead counted for demo loop"));
        state.storage.update_lead(&lead).await?;
    }

    let _ = state
        .storage
        .insert_audit(&boarddo_shared::v2::AgentAuditEntry {
            id: Uuid::now_v7(),
            run_id: run.id,
            at: now,
            kind: if simulated {
                "telegram.message.received.simulated".into()
            } else {
                "telegram.message.received".into()
            },
            message: Some(text.into()),
            payload: Some(json!({
                "lead_id": lead.id,
                "intent": intent.as_str(),
                "score": score,
                "stage": lead.stage.as_str(),
                "simulated": simulated,
            })),
        })
        .await;

    Ok((lead, conv))
}

pub fn apply_autonomy_policy(spec: &GoalSpec, type_id: &str) -> Option<String> {
    let level = autonomy_from_constraints(&spec.constraints);
    if level.observe_only() && boarddo_shared::v2::is_outbound_tool(type_id) {
        return Some(format!(
            "autonomy level {} forbids outbound/publish tools",
            level.0
        ));
    }
    None
}
