//! Execution ledger — one row per paid model call (tokens, $ and latency),
//! attributed to the objective / item / goal run / workflow execution that
//! triggered it. Used for spend tracking, budgets and execution analytics.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Who caused a model call. Every field is optional: a call made from a bare
/// API handler has an empty scope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerScope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<Uuid>,
}

impl LedgerScope {
    /// `self` with every field `other` sets taken from `other`.
    pub fn merged(&self, other: &LedgerScope) -> LedgerScope {
        LedgerScope {
            objective_id: other.objective_id.or(self.objective_id),
            item_id: other.item_id.clone().or_else(|| self.item_id.clone()),
            goal_run_id: other.goal_run_id.or(self.goal_run_id),
            workflow_id: other.workflow_id.or(self.workflow_id),
            execution_id: other.execution_id.or(self.execution_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    /// `openai` / `anthropic`.
    pub provider: String,
    pub model: String,
    /// `chat` for now; media calls can use their own kinds later.
    pub kind: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// Estimated from [`price_per_mtok`]; `None` when the model is unknown.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub duration_ms: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, flatten)]
    pub scope: LedgerScope,
}

/// USD per million (input, output) tokens for well-known models, matched by
/// prefix on the lowercased model id (longest prefix first). Rough list
/// prices — good enough for budgets, not for invoicing.
const PRICES: &[(&str, f64, f64)] = &[
    ("claude-opus-5", 15.0, 75.0),
    ("claude-opus-4", 15.0, 75.0),
    ("claude-sonnet-5", 3.0, 15.0),
    ("claude-sonnet-4", 3.0, 15.0),
    ("claude-3-7-sonnet", 3.0, 15.0),
    ("claude-3-5-sonnet", 3.0, 15.0),
    ("claude-haiku-5", 1.0, 5.0),
    ("claude-haiku-4", 1.0, 5.0),
    ("claude-3-5-haiku", 0.8, 4.0),
    ("claude-3-haiku", 0.25, 1.25),
    ("gpt-4o-mini", 0.15, 0.6),
    ("gpt-4o", 2.5, 10.0),
    ("gpt-4.1-nano", 0.1, 0.4),
    ("gpt-4.1-mini", 0.4, 1.6),
    ("gpt-4.1", 2.0, 8.0),
    ("gpt-5-nano", 0.05, 0.4),
    ("gpt-5-mini", 0.25, 2.0),
    ("gpt-5", 1.25, 10.0),
    ("o4-mini", 1.1, 4.4),
    ("o3-mini", 1.1, 4.4),
    ("o3", 2.0, 8.0),
    ("deepseek-chat", 0.27, 1.1),
    ("deepseek-reasoner", 0.55, 2.19),
];

/// (input, output) USD per million tokens, if the model is in the price table.
/// Provider prefixes like `openai/` or `anthropic/` are ignored.
pub fn price_per_mtok(model: &str) -> Option<(f64, f64)> {
    let m = model.trim().to_ascii_lowercase();
    let m = m.rsplit('/').next().unwrap_or(&m);
    PRICES
        .iter()
        .filter(|(prefix, _, _)| m.starts_with(prefix))
        .max_by_key(|(prefix, _, _)| prefix.len())
        .map(|(_, i, o)| (*i, *o))
}

pub fn estimate_cost_usd(model: &str, input_tokens: u64, output_tokens: u64) -> Option<f64> {
    price_per_mtok(model).map(|(i, o)| {
        (input_tokens as f64 * i + output_tokens as f64 * o) / 1_000_000.0
    })
}

/// Filters for listing / summarising the ledger; all optional.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LedgerQuery {
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub objective_id: Option<Uuid>,
    #[serde(default)]
    pub goal_run_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    #[serde(default)]
    pub execution_id: Option<Uuid>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerTotals {
    pub calls: u64,
    pub failed_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    /// Calls whose model has no known price (their cost is not in `cost_usd`).
    pub unpriced_calls: u64,
    pub duration_ms: u64,
}

impl LedgerTotals {
    pub fn add(&mut self, e: &LedgerEntry) {
        self.calls += 1;
        if !e.ok {
            self.failed_calls += 1;
        }
        self.input_tokens += e.input_tokens;
        self.output_tokens += e.output_tokens;
        match e.cost_usd {
            Some(c) => self.cost_usd += c,
            None if e.ok => self.unpriced_calls += 1,
            None => {}
        }
        self.duration_ms += e.duration_ms;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerGroup {
    pub key: String,
    #[serde(flatten)]
    pub totals: LedgerTotals,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerSummary {
    pub totals: LedgerTotals,
    /// Sorted by cost, then calls (descending).
    pub by_model: Vec<LedgerGroup>,
    pub by_objective: Vec<LedgerGroup>,
    pub by_workflow: Vec<LedgerGroup>,
    /// `YYYY-MM-DD` (UTC), oldest first.
    pub by_day: Vec<LedgerGroup>,
}

pub fn compute_ledger_summary(entries: &[LedgerEntry]) -> LedgerSummary {
    use std::collections::BTreeMap;

    fn sorted(map: BTreeMap<String, LedgerTotals>) -> Vec<LedgerGroup> {
        let mut v: Vec<LedgerGroup> = map
            .into_iter()
            .map(|(key, totals)| LedgerGroup { key, totals })
            .collect();
        v.sort_by(|a, b| {
            b.totals
                .cost_usd
                .total_cmp(&a.totals.cost_usd)
                .then(b.totals.calls.cmp(&a.totals.calls))
                .then(a.key.cmp(&b.key))
        });
        v
    }

    let mut s = LedgerSummary::default();
    let mut by_model = BTreeMap::<String, LedgerTotals>::new();
    let mut by_objective = BTreeMap::<String, LedgerTotals>::new();
    let mut by_workflow = BTreeMap::<String, LedgerTotals>::new();
    let mut by_day = BTreeMap::<String, LedgerTotals>::new();
    for e in entries {
        s.totals.add(e);
        by_model.entry(e.model.clone()).or_default().add(e);
        if let Some(id) = e.scope.objective_id {
            by_objective.entry(id.to_string()).or_default().add(e);
        }
        if let Some(id) = e.scope.workflow_id {
            by_workflow.entry(id.to_string()).or_default().add(e);
        }
        by_day
            .entry(e.at.format("%Y-%m-%d").to_string())
            .or_default()
            .add(e);
    }
    s.by_model = sorted(by_model);
    s.by_objective = sorted(by_objective);
    s.by_workflow = sorted(by_workflow);
    s.by_day = by_day
        .into_iter()
        .map(|(key, totals)| LedgerGroup { key, totals })
        .collect();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(model: &str, inp: u64, out: u64, ok: bool) -> LedgerEntry {
        LedgerEntry {
            id: Uuid::new_v4(),
            at: Utc::now(),
            provider: "openai".into(),
            model: model.into(),
            kind: "chat".into(),
            input_tokens: inp,
            output_tokens: out,
            cost_usd: estimate_cost_usd(model, inp, out),
            duration_ms: 100,
            ok,
            error: None,
            scope: LedgerScope::default(),
        }
    }

    #[test]
    fn prices_match_longest_prefix() {
        assert_eq!(price_per_mtok("gpt-4o-mini-2024-07-18"), Some((0.15, 0.6)));
        assert_eq!(price_per_mtok("gpt-4o"), Some((2.5, 10.0)));
        assert_eq!(price_per_mtok("openai/GPT-5-mini"), Some((0.25, 2.0)));
        assert_eq!(price_per_mtok("claude-sonnet-4-5"), Some((3.0, 15.0)));
        assert_eq!(price_per_mtok("llama-3"), None);
        let c = estimate_cost_usd("gpt-4o", 1_000_000, 500_000).unwrap();
        assert!((c - 7.5).abs() < 1e-9);
    }

    #[test]
    fn scope_merge_prefers_inner() {
        let outer = LedgerScope {
            objective_id: Some(Uuid::nil()),
            item_id: Some("1".into()),
            ..Default::default()
        };
        let wf = Uuid::new_v4();
        let inner = LedgerScope {
            item_id: Some("2".into()),
            workflow_id: Some(wf),
            ..Default::default()
        };
        let m = outer.merged(&inner);
        assert_eq!(m.objective_id, Some(Uuid::nil()));
        assert_eq!(m.item_id.as_deref(), Some("2"));
        assert_eq!(m.workflow_id, Some(wf));
    }

    #[test]
    fn summary_groups_and_counts_unpriced() {
        let obj = Uuid::new_v4();
        let mut a = entry("gpt-4o", 1000, 1000, true);
        a.scope.objective_id = Some(obj);
        let b = entry("gpt-4o", 0, 0, false);
        let c = entry("local-model", 10, 10, true);
        let s = compute_ledger_summary(&[a, b, c]);
        assert_eq!(s.totals.calls, 3);
        assert_eq!(s.totals.failed_calls, 1);
        assert_eq!(s.totals.unpriced_calls, 1);
        assert_eq!(s.totals.input_tokens, 1010);
        assert!((s.totals.cost_usd - 0.0125).abs() < 1e-9);
        assert_eq!(s.by_model[0].key, "gpt-4o");
        assert_eq!(s.by_model[0].totals.calls, 2);
        assert_eq!(s.by_objective.len(), 1);
        assert_eq!(s.by_objective[0].key, obj.to_string());
        assert_eq!(s.by_day.len(), 1);
    }
}
