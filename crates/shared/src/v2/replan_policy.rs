//! Replanning policy shared by goal runs: attempt limits, stop conditions and
//! escalation to the user when the goal looks unreachable.
//!
//! The evaluation is a pure function over persisted counters so the decision
//! survives restarts and is unit-testable without a runtime.

use serde::{Deserialize, Serialize};

/// Tool type id of an escalation approval: resolving it never executes a tool.
pub const ESCALATION_TOOL: &str = "goal.escalation";
pub const ESCALATION_CAPABILITY: &str = "goal.escalation";

/// Limits per goal (stored inside [`super::PolicyBundle`]). `0` disables a limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplanLimits {
    /// Ticks in a row where every attempted action failed or was denied.
    #[serde(default = "default_failed_ticks")]
    pub max_consecutive_failed_ticks: u32,
    /// Ticks in a row without the goal metric improving.
    #[serde(default = "default_no_progress")]
    pub max_ticks_without_progress: u32,
    /// Hard cap on ticks for the whole run.
    #[serde(default)]
    pub max_total_ticks: u32,
    /// Escalations the user may answer with "continue" before the run stops.
    #[serde(default = "default_escalations")]
    pub max_escalations: u32,
}

fn default_failed_ticks() -> u32 {
    5
}
fn default_no_progress() -> u32 {
    100
}
fn default_escalations() -> u32 {
    3
}

impl Default for ReplanLimits {
    fn default() -> Self {
        Self {
            max_consecutive_failed_ticks: default_failed_ticks(),
            max_ticks_without_progress: default_no_progress(),
            max_total_ticks: 0,
            max_escalations: default_escalations(),
        }
    }
}

/// Counters persisted on the run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReplanState {
    #[serde(default)]
    pub consecutive_failed_ticks: u32,
    #[serde(default)]
    pub ticks_without_progress: u32,
    /// Best metric value seen; progress means beating it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_current: Option<f64>,
    #[serde(default)]
    pub replans: u32,
    #[serde(default)]
    pub escalations: u32,
    /// Why the planner should change course; fed back into the next tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reason: Option<String>,
}

/// What happened in one tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickOutcome {
    /// Tick number after this tick (1-based count of finished ticks).
    pub ticks_done: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub denied: u32,
    pub current: f64,
    /// Experiments exist and every one was judged `pivot` or `stop`.
    pub all_experiments_rejected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReplanDecision {
    Continue,
    /// Change course on the next tick; the reason is shown to the planner.
    Replan(String),
    /// Ask the user whether to continue; the run waits.
    Escalate(String),
    /// Terminal: the run cannot reach the goal within its limits.
    Stop(String),
}

impl ReplanState {
    /// Updates counters with one tick and decides what the run does next.
    pub fn evaluate(&mut self, limits: &ReplanLimits, o: &TickOutcome) -> ReplanDecision {
        let attempted = o.succeeded + o.failed + o.denied;
        if attempted > 0 && o.succeeded == 0 {
            self.consecutive_failed_ticks += 1;
        } else if o.succeeded > 0 {
            self.consecutive_failed_ticks = 0;
        }

        match self.best_current {
            Some(best) if o.current <= best + f64::EPSILON => self.ticks_without_progress += 1,
            _ => {
                self.best_current = Some(o.current);
                self.ticks_without_progress = 0;
            }
        }

        let exceeded = if limits.max_consecutive_failed_ticks > 0
            && self.consecutive_failed_ticks >= limits.max_consecutive_failed_ticks
        {
            Some(format!(
                "{} ticks in a row without a successful action",
                self.consecutive_failed_ticks
            ))
        } else if limits.max_ticks_without_progress > 0
            && self.ticks_without_progress >= limits.max_ticks_without_progress
        {
            Some(format!(
                "no progress on the goal metric for {} ticks",
                self.ticks_without_progress
            ))
        } else {
            None
        };

        if limits.max_total_ticks > 0 && o.ticks_done >= limits.max_total_ticks {
            return self.stop(format!("tick limit {} reached", limits.max_total_ticks));
        }

        if let Some(reason) = exceeded {
            if self.escalations >= limits.max_escalations {
                return self.stop(format!(
                    "{reason}; escalation limit {} reached",
                    limits.max_escalations
                ));
            }
            self.escalations += 1;
            self.last_reason = Some(reason.clone());
            return ReplanDecision::Escalate(reason);
        }

        let replan = if attempted > 0 && o.succeeded == 0 {
            Some(format!(
                "all {attempted} action(s) failed or were denied last tick; choose a different approach"
            ))
        } else if o.all_experiments_rejected {
            Some("every experiment was judged pivot/stop; form new hypotheses".to_string())
        } else {
            None
        };
        match replan {
            Some(reason) => {
                self.replans += 1;
                self.last_reason = Some(reason.clone());
                ReplanDecision::Replan(reason)
            }
            None => {
                self.last_reason = None;
                ReplanDecision::Continue
            }
        }
    }

    /// The user chose to continue after an escalation: give the run a fresh window.
    pub fn acknowledge_escalation(&mut self) {
        self.consecutive_failed_ticks = 0;
        self.ticks_without_progress = 0;
        self.last_reason = Some("user asked to continue after escalation; try a new approach".into());
    }

    fn stop(&mut self, reason: String) -> ReplanDecision {
        self.last_reason = Some(reason.clone());
        ReplanDecision::Stop(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(failed: u32, progress: u32, total: u32, esc: u32) -> ReplanLimits {
        ReplanLimits {
            max_consecutive_failed_ticks: failed,
            max_ticks_without_progress: progress,
            max_total_ticks: total,
            max_escalations: esc,
        }
    }

    fn tick(n: u32, ok: u32, failed: u32, current: f64) -> TickOutcome {
        TickOutcome {
            ticks_done: n,
            succeeded: ok,
            failed,
            current,
            ..Default::default()
        }
    }

    #[test]
    fn failed_tick_replans_then_escalates_then_stops() {
        let l = limits(2, 0, 0, 1);
        let mut s = ReplanState::default();
        assert!(matches!(s.evaluate(&l, &tick(1, 0, 1, 0.0)), ReplanDecision::Replan(_)));
        assert!(matches!(s.evaluate(&l, &tick(2, 0, 1, 0.0)), ReplanDecision::Escalate(_)));
        assert_eq!(s.escalations, 1);
        s.acknowledge_escalation();
        assert!(matches!(s.evaluate(&l, &tick(3, 0, 1, 0.0)), ReplanDecision::Replan(_)));
        match s.evaluate(&l, &tick(4, 0, 1, 0.0)) {
            ReplanDecision::Stop(r) => assert!(r.contains("escalation limit")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn success_resets_failure_streak() {
        let l = limits(2, 0, 0, 3);
        let mut s = ReplanState::default();
        s.evaluate(&l, &tick(1, 0, 1, 0.0));
        assert_eq!(s.evaluate(&l, &tick(2, 1, 1, 0.0)), ReplanDecision::Continue);
        assert_eq!(s.consecutive_failed_ticks, 0);
        assert!(s.last_reason.is_none());
    }

    #[test]
    fn idle_tick_is_not_a_failure() {
        let l = limits(1, 0, 0, 3);
        let mut s = ReplanState::default();
        assert_eq!(s.evaluate(&l, &tick(1, 0, 0, 0.0)), ReplanDecision::Continue);
        assert_eq!(s.consecutive_failed_ticks, 0);
    }

    #[test]
    fn stagnation_escalates_and_progress_resets() {
        let l = limits(0, 2, 0, 3);
        let mut s = ReplanState::default();
        assert_eq!(s.evaluate(&l, &tick(1, 1, 0, 1.0)), ReplanDecision::Continue);
        assert_eq!(s.evaluate(&l, &tick(2, 1, 0, 1.0)), ReplanDecision::Continue);
        assert!(matches!(s.evaluate(&l, &tick(3, 1, 0, 1.0)), ReplanDecision::Escalate(r) if r.contains("no progress")));
        s.acknowledge_escalation();
        assert_eq!(s.evaluate(&l, &tick(4, 1, 0, 2.0)), ReplanDecision::Continue);
        assert_eq!(s.ticks_without_progress, 0);
        assert_eq!(s.best_current, Some(2.0));
    }

    #[test]
    fn total_tick_cap_stops_even_when_healthy() {
        let l = limits(0, 0, 3, 3);
        let mut s = ReplanState::default();
        assert_eq!(s.evaluate(&l, &tick(2, 1, 0, 1.0)), ReplanDecision::Continue);
        assert!(matches!(s.evaluate(&l, &tick(3, 1, 0, 2.0)), ReplanDecision::Stop(_)));
    }

    #[test]
    fn rejected_experiments_trigger_replan() {
        let l = ReplanLimits::default();
        let mut s = ReplanState::default();
        let o = TickOutcome {
            ticks_done: 1,
            succeeded: 1,
            all_experiments_rejected: true,
            ..Default::default()
        };
        assert!(matches!(s.evaluate(&l, &o), ReplanDecision::Replan(_)));
        assert_eq!(s.replans, 1);
    }

    #[test]
    fn zero_escalation_budget_stops_immediately() {
        let l = limits(1, 0, 0, 0);
        let mut s = ReplanState::default();
        assert!(matches!(s.evaluate(&l, &tick(1, 0, 1, 0.0)), ReplanDecision::Stop(_)));
    }

    #[test]
    fn old_policy_json_gets_default_limits() {
        let l: ReplanLimits = serde_json::from_str("{}").unwrap();
        assert_eq!(l, ReplanLimits::default());
        let s: ReplanState = serde_json::from_str("{}").unwrap();
        assert_eq!(s, ReplanState::default());
    }
}
