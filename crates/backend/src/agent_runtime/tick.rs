//! One Observe → Think → Plan → Act → Evaluate tick.

use boarddo_shared::v2::{
    AgentApproval, AgentAuditEntry, Experiment, ExperimentDecision, GoalRun, GoalRunStatus,
    GoalRuntimeEvent, GoalSpec, PlannedToolCall, capability_for_tool, tool_counts_as_action,
};
use boarddo_shared::v2::ApprovalDecision;
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use super::policy::{PolicyDecision, check_tool};
use super::tools::{ToolGateway, today};
use crate::state::SharedState;

pub async fn run_tick(state: &SharedState, spec: GoalSpec, mut run: GoalRun) -> anyhow::Result<()> {
    if spec.policy.kill_switch {
        run.status = GoalRunStatus::Stopped;
        run.error = Some("kill switch".into());
        run.finished_at = Some(Utc::now());
        state.storage.update_goal_run(&run).await?;
        state.emit_goal(GoalRuntimeEvent::StatusChanged {
            run_id: run.id,
            status: run.status,
        });
        return Ok(());
    }

    if let Some(deadline) = spec.deadline
        && Utc::now() > deadline
        && run.current < spec.target
    {
        run.status = GoalRunStatus::Failed;
        run.error = Some("deadline passed".into());
        run.finished_at = Some(Utc::now());
        state.storage.update_goal_run(&run).await?;
        state.emit_goal(GoalRuntimeEvent::StatusChanged {
            run_id: run.id,
            status: run.status,
        });
        return Ok(());
    }

    let pending = state.storage.count_pending_approvals(run.id).await?;
    if pending > 0 {
        run.status = GoalRunStatus::WaitingApproval;
        run.next_tick_at = Some(Utc::now() + Duration::seconds(spec.tick_interval_secs as i64));
        state.storage.update_goal_run(&run).await?;
        return Ok(());
    } else if run.status == GoalRunStatus::WaitingApproval {
        run.status = GoalRunStatus::Running;
    }

    let tick = run.tick_count;
    state.emit_goal(GoalRuntimeEvent::TickStarted {
        run_id: run.id,
        goal_id: spec.id,
        tick,
    });
    audit(
        state,
        run.id,
        "tick.started",
        Some(&format!("tick {tick}")),
        None,
    )
    .await?;

    let experiments = state.storage.list_experiments(run.id).await?;
    let memory = state.storage.list_memory(run.id).await?;
    let mut memory_summary = memory
        .iter()
        .take(12)
        .map(|m| format!("{}={}", m.key, m.value))
        .collect::<Vec<_>>()
        .join("\n");

    let exec_notes = observe_executions(state, &run).await.unwrap_or_default();
    if !exec_notes.is_empty() {
        memory_summary = format!("{memory_summary}\n{exec_notes}");
    }

    let plan = super::planner::plan(state, &spec, &run, &experiments, &memory_summary).await;

    run.last_observe = Some(plan.observe_notes.clone());
    run.last_think = Some(plan.think.clone());
    state.emit_goal(GoalRuntimeEvent::Observe {
        run_id: run.id,
        notes: plan.observe_notes.clone(),
    });
    state.emit_goal(GoalRuntimeEvent::Think {
        run_id: run.id,
        summary: plan.think.clone(),
    });
    audit(state, run.id, "observe", Some(&plan.observe_notes), None).await?;
    audit(state, run.id, "think", Some(&plan.think), None).await?;

    for hypo in &plan.new_hypotheses {
        let number = state.storage.next_experiment_number(run.id).await?;
        let now = Utc::now();
        let exp = Experiment {
            id: Uuid::now_v7(),
            run_id: run.id,
            number,
            hypothesis: hypo.clone(),
            actions: json!([]),
            metrics: json!({}),
            decision: ExperimentDecision::Pending,
            created_at: now,
            updated_at: now,
        };
        state.storage.insert_experiment(&exp).await?;
        audit(
            state,
            run.id,
            "experiment.created",
            Some(hypo),
            Some(json!({ "number": number })),
        )
        .await?;
    }

    for upd in &plan.experiment_updates {
        if let Some(id) = upd.id {
            let mut list = state.storage.list_experiments(run.id).await?;
            if let Some(exp) = list.iter_mut().find(|e| e.id == id) {
                if let Some(m) = &upd.metrics {
                    exp.metrics = m.clone();
                }
                if let Some(d) = upd.decision {
                    exp.decision = d;
                }
                exp.updated_at = Utc::now();
                state.storage.update_experiment(exp).await?;
            }
        }
    }

    if !plan.strategy_deltas.is_empty() {
        run.strategy = plan.strategy_deltas.clone();
    }

    state.emit_goal(GoalRuntimeEvent::Plan {
        run_id: run.id,
        tool_count: plan.tool_calls.len() as u32,
    });

    let gateway = ToolGateway::new();
    let day = today();
    if run.actions_day != day {
        run.actions_day = day;
        run.actions_today = 0;
    }

    for call in &plan.tool_calls {
        match check_tool(&spec, &run, call) {
            PolicyDecision::Deny(reason) => {
                state.emit_goal(GoalRuntimeEvent::Act {
                    run_id: run.id,
                    tool_type: call.type_id.clone(),
                    ok: false,
                    message: reason.clone(),
                });
                audit(
                    state,
                    run.id,
                    "act.denied",
                    Some(&reason),
                    Some(json!({ "type_id": call.type_id })),
                )
                .await?;
            }
            PolicyDecision::NeedsApproval { capability } => {
                queue_approval(state, &spec, &run, call, &capability).await?;
                run.status = GoalRunStatus::WaitingApproval;
            }
            PolicyDecision::Allow => {
                match gateway.execute(state, &spec, &run, call).await {
                    Ok(output) => {
                        if tool_counts_as_action(&call.type_id) {
                            run.actions_today += 1;
                        }
                        let msg = summarize_output(&call.type_id, &output);
                        state.emit_goal(GoalRuntimeEvent::Act {
                            run_id: run.id,
                            tool_type: call.type_id.clone(),
                            ok: true,
                            message: msg.clone(),
                        });
                        audit(
                            state,
                            run.id,
                            "act",
                            Some(&msg),
                            Some(json!({ "type_id": call.type_id, "output": output })),
                        )
                        .await?;
                    }
                    Err(err) => {
                        if tool_counts_as_action(&call.type_id) {
                            run.actions_today += 1;
                        }
                        state.emit_goal(GoalRuntimeEvent::Act {
                            run_id: run.id,
                            tool_type: call.type_id.clone(),
                            ok: false,
                            message: err.clone(),
                        });
                        audit(
                            state,
                            run.id,
                            "act.failed",
                            Some(&err),
                            Some(json!({ "type_id": call.type_id })),
                        )
                        .await?;
                    }
                }
            }
        }
        if run.status == GoalRunStatus::WaitingApproval {
            break;
        }
    }

    apply_execution_metric(state, &spec, &mut run).await?;

    if run.current >= spec.target {
        run.status = GoalRunStatus::Succeeded;
        run.finished_at = Some(Utc::now());
    } else if run.status == GoalRunStatus::Running {
        run.status = GoalRunStatus::Running;
    }

    run.tick_count += 1;
    run.last_tick_at = Some(Utc::now());
    run.next_tick_at = Some(Utc::now() + Duration::seconds(spec.tick_interval_secs.max(5) as i64));
    run.target = spec.target;
    state.storage.update_goal_run(&run).await?;

    state.emit_goal(GoalRuntimeEvent::Evaluate {
        run_id: run.id,
        current: run.current,
        target: spec.target,
        status: run.status,
    });
    audit(
        state,
        run.id,
        "evaluate",
        Some(&format!("{:.0}/{:.0} {}", run.current, spec.target, run.status.as_str())),
        None,
    )
    .await?;

    Ok(())
}

async fn queue_approval(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    call: &PlannedToolCall,
    capability: &str,
) -> anyhow::Result<()> {
    let approval = AgentApproval {
        id: Uuid::now_v7(),
        run_id: run.id,
        goal_id: spec.id,
        title: format!("Approve {}", call.type_id),
        description: Some(format!("Capability `{capability}` requires approval.")),
        capability: capability.into(),
        tool_type_id: call.type_id.clone(),
        payload: json!({ "config": call.config, "id": call.id }),
        status: ApprovalDecision::Pending,
        created_at: Utc::now(),
        resolved_at: None,
    };
    state.storage.insert_approval(&approval).await?;
    state.emit_goal(GoalRuntimeEvent::ApprovalRequested {
        run_id: run.id,
        approval_id: approval.id,
        title: approval.title.clone(),
    });
    audit(
        state,
        run.id,
        "approval.requested",
        Some(&approval.title),
        Some(json!({ "approval_id": approval.id, "capability": capability })),
    )
    .await?;
    let _ = capability_for_tool(&call.type_id);
    Ok(())
}

pub async fn execute_approved_tool(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    call: &PlannedToolCall,
) -> Result<Value, String> {
    ToolGateway::new().execute(state, spec, run, call).await
}

async fn audit(
    state: &SharedState,
    run_id: Uuid,
    kind: &str,
    message: Option<&str>,
    payload: Option<Value>,
) -> anyhow::Result<()> {
    state
        .storage
        .insert_audit(&AgentAuditEntry {
            id: Uuid::now_v7(),
            run_id,
            at: Utc::now(),
            kind: kind.into(),
            message: message.map(str::to_string),
            payload,
        })
        .await
}

fn summarize_output(type_id: &str, output: &Value) -> String {
    if let Some(msg) = output.get("message").and_then(Value::as_str) {
        return msg.chars().take(240).collect();
    }
    if let Some(n) = output.get("results").and_then(Value::as_array) {
        return format!("{type_id}: {} results", n.len());
    }
    format!("{type_id} ok")
}

async fn observe_executions(state: &SharedState, run: &GoalRun) -> anyhow::Result<String> {
    let ids = state.storage.list_goal_run_workflow_ids(run.id).await?;
    if ids.is_empty() {
        return Ok(String::new());
    }
    let mut lines = Vec::new();
    for wf_id in ids {
        let execs = state
            .storage
            .list_executions_for_workflow(wf_id, 3)
            .await?;
        if execs.is_empty() {
            lines.push(format!("workflow {wf_id}: no executions yet"));
            continue;
        }
        for ex in execs {
            lines.push(format!(
                "workflow {wf_id} exec {} status={:?}",
                ex.id, ex.status
            ));
        }
    }
    let summary = lines.join("\n");
    let _ = state
        .storage
        .upsert_memory(run.id, "last_executions", &json!(summary), "short")
        .await;
    Ok(summary)
}

async fn apply_execution_metric(
    state: &SharedState,
    spec: &GoalSpec,
    run: &mut GoalRun,
) -> anyhow::Result<()> {
    if spec.metric != "scenarios_validated" && spec.metric != "execution.succeeded_count" {
        return Ok(());
    }
    let ids = state.storage.list_goal_run_workflow_ids(run.id).await?;
    let mut scored = 0.0;
    for wf_id in ids {
        let execs = state
            .storage
            .list_executions_for_workflow(wf_id, 20)
            .await?;
        if execs.iter().any(|e| e.status == boarddo_shared::ExecutionStatus::Completed) {
            scored += 1.0;
        }
    }
    run.current = scored;
    Ok(())
}
