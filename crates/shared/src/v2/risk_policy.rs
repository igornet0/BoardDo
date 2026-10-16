//! Unified risk policy: one place that combines capability budgets, autonomy
//! levels, approval requirements and tool risk metadata into a single
//! pre-execution decision. Used by the agent tick loop, approval replays and
//! workflow preflight so every execution path applies the same rules.

use super::capability::{CapabilityBudget, caps};
use super::goal_runtime::resolve_tool_alias;
use super::marketing::{AutonomyLevel, is_outbound_tool};
use super::node_contract::NodeKind;
use super::tool_registry::lookup_tool;

/// Marker prefix for errors that mean "a human must approve this first".
pub const NEEDS_APPROVAL_PREFIX: &str = "needs_approval:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskDecision {
    Allow,
    Deny(String),
    NeedsApproval { capability: String },
}

/// Inputs of a single risk evaluation.
#[derive(Debug, Clone)]
pub struct RiskRequest<'a> {
    pub type_id: &'a str,
    pub autonomy: AutonomyLevel,
    pub budget: &'a CapabilityBudget,
    /// Goal tool allowlist; `None` or empty means "any catalogued tool".
    pub allowlist: Option<&'a [String]>,
    pub require_approval: &'a [String],
    /// A daily spend cap is configured, so paid tools are gated.
    pub spend_gated: bool,
    /// A human already approved this exact call.
    pub approved: bool,
}

const PUBLISH_APPROVALS: &[&str] = &[
    caps::TELEGRAM_WRITE,
    "telegram.write",
    "publisher.telegram.send",
    "telegram.publish",
];

/// Approval list after applying the autonomy level: level 3+ drops publish
/// approvals but always keeps the hard (money / destructive) list.
pub fn effective_require_approval(require: &[String], level: AutonomyLevel) -> Vec<String> {
    let mut out: Vec<String> = require.to_vec();
    if level.allows_publish_without_approval() {
        out.retain(|c| !PUBLISH_APPROVALS.contains(&c.as_str()));
        for hard in AutonomyLevel::full_autonomous_require_approval() {
            if !out.contains(&hard) {
                out.push(hard);
            }
        }
    }
    out
}

/// Tool spends real money (ads, billing, payments).
pub fn is_spend_tool(type_id: &str) -> bool {
    let canonical = resolve_tool_alias(type_id);
    if let Some(d) = lookup_tool(canonical)
        && matches!(d.capability, caps::ADS_LAUNCH | caps::BILLING_WRITE | "payments")
    {
        return true;
    }
    matches!(
        canonical,
        "ads.launch" | "billing.charge" | "payments" | "ads.spend"
    ) || canonical.contains("ads.")
        || canonical.contains("billing.")
}

/// Node kinds that only read or route data and never act on the outside world.
pub fn is_passive_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Trigger | NodeKind::Data | NodeKind::Decision | NodeKind::Control | NodeKind::Debug
    )
}

pub fn evaluate(req: &RiskRequest<'_>) -> RiskDecision {
    let decision = evaluate_raw(req);
    match decision {
        RiskDecision::NeedsApproval { .. } if req.approved => RiskDecision::Allow,
        other => other,
    }
}

fn evaluate_raw(req: &RiskRequest<'_>) -> RiskDecision {
    let canonical = resolve_tool_alias(req.type_id);

    if req.autonomy.observe_only() && is_outbound_tool(canonical) {
        return RiskDecision::Deny(format!(
            "autonomy level {} forbids outbound/publish tools",
            req.autonomy.0
        ));
    }

    let Some(descriptor) = lookup_tool(canonical) else {
        return RiskDecision::Deny(format!("unregistered tool `{}`", req.type_id));
    };
    if !descriptor.available {
        return RiskDecision::Deny(format!("tool `{}` has no executor", req.type_id));
    }

    let cap = descriptor.capability;
    if !req.budget.allows(cap) {
        return RiskDecision::Deny(format!("capability `{cap}` not allowed"));
    }

    if let Some(list) = req.allowlist
        && !list.is_empty()
        && !list.iter().any(|t| {
            t == req.type_id || t == canonical || resolve_tool_alias(t) == canonical
        })
    {
        return RiskDecision::Deny(format!(
            "tool `{}` is not in the goal allowlist",
            req.type_id
        ));
    }

    let require = effective_require_approval(req.require_approval, req.autonomy);

    if req.spend_gated && is_spend_tool(canonical) && require.iter().all(|c| c != "payments") {
        return RiskDecision::NeedsApproval {
            capability: "payments".into(),
        };
    }

    if require
        .iter()
        .any(|c| c == cap || c == req.type_id || c == canonical)
    {
        return RiskDecision::NeedsApproval {
            capability: cap.into(),
        };
    }

    if descriptor.irreversible {
        return RiskDecision::NeedsApproval {
            capability: "irreversible".into(),
        };
    }

    RiskDecision::Allow
}

/// Error text that asks the caller to queue an approval for `capability`.
pub fn needs_approval_error(capability: &str, detail: &str) -> String {
    format!("{NEEDS_APPROVAL_PREFIX}{capability}:{detail}")
}

/// Capability from an error produced by [`needs_approval_error`].
pub fn parse_needs_approval(err: &str) -> Option<String> {
    let rest = err.strip_prefix(NEEDS_APPROVAL_PREFIX)?;
    let cap = rest.split(':').next().unwrap_or_default().trim();
    // Capabilities may contain dots but never colons; empty is malformed.
    if cap.is_empty() { None } else { Some(cap.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget() -> CapabilityBudget {
        CapabilityBudget::default()
            .grant(caps::WEB_SEARCH)
            .grant(caps::TELEGRAM_WRITE)
            .grant(caps::GITHUB_WRITE)
    }

    fn req<'a>(
        type_id: &'a str,
        budget: &'a CapabilityBudget,
        require: &'a [String],
        level: u8,
    ) -> RiskRequest<'a> {
        RiskRequest {
            type_id,
            autonomy: AutonomyLevel(level),
            budget,
            allowlist: None,
            require_approval: require,
            spend_gated: false,
            approved: false,
        }
    }

    #[test]
    fn level3_drops_publish_keeps_hard_list() {
        let req = vec![caps::TELEGRAM_WRITE.to_string(), "telegram.publish".into()];
        let out = effective_require_approval(&req, AutonomyLevel(3));
        assert!(!out.iter().any(|c| c == caps::TELEGRAM_WRITE || c == "telegram.publish"));
        assert!(out.iter().any(|c| c == "payments"));
        assert!(out.iter().any(|c| c == caps::ADS_LAUNCH));
        let low = effective_require_approval(&req, AutonomyLevel(2));
        assert_eq!(low, req);
    }

    #[test]
    fn deny_beats_approval() {
        let b = CapabilityBudget::default().grant(caps::WEB_SEARCH);
        let mut r = req("telegram.user.send_message", &b, &[], 2);
        r.approved = true;
        assert!(matches!(evaluate(&r), RiskDecision::Deny(_)));
    }

    #[test]
    fn approval_turns_needs_approval_into_allow() {
        let b = budget();
        let require = vec![caps::TELEGRAM_WRITE.to_string()];
        let mut r = req("telegram.user.send_message", &b, &require, 2);
        assert!(matches!(evaluate(&r), RiskDecision::NeedsApproval { .. }));
        r.approved = true;
        assert_eq!(evaluate(&r), RiskDecision::Allow);
    }

    #[test]
    fn observe_only_denies_outbound_even_if_approved() {
        let b = budget();
        let mut r = req("telegram.user.send_message", &b, &[], 1);
        r.approved = true;
        assert!(matches!(evaluate(&r), RiskDecision::Deny(m) if m.contains("autonomy level 1")));
    }

    #[test]
    fn irreversible_needs_approval_at_any_level() {
        let b = budget();
        for level in 1..=4 {
            let r = req("projecthub.command", &b, &[], level);
            assert_eq!(
                evaluate(&r),
                RiskDecision::NeedsApproval {
                    capability: "irreversible".into()
                }
            );
        }
    }

    #[test]
    fn allowlist_respects_aliases() {
        let b = budget();
        let list = vec!["publisher.telegram.send".to_string()];
        let mut r = req("telegram.publish", &b, &[], 3);
        r.allowlist = Some(&list);
        assert_eq!(evaluate(&r), RiskDecision::Allow);
        let mut r = req("web.search", &b, &[], 3);
        r.allowlist = Some(&list);
        assert!(matches!(evaluate(&r), RiskDecision::Deny(m) if m.contains("allowlist")));
    }

    #[test]
    fn unknown_tool_fails_closed() {
        let b = budget();
        let mut r = req("nope.nothing", &b, &[], 4);
        r.approved = true;
        assert!(matches!(evaluate(&r), RiskDecision::Deny(_)));
    }

    #[test]
    fn needs_approval_marker_roundtrip() {
        let e = needs_approval_error("telegram.write", "node `n1` telegram.publish");
        assert_eq!(parse_needs_approval(&e).as_deref(), Some("telegram.write"));
        assert_eq!(parse_needs_approval("boom"), None);
        assert_eq!(parse_needs_approval("needs_approval::x"), None);
    }

    #[test]
    fn spend_tools_detected_by_capability() {
        assert!(is_spend_tool("ads.launch"));
        assert!(!is_spend_tool("web.search"));
    }
}
