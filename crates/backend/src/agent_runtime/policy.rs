//! Policy gate for Goal agent tool calls.

use boarddo_shared::v2::{
    GoalRun, GoalSpec, PlannedToolCall, autonomy_from_constraints, capability_for_tool,
    resolve_tool_alias, tool_counts_as_action,
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

    if let Some(reason) = super::marketing::apply_autonomy_policy(spec, &canonical) {
        return PolicyDecision::Deny(reason);
    }

    let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let used = if run.actions_day == day {
        run.actions_today
    } else {
        0
    };
    if tool_counts_as_action(&canonical) && used >= spec.policy.max_actions_per_day {
        return PolicyDecision::Deny(format!(
            "daily action cap reached ({})",
            spec.policy.max_actions_per_day
        ));
    }

    let cap = capability_for_tool(&canonical);
    if !spec.budget.allows(cap) {
        return PolicyDecision::Deny(format!("capability `{cap}` not allowed"));
    }

    if !spec.tools.is_empty()
        && !spec.tools.iter().any(|t| {
            t.type_id == call.type_id
                || t.type_id == canonical
                || resolve_tool_alias(&t.type_id) == canonical
        })
    {
        return PolicyDecision::Deny(format!("tool `{}` is not in the goal allowlist", call.type_id));
    }

    let level = autonomy_from_constraints(&spec.constraints);

    // Level 3+: skip telegram.write approval unless listed as hard cap.
    let mut require = spec.policy.require_approval.clone();
    if level.allows_publish_without_approval() {
        require.retain(|c| {
            c != boarddo_shared::v2::caps::TELEGRAM_WRITE
                && c != "telegram.write"
                && c != "publisher.telegram.send"
                && c != "telegram.publish"
        });
        for hard in boarddo_shared::v2::AutonomyLevel::full_autonomous_require_approval() {
            if !require.contains(&hard) {
                require.push(hard);
            }
        }
    }

    // Soft daily spend: tools that look like paid spend need approval over cap.
    if spec.policy.max_daily_spend_usd > 0.0
        && is_spend_tool(&canonical)
        && require.iter().all(|c| c != "payments")
    {
        // Always gate spend tools when a daily spend cap is configured.
        return PolicyDecision::NeedsApproval {
            capability: "payments".into(),
        };
    }

    if require
        .iter()
        .any(|c| c == cap || c == &call.type_id || c == &canonical)
    {
        return PolicyDecision::NeedsApproval {
            capability: cap.into(),
        };
    }

    PolicyDecision::Allow
}

fn is_spend_tool(type_id: &str) -> bool {
    matches!(
        type_id,
        "ads.launch" | "billing.charge" | "payments" | "ads.spend"
    ) || type_id.contains("ads.")
        || type_id.contains("billing.")
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
}
