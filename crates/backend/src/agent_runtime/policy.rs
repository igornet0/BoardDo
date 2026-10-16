//! Policy gate for Goal agent tool calls.

use boarddo_shared::v2::{
    GoalRun, GoalSpec, PlannedToolCall, RiskDecision, RiskRequest, autonomy_from_constraints,
    evaluate, lookup_tool, resolve_tool_alias, tool_counts_as_action,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny(String),
    NeedsApproval { capability: String },
}

pub fn check_tool(spec: &GoalSpec, run: &GoalRun, call: &PlannedToolCall) -> PolicyDecision {
    if spec.policy.kill_switch {
        return PolicyDecision::Deny("kill switch is on".into());
    }
    if run.status == boarddo_shared::v2::GoalRunStatus::Stopped {
        return PolicyDecision::Deny("run is stopped".into());
    }
    if let Some(deadline) = spec.deadline
        && chrono::Utc::now() > deadline
    {
        return PolicyDecision::Deny("deadline passed".into());
    }

    let canonical = resolve_tool_alias(&call.type_id).to_string();

    let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let used = if run.actions_day == day {
        run.actions_today
    } else {
        0
    };
    if lookup_tool(&canonical).is_some_and(|d| d.available)
        && tool_counts_as_action(&canonical)
        && used >= spec.policy.max_actions_per_day
    {
        return PolicyDecision::Deny(format!(
            "daily action cap reached ({})",
            spec.policy.max_actions_per_day
        ));
    }

    evaluate_for_goal(spec, &call.type_id, false).into()
}

/// Goal-scoped risk check for one tool id (no run/kill/deadline gates).
pub fn evaluate_for_goal(spec: &GoalSpec, type_id: &str, approved: bool) -> RiskDecision {
    let allowlist: Vec<String> = spec.tools.iter().map(|t| t.type_id.clone()).collect();
    evaluate_goal_request(spec, type_id, Some(&allowlist), approved)
}

/// Risk check for a node inside a goal's workflow. The goal allowlist names
/// agent-level tools, so it does not apply to nodes; capabilities, autonomy
/// and approvals do.
pub fn evaluate_node_for_goal(spec: &GoalSpec, type_id: &str, approved: bool) -> RiskDecision {
    evaluate_goal_request(spec, type_id, None, approved)
}

fn evaluate_goal_request(
    spec: &GoalSpec,
    type_id: &str,
    allowlist: Option<&[String]>,
    approved: bool,
) -> RiskDecision {
    evaluate(&RiskRequest {
        type_id,
        autonomy: autonomy_from_constraints(&spec.constraints),
        budget: &spec.budget,
        allowlist,
        require_approval: &spec.policy.require_approval,
        spend_gated: spec.policy.max_daily_spend_usd > 0.0,
        approved,
    })
}

impl From<RiskDecision> for PolicyDecision {
    fn from(d: RiskDecision) -> Self {
        match d {
            RiskDecision::Allow => PolicyDecision::Allow,
            RiskDecision::Deny(r) => PolicyDecision::Deny(r),
            RiskDecision::NeedsApproval { capability } => {
                PolicyDecision::NeedsApproval { capability }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::{CapabilityBudget, PolicyBundle, caps};
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    fn spec_run() -> (GoalSpec, GoalRun) {
        let now = Utc::now();
        let spec = GoalSpec {
            id: Uuid::now_v7(),
            title: "t".into(),
            text: "g".into(),
            metric: "leads_interested".into(),
            target: 10.0,
            deadline: None,
            constraints: json!({ "autonomy_level": 2 }),
            budget: CapabilityBudget::default()
                .grant(caps::WEB_SEARCH)
                .grant(caps::TELEGRAM_WRITE)
                .forbid(caps::ADS_LAUNCH),
            policy: PolicyBundle {
                max_actions_per_day: 2,
                budget_usd: 0.0,
                max_daily_spend_usd: 0.0,
                require_approval: vec![caps::TELEGRAM_WRITE.into()],
                allowed_domains: vec![],
                blocked_domains: vec![],
                kill_switch: false,
                replan: Default::default(),
            },
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
            replan: Default::default(),
        };
        (spec, run)
    }

    #[test]
    fn allows_web_search() {
        let (spec, run) = spec_run();
        let d = check_tool(
            &spec,
            &run,
            &PlannedToolCall {
                id: "1".into(),
                type_id: "web.search".into(),
                config: json!({}),
            },
        );
        assert_eq!(d, PolicyDecision::Allow);
    }

    #[test]
    fn telegram_needs_approval() {
        let (spec, run) = spec_run();
        let d = check_tool(
            &spec,
            &run,
            &PlannedToolCall {
                id: "1".into(),
                type_id: "telegram.user.send_message".into(),
                config: json!({}),
            },
        );
        assert!(matches!(d, PolicyDecision::NeedsApproval { .. }));
    }

    #[test]
    fn autonomy_3_skips_telegram_approval() {
        let (mut spec, run) = spec_run();
        spec.constraints = json!({ "autonomy_level": 3, "autonomy": "autonomous" });
        let d = check_tool(
            &spec,
            &run,
            &PlannedToolCall {
                id: "1".into(),
                type_id: "publisher.telegram.send".into(),
                config: json!({}),
            },
        );
        assert_eq!(d, PolicyDecision::Allow);
    }

    #[test]
    fn deny_when_cap_exceeded() {
        let (spec, mut run) = spec_run();
        run.actions_today = 2;
        let d = check_tool(
            &spec,
            &run,
            &PlannedToolCall {
                id: "1".into(),
                type_id: "web.search".into(),
                config: json!({}),
            },
        );
        assert!(matches!(d, PolicyDecision::Deny(_)));
    }

    #[test]
    fn deny_tool_not_on_allowlist() {
        let (mut spec, run) = spec_run();
        spec.budget = spec.budget.clone().grant(boarddo_shared::v2::caps::ARTIFACT_WRITE);
        spec.tools = vec![boarddo_shared::v2::AgentTool {
            type_id: "web.search".into(),
            title: None,
            config: json!({}),
        }];
        let d = check_tool(
            &spec,
            &run,
            &PlannedToolCall {
                id: "1".into(),
                type_id: "debug.log".into(),
                config: json!({}),
            },
        );
        assert!(matches!(d, PolicyDecision::Deny(_)));
    }

    #[test]
    fn alias_resolves_for_allowlist() {
        let (mut spec, run) = spec_run();
        spec.budget = spec
            .budget
            .clone()
            .grant(caps::ARTIFACT_WRITE)
            .grant(caps::TELEGRAM_WRITE);
        spec.tools = vec![boarddo_shared::v2::AgentTool {
            type_id: "publisher.telegram.send".into(),
            title: None,
            config: json!({}),
        }];
        let d = check_tool(
            &spec,
            &run,
            &PlannedToolCall {
                id: "1".into(),
                type_id: "telegram.publish".into(),
                config: json!({}),
            },
        );
        assert!(matches!(d, PolicyDecision::NeedsApproval { .. }));
    }

    fn call(type_id: &str) -> PlannedToolCall {
        PlannedToolCall {
            id: "1".into(),
            type_id: type_id.into(),
            config: json!({}),
        }
    }

    #[test]
    fn deny_unregistered_tool() {
        let (spec, run) = spec_run();
        let d = check_tool(&spec, &run, &call("totally.unknown"));
        assert_eq!(d, PolicyDecision::Deny("unregistered tool `totally.unknown`".into()));
    }

    #[test]
    fn deny_tool_without_executor() {
        let (mut spec, run) = spec_run();
        spec.budget = spec.budget.clone().grant(caps::AI_GENERATE);
        let d = check_tool(&spec, &run, &call("ai.generate"));
        assert_eq!(d, PolicyDecision::Deny("tool `ai.generate` has no executor".into()));
    }

    #[test]
    fn irreversible_needs_approval_even_when_autonomous() {
        let (mut spec, run) = spec_run();
        spec.constraints = json!({ "autonomy_level": 3, "autonomy": "autonomous" });
        spec.budget = spec.budget.clone().grant(caps::GITHUB_WRITE);
        spec.policy.require_approval.clear();
        let d = check_tool(&spec, &run, &call("projecthub.command"));
        assert_eq!(
            d,
            PolicyDecision::NeedsApproval {
                capability: "irreversible".into()
            }
        );
    }
}
