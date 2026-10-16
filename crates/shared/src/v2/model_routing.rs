//! Multi-model routing: picks a (connection, model) for one LLM call from the
//! task kind, required capabilities, model availability and worst-case cost.
//!
//! Pure and deterministic — the backend supplies the candidates (enabled
//! connections × their models, plus health) and the remaining budget.
//! Fail-closed: no candidate that provably fits ⇒ `Err` with every rejection.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ledger::estimate_cost_usd;
use crate::{AiModelCapabilities, LlmProvider};

/// What the call is for; decides the minimum quality tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Multi-step planning / strategy (goal planner, objective drafting).
    Planning,
    /// Hard reasoning, evaluation of results, code authoring.
    Reasoning,
    /// General chat / text generation.
    #[default]
    Generation,
    /// Summaries, rewriting.
    Summarization,
    /// Structured extraction (JSON from text).
    Extraction,
    /// Labels, routing, yes/no checks.
    Classification,
}

impl TaskKind {
    pub fn min_tier(self) -> ModelTier {
        match self {
            Self::Planning | Self::Reasoning => ModelTier::Flagship,
            Self::Generation | Self::Summarization => ModelTier::Balanced,
            Self::Extraction | Self::Classification => ModelTier::Light,
        }
    }
}

/// Coarse quality class derived from list price (output USD / Mtok).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Light,
    Balanced,
    Flagship,
}

/// Tier of a model; `None` when its price (and so its class) is unknown.
pub fn model_tier(model: &str) -> Option<ModelTier> {
    let (_, out) = super::ledger::price_per_mtok(model)?;
    Some(if out >= 8.0 {
        ModelTier::Flagship
    } else if out >= 1.5 {
        ModelTier::Balanced
    } else {
        ModelTier::Light
    })
}

/// One routable (connection, model) pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCandidate {
    pub connection_id: Uuid,
    #[serde(default)]
    pub connection_name: String,
    pub provider: LlmProvider,
    pub model: String,
    #[serde(default)]
    pub capabilities: AiModelCapabilities,
    /// Local endpoint (localhost) — free, any model id.
    #[serde(default)]
    pub local: bool,
    /// `false` while the circuit breaker holds it open after failures.
    #[serde(default = "yes")]
    pub available: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteRequest {
    pub task: TaskKind,
    /// Capabilities every chosen model must have (`chat`, `image_generate`, …).
    #[serde(default)]
    pub required: Vec<String>,
    pub est_input_tokens: u64,
    pub max_output_tokens: u64,
    /// Remaining USD under the active budget; `None` = no budget applies.
    #[serde(default)]
    pub budget_remaining_usd: Option<f64>,
    /// Only route inside this connection (explicit user choice).
    #[serde(default)]
    pub connection_id: Option<Uuid>,
    /// Tie-break preference (Claude keeps the historical default).
    #[serde(default)]
    pub prefer_provider: Option<LlmProvider>,
    /// Pairs already tried in this call (fallback after a failure).
    #[serde(default)]
    pub exclude: Vec<(Uuid, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteChoice {
    pub connection_id: Uuid,
    pub model: String,
    pub provider: LlmProvider,
    /// Worst-case cost of the call; `None` only when no budget applies and
    /// the price is unknown.
    pub worst_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteDecision {
    pub choice: RouteChoice,
    /// Remaining eligible pairs, best first, for fallback on failure.
    pub fallbacks: Vec<RouteChoice>,
    /// `true` when nothing met the task's minimum tier and a weaker model was taken.
    pub degraded: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rejection {
    pub connection_id: Uuid,
    pub model: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteError {
    pub message: String,
    pub rejected: Vec<Rejection>,
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        for r in &self.rejected {
            write!(f, "; {} ({}): {}", r.model, r.connection_id, r.reason)?;
        }
        Ok(())
    }
}

struct Eligible<'a> {
    c: &'a RouteCandidate,
    cost: Option<f64>,
    tier: ModelTier,
}

/// Picks the cheapest available model meeting the task's tier and all
/// capabilities within budget; degrades to the strongest affordable one when
/// none meets the tier. Unknown price under a budget is refused.
pub fn route(req: &RouteRequest, candidates: &[RouteCandidate]) -> Result<RouteDecision, RouteError> {
    let mut rejected = Vec::new();
    let mut eligible: Vec<Eligible> = Vec::new();
    let reject = |rejected: &mut Vec<Rejection>, c: &RouteCandidate, reason: String| {
        rejected.push(Rejection {
            connection_id: c.connection_id,
            model: c.model.clone(),
            reason,
        })
    };
    for c in candidates {
        if c.model.trim().is_empty() {
            continue;
        }
        if req.connection_id.is_some_and(|id| id != c.connection_id) {
            continue;
        }
        if req.exclude.iter().any(|(id, m)| *id == c.connection_id && *m == c.model) {
            reject(&mut rejected, c, "already tried in this call".into());
            continue;
        }
        if !c.available {
            reject(&mut rejected, c, "temporarily unavailable after failures".into());
            continue;
        }
        let missing: Vec<&str> = req
            .required
            .iter()
            .map(String::as_str)
            .filter(|cap| !c.capabilities.supports(cap))
            .collect();
        if !missing.is_empty() {
            reject(&mut rejected, c, format!("missing capability {}", missing.join(", ")));
            continue;
        }
        let cost = if c.local {
            Some(0.0)
        } else {
            estimate_cost_usd(&c.model, req.est_input_tokens, req.max_output_tokens)
        };
        if let Some(remaining) = req.budget_remaining_usd {
            match cost {
                None => {
                    reject(&mut rejected, c, "price unknown while a budget applies".into());
                    continue;
                }
                Some(cost) if !(remaining.is_finite() && cost <= remaining) => {
                    reject(
                        &mut rejected,
                        c,
                        format!("worst case ${cost:.4} exceeds remaining budget ${remaining:.4}"),
                    );
                    continue;
                }
                _ => {}
            }
        }
        // Unknown price (local or unlisted model) counts as balanced.
        let tier = model_tier(&c.model).unwrap_or(ModelTier::Balanced);
        eligible.push(Eligible { c, cost, tier });
    }

    if eligible.is_empty() {
        let message = if candidates.is_empty() {
            "no AI model available — add an OpenAI-compatible or Claude provider in Settings → AI".to_string()
        } else {
            format!("no AI model fits task `{:?}`", req.task)
        };
        return Err(RouteError { message, rejected });
    }

    let min = req.task.min_tier();
    let degraded = !eligible.iter().any(|e| e.tier >= min);
    let prefer = req.prefer_provider;
    eligible.sort_by(|a, b| {
        let fit_a = a.tier >= min;
        let fit_b = b.tier >= min;
        fit_b
            .cmp(&fit_a)
            // Degraded: strongest first. Fitting: cheapest first.
            .then_with(|| if degraded { b.tier.cmp(&a.tier) } else { std::cmp::Ordering::Equal })
            .then_with(|| {
                let ca = a.cost.unwrap_or(f64::INFINITY);
                let cb = b.cost.unwrap_or(f64::INFINITY);
                ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| (Some(b.c.provider) == prefer).cmp(&(Some(a.c.provider) == prefer)))
            .then_with(|| a.c.model.cmp(&b.c.model))
            .then_with(|| a.c.connection_id.cmp(&b.c.connection_id))
    });

    let to_choice = |e: &Eligible| RouteChoice {
        connection_id: e.c.connection_id,
        model: e.c.model.clone(),
        provider: e.c.provider,
        worst_cost_usd: e.cost,
    };
    let best = &eligible[0];
    let reason = format!(
        "{:?} task → `{}` ({:?}{}, worst case {}){}",
        req.task,
        best.c.model,
        best.tier,
        if best.c.local { ", local" } else { "" },
        best.cost.map(|c| format!("${c:.4}")).unwrap_or_else(|| "unpriced".into()),
        if degraded { "; no model meets the task tier, strongest affordable taken" } else { "" }
    );
    Ok(RouteDecision {
        choice: to_choice(best),
        fallbacks: eligible[1..].iter().map(to_choice).collect(),
        degraded,
        reason,
    })
}

/// Errors after which another model may be tried: the call had no effect
/// (LLM calls are read-only) and the cause is the provider, not the request
/// or a budget/policy refusal.
pub fn is_retryable_on_other_model(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    if e.contains("budget") || e.contains("refused") || e.contains("policy") {
        return false;
    }
    [
        "timeout", "timed out", "429", "rate limit", "overloaded", "500", "502", "503", "504",
        "connection", "unavailable", "model_not_found", "does not exist", "not found",
    ]
    .iter()
    .any(|k| e.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(conn: u128, provider: LlmProvider, model: &str) -> RouteCandidate {
        RouteCandidate {
            connection_id: Uuid::from_u128(conn),
            connection_name: format!("c{conn}"),
            provider,
            model: model.into(),
            capabilities: AiModelCapabilities::chat_only(),
            local: false,
            available: true,
        }
    }

    fn req(task: TaskKind) -> RouteRequest {
        RouteRequest {
            task,
            required: vec!["chat".into()],
            est_input_tokens: 2_000,
            max_output_tokens: 1_000,
            ..Default::default()
        }
    }

    fn pool() -> Vec<RouteCandidate> {
        vec![
            cand(1, LlmProvider::Anthropic, "claude-opus-5-5"),
            cand(1, LlmProvider::Anthropic, "claude-sonnet-5-5"),
            cand(1, LlmProvider::Anthropic, "claude-haiku-4-5"),
            cand(2, LlmProvider::Openai, "gpt-4o-mini"),
        ]
    }

    #[test]
    fn tiers_follow_price() {
        assert_eq!(model_tier("claude-opus-5-5"), Some(ModelTier::Flagship));
        assert_eq!(model_tier("claude-haiku-4-5"), Some(ModelTier::Balanced));
        assert_eq!(model_tier("gpt-4o-mini"), Some(ModelTier::Light));
        assert_eq!(model_tier("my-local-llama"), None);
    }

    #[test]
    fn cheap_task_gets_cheapest_model() {
        let d = route(&req(TaskKind::Classification), &pool()).unwrap();
        assert_eq!(d.choice.model, "gpt-4o-mini");
        assert!(!d.degraded);
        assert_eq!(d.fallbacks.len(), 3);
    }

    #[test]
    fn planning_gets_cheapest_flagship() {
        let d = route(&req(TaskKind::Planning), &pool()).unwrap();
        // sonnet ($15 out) is flagship and cheaper than opus.
        assert_eq!(d.choice.model, "claude-sonnet-5-5");
        assert_eq!(d.fallbacks[0].model, "claude-opus-5-5");
    }

    #[test]
    fn budget_excludes_expensive_and_degrades() {
        let mut r = req(TaskKind::Planning);
        // opus: 2k*15 + 1k*75 = 0.105; sonnet 0.021; haiku 0.007; 4o-mini 0.0009
        r.budget_remaining_usd = Some(0.01);
        let d = route(&r, &pool()).unwrap();
        assert!(d.degraded);
        assert_eq!(d.choice.model, "claude-haiku-4-5", "strongest affordable");
        assert!(d.fallbacks.iter().all(|f| f.worst_cost_usd.unwrap() <= 0.01));
    }

    #[test]
    fn budget_exhausted_fails_closed() {
        let mut r = req(TaskKind::Classification);
        r.budget_remaining_usd = Some(0.0001);
        let err = route(&r, &pool()).unwrap_err();
        assert_eq!(err.rejected.len(), 4);
        assert!(err.to_string().contains("exceeds remaining budget"), "{err}");
    }

    #[test]
    fn unknown_price_refused_under_budget_but_allowed_without() {
        let pool = vec![cand(3, LlmProvider::Openai, "mystery-model")];
        let mut r = req(TaskKind::Generation);
        assert_eq!(route(&r, &pool).unwrap().choice.worst_cost_usd, None);
        r.budget_remaining_usd = Some(100.0);
        let err = route(&r, &pool).unwrap_err();
        assert!(err.to_string().contains("price unknown"), "{err}");
    }

    #[test]
    fn local_models_are_free_under_budget() {
        let mut local = cand(4, LlmProvider::Openai, "llama3");
        local.local = true;
        let mut r = req(TaskKind::Generation);
        r.budget_remaining_usd = Some(0.0);
        let d = route(&r, &[local]).unwrap();
        assert_eq!(d.choice.worst_cost_usd, Some(0.0));
    }

    #[test]
    fn capabilities_are_required() {
        let mut img = cand(2, LlmProvider::Openai, "gpt-image-1");
        img.capabilities.image_generate = true;
        let mut pool = pool();
        pool.push(img);
        let mut r = req(TaskKind::Generation);
        r.required = vec!["image_generate".into()];
        let d = route(&r, &pool).unwrap();
        assert_eq!(d.choice.model, "gpt-image-1");
        assert!(d.fallbacks.is_empty());
        r.required.push("video_generate".into());
        assert!(route(&r, &pool).is_err());
    }

    #[test]
    fn unavailable_and_excluded_are_skipped() {
        let mut pool = pool();
        pool[3].available = false;
        let mut r = req(TaskKind::Classification);
        r.exclude = vec![(Uuid::from_u128(1), "claude-haiku-4-5".into())];
        let d = route(&r, &pool).unwrap();
        assert_eq!(d.choice.model, "claude-sonnet-5-5");
    }

    #[test]
    fn explicit_connection_is_respected() {
        let mut r = req(TaskKind::Classification);
        r.connection_id = Some(Uuid::from_u128(1));
        let d = route(&r, &pool()).unwrap();
        assert_eq!(d.choice.connection_id, Uuid::from_u128(1));
        assert_eq!(d.choice.model, "claude-haiku-4-5");
    }

    #[test]
    fn provider_preference_breaks_ties() {
        let pool = vec![
            cand(2, LlmProvider::Openai, "claude-sonnet-5-5"),
            cand(1, LlmProvider::Anthropic, "claude-sonnet-5-5"),
        ];
        let mut r = req(TaskKind::Generation);
        r.prefer_provider = Some(LlmProvider::Anthropic);
        assert_eq!(route(&r, &pool).unwrap().choice.connection_id, Uuid::from_u128(1));
    }

    #[test]
    fn no_candidates_is_an_error() {
        let err = route(&req(TaskKind::Generation), &[]).unwrap_err();
        assert!(err.message.contains("no AI model available"));
    }

    #[test]
    fn retryable_classification() {
        assert!(is_retryable_on_other_model("HTTP 503 Service Unavailable"));
        assert!(is_retryable_on_other_model("request timed out"));
        assert!(!is_retryable_on_other_model("budget `g` exhausted: call refused"));
        assert!(!is_retryable_on_other_model("HTTP 400 invalid json"));
    }
}
