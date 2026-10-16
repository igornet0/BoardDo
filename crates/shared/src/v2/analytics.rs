//! Goal analytics: did the goal get reached, and what did it cost.
//!
//! Operational metrics (tasks done, success/error rates, time, spend) and
//! outcome metrics (leads, conversion, revenue, goal reached) are reported in
//! separate blocks and never substitute for each other: a run that completed
//! fifty tool calls and produced no qualified lead is `not_reached`, whatever
//! its own counter says. The goal value is taken from verified records (real
//! leads, won deals, completed executions); the agent's self-reported
//! `run.current` is shown next to it, never instead of it. Where the records
//! cannot answer a question, the report says so in `data_gaps`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::goal_runtime::{GoalRun, GoalRunStatus, GoalSpec};

/// Spend figures come from the model price list, not from provider invoices.
pub const COST_BASIS_LIST_PRICE: &str = "list_price_estimate";

const MAX_FAILURE_REASONS: usize = 10;
const HIGH_ERROR_RATE: f64 = 0.3;
const BUDGET_WARN_PCT: f64 = 80.0;

// ---------------------------------------------------------------- facts ----

/// Agent tool calls as recorded in the audit trail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolCallFacts {
    pub succeeded: u64,
    pub failed: u64,
    /// Refused by policy / approval / budget before running.
    pub denied: u64,
}

/// Single-execution call journal (sends, publishes, irreversible actions).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct JournalFacts {
    pub succeeded: u64,
    pub failed: u64,
    /// `started` and never finished: in flight or outcome unknown.
    pub unresolved: u64,
}

/// Workflow executions of the workflows linked to the goal's runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecutionFacts {
    /// Completed with no node reporting a failure.
    pub completed: u64,
    /// Completed, but a node reported a failure and the run went on.
    #[serde(default)]
    pub degraded: u64,
    pub failed: u64,
    pub cancelled: u64,
    /// `pending` / `running`.
    pub unfinished: u64,
    /// Linked workflows with at least one clean completed execution.
    pub workflows_with_completed: u64,
    pub workflows_linked: u64,
}

/// Model calls from the execution ledger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SpendFacts {
    pub calls: u64,
    pub failed_calls: u64,
    /// Sum of priced calls (list price estimate).
    pub priced_usd: f64,
    /// Calls without a price: the real spend is higher than `priced_usd`.
    pub unpriced_calls: u64,
}

/// Money spent outside model calls (ads, external services), entered by the
/// operator. Amounts keep their currency; only USD enters `actual_usd`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExternalSpendFacts {
    pub entries: u64,
    pub by_currency: BTreeMap<String, f64>,
}

/// One recorded external expense of a goal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalSpendEntry {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub amount: f64,
    pub currency: String,
    pub category: String,
    pub description: String,
    pub occurred_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewExternalSpend {
    pub amount: f64,
    pub currency: String,
    /// `ads`, `service`, `contractor`, ... free text, defaults to `other`.
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub description: String,
    /// When the money was spent; defaults to the time of recording.
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
}

impl NewExternalSpend {
    /// Checks the input and returns the entry to store.
    pub fn into_entry(
        self,
        goal_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<ExternalSpendEntry, String> {
        if !self.amount.is_finite() || self.amount <= 0.0 {
            return Err("amount must be a positive number".into());
        }
        let currency = self.currency.trim().to_uppercase();
        if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err("currency must be a 3-letter code, e.g. USD".into());
        }
        let category = self.category.trim();
        if let Some(at) = self.occurred_at
            && at > now
        {
            return Err("occurred_at is in the future".into());
        }
        Ok(ExternalSpendEntry {
            id: Uuid::now_v7(),
            goal_id,
            amount: self.amount,
            currency,
            category: if category.is_empty() { "other".into() } else { category.to_lowercase() },
            description: self.description.trim().to_string(),
            occurred_at: self.occurred_at.unwrap_or(now),
            created_at: now,
        })
    }
}

/// Before-automation figures from `spec.constraints.baseline`, used to
/// measure cost reduction and time saved per unit of the goal metric.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub cost_per_unit_usd: Option<f64>,
    pub manual_hours_per_unit: Option<f64>,
}

impl Baseline {
    /// Reads `constraints.baseline`; non-positive or non-numeric values are
    /// dropped and listed in the second element.
    pub fn from_constraints(c: &serde_json::Value) -> (Self, Vec<&'static str>) {
        let mut invalid = Vec::new();
        let b = c.get("baseline");
        let mut read = |key: &'static str| match b.and_then(|b| b.get(key)) {
            None | Some(serde_json::Value::Null) => None,
            Some(v) => {
                let n = v.as_f64().and_then(positive);
                if n.is_none() {
                    invalid.push(key);
                }
                n
            }
        };
        let out = Self {
            cost_per_unit_usd: read("cost_per_unit_usd"),
            manual_hours_per_unit: read("manual_hours_per_unit"),
        };
        (out, invalid)
    }
}

/// Leads of the goal by stage. Simulated leads are counted apart and never
/// enter the outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct LeadFacts {
    pub cold: u64,
    pub interested: u64,
    pub qualified: u64,
    pub converted: u64,
    pub lost: u64,
    pub simulated: u64,
}

impl LeadFacts {
    pub fn real_total(&self) -> u64 {
        self.cold + self.interested + self.qualified + self.converted + self.lost
    }
    /// Leads that reached at least `interested`.
    pub fn at_least_interested(&self) -> u64 {
        self.interested + self.qualified + self.converted
    }
    /// Leads that reached at least `qualified`.
    pub fn at_least_qualified(&self) -> u64 {
        self.qualified + self.converted
    }
}

/// Deals opened from the goal's real leads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DealFacts {
    pub won: u64,
    pub open: u64,
    pub lost: u64,
    /// Won revenue per currency; currencies are never summed together.
    pub revenue_by_currency: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ExperimentFacts {
    pub total: u64,
    pub stopped: u64,
    pub pivoted: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FailureReason {
    /// `tool`, `policy`, `budget`, `execution`, `model`, `run`, `deal`.
    pub source: String,
    pub reason: String,
    pub count: u64,
}

/// Everything the report is computed from; collected by storage.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalAnalyticsFacts {
    pub spec: GoalSpec,
    pub runs: Vec<GoalRun>,
    pub tool_calls: ToolCallFacts,
    pub journal: JournalFacts,
    pub executions: ExecutionFacts,
    pub spend: SpendFacts,
    pub external: ExternalSpendFacts,
    pub leads: LeadFacts,
    pub deals: DealFacts,
    pub experiments: ExperimentFacts,
    pub failures: Vec<FailureReason>,
    pub now: DateTime<Utc>,
}

// --------------------------------------------------------------- report ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyticsNote {
    pub code: String,
    pub message: String,
}

fn note(code: &str, message: impl Into<String>) -> AnalyticsNote {
    AnalyticsNote {
        code: code.into(),
        message: message.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalSummary {
    pub id: Uuid,
    pub title: String,
    pub text: String,
    pub metric: String,
    pub target: f64,
    pub deadline: Option<DateTime<Utc>>,
    /// `None` when no budget was set.
    pub budget_usd: Option<f64>,
    pub max_daily_spend_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostReport {
    pub planned_budget_usd: Option<f64>,
    /// Operator estimate from `constraints.planned_cost_usd`; `None` when
    /// none was given (see gaps).
    pub planned_estimate_usd: Option<f64>,
    /// Model calls + external spend in USD.
    pub actual_usd: f64,
    pub model_usd: f64,
    pub external_usd: f64,
    /// All recorded external spend, by currency (non-USD is not converted).
    pub external_by_currency: BTreeMap<String, f64>,
    pub external_entries: u64,
    /// `actual_usd - planned_estimate_usd`.
    pub estimate_deviation_usd: Option<f64>,
    pub unpriced_calls: u64,
    /// `actual_usd` misses unpriced calls, so the real spend is higher.
    pub actual_is_lower_bound: bool,
    pub budget_used_pct: Option<f64>,
    pub basis: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalReport {
    pub runs_by_status: BTreeMap<String, u64>,
    pub ticks: u64,
    pub tool_calls: ToolCallFacts,
    pub executions: ExecutionFacts,
    pub model_calls: u64,
    pub model_calls_failed: u64,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub tasks_denied: u64,
    /// Started and not finished, or with an unknown outcome.
    pub tasks_unresolved: u64,
    /// completed / (completed + failed); `None` without finished tasks.
    pub success_rate: Option<f64>,
    /// failed operations (tasks + model calls) / all finished operations.
    pub error_rate: Option<f64>,
    pub duration_secs: Option<i64>,
    pub cost: CostReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Reached,
    NotReached,
    InProgress,
    InsufficientData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricBasis {
    /// Counted from leads / deals / executions.
    Verified,
    /// Only the agent's own counter exists for this metric.
    SelfReported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeReport {
    pub metric: String,
    pub target: f64,
    pub basis: MetricBasis,
    /// Verified value of the goal metric; `None` when it can not be verified.
    pub achieved: Option<f64>,
    /// Highest `run.current` the agent recorded; shown, never trusted.
    pub reported_by_agent: Option<f64>,
    pub deviation: Option<f64>,
    pub progress_pct: Option<f64>,
    pub status: OutcomeStatus,
    pub deadline_passed: bool,
    pub leads_total: u64,
    pub leads_interested: u64,
    pub leads_qualified: u64,
    pub leads_converted: u64,
    pub leads_simulated_excluded: u64,
    pub interested_to_qualified: Option<f64>,
    pub qualified_to_won: Option<f64>,
    pub lead_to_won: Option<f64>,
    pub deals_won: u64,
    pub deals_open: u64,
    pub deals_lost: u64,
    pub revenue_by_currency: BTreeMap<String, f64>,
    pub cost_per_qualified_lead_usd: Option<f64>,
    pub cost_per_won_deal_usd: Option<f64>,
    /// USD: `baseline.cost_per_unit_usd * achieved - actual_usd`; `None`
    /// when the baseline, the verified value or a complete USD cost is missing.
    pub cost_reduction: Option<f64>,
    /// `baseline.manual_hours_per_unit * achieved`; `None` without baseline
    /// or verified value.
    pub time_saved_hours: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalAnalyticsReport {
    pub goal: GoalSummary,
    pub generated_at: DateTime<Utc>,
    pub operational: OperationalReport,
    pub outcome: OutcomeReport,
    pub failure_reasons: Vec<FailureReason>,
    pub recommendations: Vec<AnalyticsNote>,
    pub data_gaps: Vec<AnalyticsNote>,
}

/// One row of the cross-goal overview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalAnalyticsRow {
    pub goal_id: Uuid,
    pub title: String,
    pub metric: String,
    pub target: f64,
    pub achieved: Option<f64>,
    pub status: OutcomeStatus,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub actual_usd: f64,
    pub actual_is_lower_bound: bool,
    pub budget_usd: Option<f64>,
    pub data_gaps: usize,
}

impl From<&GoalAnalyticsReport> for GoalAnalyticsRow {
    fn from(r: &GoalAnalyticsReport) -> Self {
        Self {
            goal_id: r.goal.id,
            title: r.goal.title.clone(),
            metric: r.outcome.metric.clone(),
            target: r.outcome.target,
            achieved: r.outcome.achieved,
            status: r.outcome.status,
            tasks_completed: r.operational.tasks_completed,
            tasks_failed: r.operational.tasks_failed,
            actual_usd: r.operational.cost.actual_usd,
            actual_is_lower_bound: r.operational.cost.actual_is_lower_bound,
            budget_usd: r.goal.budget_usd,
            data_gaps: r.data_gaps.len(),
        }
    }
}

// -------------------------------------------------------------- compute ----

fn ratio(num: u64, den: u64) -> Option<f64> {
    (den > 0).then(|| num as f64 / den as f64)
}

fn positive(v: f64) -> Option<f64> {
    (v.is_finite() && v > 0.0).then_some(v)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn is_terminal(s: GoalRunStatus) -> bool {
    matches!(
        s,
        GoalRunStatus::Succeeded | GoalRunStatus::Failed | GoalRunStatus::Stopped
    )
}

/// Verified value of `metric`, or `None` when no record can back it.
fn verified_metric(
    metric: &str,
    f: &GoalAnalyticsFacts,
    gaps: &mut Vec<AnalyticsNote>,
) -> Option<f64> {
    match metric {
        "leads_interested" => Some(f.leads.at_least_interested() as f64),
        "leads_qualified" => Some(f.leads.at_least_qualified() as f64),
        "leads_converted" => Some(f.leads.converted as f64),
        "leads_total" | "leads" => Some(f.leads.real_total() as f64),
        "deals_won" => Some(f.deals.won as f64),
        "revenue" => match f.deals.revenue_by_currency.len() {
            0 => Some(0.0),
            1 => f.deals.revenue_by_currency.values().next().copied(),
            _ => {
                gaps.push(note(
                    "revenue_mixed_currencies",
                    "Won revenue is in several currencies with no exchange rate; the revenue goal can not be compared to its target.",
                ));
                None
            }
        },
        "scenarios_validated" | "execution.succeeded_count" => {
            Some(f.executions.workflows_with_completed as f64)
        }
        other => {
            gaps.push(note(
                "metric_not_verifiable",
                format!(
                    "Metric `{other}` has no verified data source; only the agent's own counter exists, so goal attainment is not assessed."
                ),
            ));
            None
        }
    }
}

pub fn compute_goal_report(f: &GoalAnalyticsFacts) -> GoalAnalyticsReport {
    let spec = &f.spec;
    let mut gaps: Vec<AnalyticsNote> = Vec::new();
    let mut recs: Vec<AnalyticsNote> = Vec::new();

    let budget = positive(spec.policy.budget_usd);
    let daily = positive(spec.policy.max_daily_spend_usd);

    // ---- operational ----
    let mut runs_by_status: BTreeMap<String, u64> = BTreeMap::new();
    for r in &f.runs {
        *runs_by_status.entry(r.status.as_str().into()).or_default() += 1;
    }
    let ticks: u64 = f.runs.iter().map(|r| r.tick_count as u64).sum();

    let tasks_completed = f.tool_calls.succeeded + f.executions.completed;
    let tasks_failed = f.tool_calls.failed + f.executions.failed + f.executions.degraded;
    let tasks_unresolved = f.journal.unresolved + f.executions.unfinished;
    let success_rate = ratio(tasks_completed, tasks_completed + tasks_failed);
    let finished_ops = tasks_completed + tasks_failed + f.spend.calls;
    let error_rate = ratio(tasks_failed + f.spend.failed_calls, finished_ops);

    let duration_secs = f.runs.iter().map(|r| r.started_at).min().map(|start| {
        let end = if f.runs.iter().all(|r| is_terminal(r.status)) {
            f.runs
                .iter()
                .map(|r| r.finished_at.unwrap_or(f.now))
                .max()
                .unwrap_or(f.now)
        } else {
            f.now
        };
        (end - start).num_seconds().max(0)
    });

    let model_usd = round_usd(f.spend.priced_usd);
    let external_usd = round_usd(f.external.by_currency.get("USD").copied().unwrap_or(0.0));
    let external_non_usd: Vec<&String> = f
        .external
        .by_currency
        .iter()
        .filter(|(c, v)| c.as_str() != "USD" && **v > 0.0)
        .map(|(c, _)| c)
        .collect();
    let actual_usd = round_usd(model_usd + external_usd);
    let lower_bound = f.spend.unpriced_calls > 0;
    let planned_raw = spec.constraints.get("planned_cost_usd");
    let planned_estimate = planned_raw.and_then(|v| v.as_f64()).and_then(positive);
    let cost = CostReport {
        planned_budget_usd: budget,
        planned_estimate_usd: planned_estimate,
        actual_usd,
        model_usd,
        external_usd,
        external_by_currency: f.external.by_currency.clone(),
        external_entries: f.external.entries,
        estimate_deviation_usd: planned_estimate.map(|p| round_usd(actual_usd - p)),
        unpriced_calls: f.spend.unpriced_calls,
        actual_is_lower_bound: lower_bound,
        budget_used_pct: budget.map(|b| round2(actual_usd / b * 100.0)),
        basis: COST_BASIS_LIST_PRICE.into(),
    };
    match (planned_raw, planned_estimate) {
        (_, Some(_)) => {}
        (None | Some(serde_json::Value::Null), None) => gaps.push(note(
            "no_planned_cost_estimate",
            "No planned cost is set (constraints.planned_cost_usd); only the budget limit can be compared to actual spend.",
        )),
        (Some(_), None) => gaps.push(note(
            "planned_cost_invalid",
            "constraints.planned_cost_usd is not a positive number and is ignored.",
        )),
    }
    if lower_bound {
        gaps.push(note(
            "unpriced_model_calls",
            format!(
                "{} model call(s) have no price; actual spend is a lower bound.",
                f.spend.unpriced_calls
            ),
        ));
    }
    if f.external.entries == 0 {
        gaps.push(note(
            "external_spend_not_tracked",
            "No ad spend or external service cost is recorded for this goal; actual cost covers model calls only.",
        ));
    }
    if !external_non_usd.is_empty() {
        gaps.push(note(
            "external_spend_non_usd",
            format!(
                "External spend in {} is not converted and is not included in actual_usd.",
                external_non_usd.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ")
            ),
        ));
    }

    // ---- outcome ----
    let target = spec.target;
    let deadline_passed = spec.deadline.is_some_and(|d| d <= f.now);
    let reported_by_agent = f
        .runs
        .iter()
        .map(|r| r.current)
        .fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.max(v))));

    let achieved = verified_metric(&spec.metric, f, &mut gaps);
    let basis = if achieved.is_some() {
        MetricBasis::Verified
    } else {
        MetricBasis::SelfReported
    };
    let target_set = target.is_finite() && target > 0.0;
    if !target_set {
        gaps.push(note(
            "target_not_set",
            "The goal has no positive target; attainment can not be judged.",
        ));
    }

    let status = match achieved {
        _ if f.runs.is_empty() => OutcomeStatus::InsufficientData,
        _ if !target_set => OutcomeStatus::InsufficientData,
        None => OutcomeStatus::InsufficientData,
        Some(v) if v >= target => OutcomeStatus::Reached,
        Some(_) if deadline_passed || f.runs.iter().all(|r| is_terminal(r.status)) => {
            OutcomeStatus::NotReached
        }
        Some(_) => OutcomeStatus::InProgress,
    };
    let deviation = achieved.filter(|_| target_set).map(|v| round2(v - target));
    let progress_pct = achieved
        .filter(|_| target_set)
        .map(|v| round2(v / target * 100.0));

    let leads_interested = f.leads.at_least_interested();
    let leads_qualified = f.leads.at_least_qualified();
    let leads_total = f.leads.real_total();

    let has_cost = f.spend.calls > 0 || f.external.entries > 0;
    let cost_per = |n: u64| (n > 0 && has_cost).then(|| round_usd(actual_usd / n as f64));

    let (baseline, invalid_baseline) = Baseline::from_constraints(&spec.constraints);
    for key in &invalid_baseline {
        gaps.push(note(
            "baseline_invalid",
            format!("constraints.baseline.{key} is not a positive number and is ignored."),
        ));
    }
    let cost_reduction = match (baseline.cost_per_unit_usd, achieved) {
        (None, _) => {
            gaps.push(note(
                "cost_reduction_not_measured",
                "No cost baseline is recorded (constraints.baseline.cost_per_unit_usd), so cost reduction is not measured.",
            ));
            None
        }
        (Some(_), None) => {
            gaps.push(note(
                "cost_reduction_not_measured",
                "The goal value is not verified, so cost reduction against the baseline is not measured.",
            ));
            None
        }
        (Some(_), Some(_)) if lower_bound || !external_non_usd.is_empty() => {
            gaps.push(note(
                "cost_reduction_not_measured",
                "Actual cost in USD is incomplete (unpriced calls or unconverted external spend), so cost reduction is not measured.",
            ));
            None
        }
        (Some(per_unit), Some(v)) => Some(round_usd(per_unit * v - actual_usd)),
    };
    let time_saved_hours = match (baseline.manual_hours_per_unit, achieved) {
        (Some(h), Some(v)) => Some(round2(h * v)),
        (None, _) => {
            gaps.push(note(
                "time_saved_not_measured",
                "No manual-effort baseline is recorded (constraints.baseline.manual_hours_per_unit), so time saved is not measured.",
            ));
            None
        }
        (Some(_), None) => {
            gaps.push(note(
                "time_saved_not_measured",
                "The goal value is not verified, so time saved against the baseline is not measured.",
            ));
            None
        }
    };
    if f.runs.is_empty() {
        gaps.push(note("no_runs", "The goal has never been started; there is nothing to evaluate."));
    }
    if f.leads.simulated > 0 {
        gaps.push(note(
            "simulated_leads_excluded",
            format!("{} simulated lead(s) are excluded from the outcome.", f.leads.simulated),
        ));
    }

    let outcome = OutcomeReport {
        metric: spec.metric.clone(),
        target,
        basis,
        achieved,
        reported_by_agent,
        deviation,
        progress_pct,
        status,
        deadline_passed,
        leads_total,
        leads_interested,
        leads_qualified,
        leads_converted: f.leads.converted,
        leads_simulated_excluded: f.leads.simulated,
        interested_to_qualified: ratio(leads_qualified, leads_interested),
        qualified_to_won: ratio(f.deals.won, leads_qualified),
        lead_to_won: ratio(f.deals.won, leads_total),
        deals_won: f.deals.won,
        deals_open: f.deals.open,
        deals_lost: f.deals.lost,
        revenue_by_currency: f.deals.revenue_by_currency.clone(),
        cost_per_qualified_lead_usd: cost_per(leads_qualified),
        cost_per_won_deal_usd: cost_per(f.deals.won),
        cost_reduction,
        time_saved_hours,
    };

    // ---- failure reasons ----
    let mut failure_reasons = f.failures.clone();
    failure_reasons.retain(|r| r.count > 0);
    failure_reasons.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.reason.cmp(&b.reason))
    });
    failure_reasons.truncate(MAX_FAILURE_REASONS);

    // ---- recommendations ----
    let progressed = achieved.is_some_and(|v| v > 0.0);
    if f.runs.is_empty() {
        recs.push(note("start_goal", "Start the goal: no run exists, so there is no result to evaluate."));
    }
    if status == OutcomeStatus::Reached {
        recs.push(note(
            "goal_reached",
            "The goal is reached on verified data. Set the next target or stop the run to avoid further spend.",
        ));
    }
    if tasks_completed > 0 && achieved == Some(0.0) {
        recs.push(note(
            "activity_without_result",
            format!(
                "{tasks_completed} task(s) completed but `{}` is still 0. Change the channel, offer or audience instead of repeating the same actions.",
                spec.metric
            ),
        ));
    }
    if error_rate.is_some_and(|r| r > HIGH_ERROR_RATE) {
        recs.push(note(
            "high_error_rate",
            "More than 30% of operations fail. Fix the top failure reasons before spending more.",
        ));
    }
    if tasks_completed == 0 && f.tool_calls.denied > 0 {
        recs.push(note(
            "only_denied_actions",
            "Every attempted action was refused by policy, approval or budget. Review permissions and pending approvals.",
        ));
    }
    if lower_bound {
        recs.push(note(
            "price_unpriced_models",
            "Add prices for the models used so spend and budget checks are complete.",
        ));
    }
    match (budget, cost.budget_used_pct) {
        (None, _) => recs.push(note(
            "set_budget",
            "Set a budget for the goal so spend can be limited and judged.",
        )),
        (Some(_), Some(pct)) if pct >= BUDGET_WARN_PCT && status != OutcomeStatus::Reached => {
            recs.push(note(
                "budget_nearly_spent",
                format!(
                    "{pct:.0}% of the budget is spent and the goal is not reached. Re-plan or stop before the budget runs out."
                ),
            ))
        }
        _ => {}
    }
    if leads_interested > 0 && leads_qualified == 0 {
        recs.push(note(
            "qualify_interested_leads",
            "Interested leads exist but none is qualified. Follow up with them and qualify before generating more.",
        ));
    }
    if leads_qualified > 0 && f.deals.won == 0 {
        recs.push(note(
            "convert_qualified_leads",
            "Qualified leads exist but no deal is won. Focus the next cycle on proposals and closing.",
        ));
    }
    if deadline_passed && status != OutcomeStatus::Reached {
        recs.push(note(
            "deadline_passed",
            "The deadline has passed without reaching the goal. Decide: extend the deadline with a new plan, or stop.",
        ));
    }
    if tasks_unresolved > 0 {
        recs.push(note(
            "resolve_unfinished_calls",
            format!(
                "{tasks_unresolved} operation(s) are unfinished or have an unknown outcome. Check them before retrying anything irreversible."
            ),
        ));
    }
    if let (Some(v), Some(rep)) = (achieved, reported_by_agent) {
        if (rep - v).abs() > f64::EPSILON {
            recs.push(note(
                "agent_counter_mismatch",
                format!(
                    "The agent recorded {rep} for `{}` but verified data shows {v}. Trust the verified value.",
                    spec.metric
                ),
            ));
        }
    }
    if f.runs.iter().any(|r| r.status == GoalRunStatus::Succeeded) && status != OutcomeStatus::Reached {
        recs.push(note(
            "run_success_unverified",
            "A run is marked succeeded, but verified data does not confirm the goal.",
        ));
    }
    if f.experiments.stopped + f.experiments.pivoted > 0 && !progressed {
        recs.push(note(
            "rethink_hypotheses",
            format!(
                "{} experiment(s) were stopped or pivoted with no progress. Pick a different hypothesis rather than a variation.",
                f.experiments.stopped + f.experiments.pivoted
            ),
        ));
    }
    if daily.is_none() && budget.is_some() {
        gaps.push(note(
            "no_daily_limit",
            "No daily spend limit is set; the whole budget can be used in one day.",
        ));
    }

    GoalAnalyticsReport {
        goal: GoalSummary {
            id: spec.id,
            title: spec.title.clone(),
            text: spec.text.clone(),
            metric: spec.metric.clone(),
            target,
            deadline: spec.deadline,
            budget_usd: budget,
            max_daily_spend_usd: daily,
        },
        generated_at: f.now,
        operational: OperationalReport {
            runs_by_status,
            ticks,
            tool_calls: f.tool_calls,
            executions: f.executions,
            model_calls: f.spend.calls,
            model_calls_failed: f.spend.failed_calls,
            tasks_completed,
            tasks_failed,
            tasks_denied: f.tool_calls.denied,
            tasks_unresolved,
            success_rate,
            error_rate,
            duration_secs,
            cost,
        },
        outcome,
        failure_reasons,
        recommendations: recs,
        data_gaps: gaps,
    }
}

fn round_usd(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use serde_json::json;

    fn spec(metric: &str, target: f64, budget: f64) -> GoalSpec {
        let mut s: GoalSpec = serde_json::from_value(json!({
            "id": Uuid::nil(),
            "title": "Get clients",
            "text": "Find 10 qualified leads",
            "metric": metric,
            "target": target,
            "constraints": {},
            "policy": {},
            "agent_type_id": "agent.growth",
            "created_at": Utc::now(),
            "updated_at": Utc::now(),
        }))
        .unwrap_or_else(|e| panic!("spec fixture: {e}"));
        s.policy.budget_usd = budget;
        s
    }

    fn run(status: GoalRunStatus, current: f64, now: DateTime<Utc>) -> GoalRun {
        GoalRun {
            id: Uuid::now_v7(),
            goal_id: Uuid::nil(),
            status,
            current,
            target: 10.0,
            strategy: Default::default(),
            actions_today: 0,
            actions_day: String::new(),
            tick_count: 5,
            last_tick_at: None,
            next_tick_at: None,
            last_observe: None,
            last_think: None,
            error: None,
            started_at: now - Duration::hours(2),
            finished_at: None,
            replan: Default::default(),
        }
    }

    fn facts(spec: GoalSpec, runs: Vec<GoalRun>, now: DateTime<Utc>) -> GoalAnalyticsFacts {
        GoalAnalyticsFacts {
            spec,
            runs,
            tool_calls: Default::default(),
            journal: Default::default(),
            executions: Default::default(),
            spend: Default::default(),
            external: Default::default(),
            leads: Default::default(),
            deals: Default::default(),
            experiments: Default::default(),
            failures: vec![],
            now,
        }
    }

    fn has(notes: &[AnalyticsNote], code: &str) -> bool {
        notes.iter().any(|n| n.code == code)
    }

    #[test]
    fn many_tasks_without_clients_is_not_success() {
        let now = Utc::now();
        // Agent claims 12 via its own counter and the run is marked succeeded.
        let mut f = facts(spec("leads_qualified", 10.0, 5.0), vec![run(GoalRunStatus::Succeeded, 12.0, now)], now);
        f.tool_calls.succeeded = 50;
        f.spend = SpendFacts { calls: 20, failed_calls: 0, priced_usd: 4.5, unpriced_calls: 0 };
        let r = compute_goal_report(&f);
        assert_eq!(r.operational.tasks_completed, 50);
        assert_eq!(r.operational.success_rate, Some(1.0));
        assert_eq!(r.outcome.achieved, Some(0.0));
        assert_eq!(r.outcome.reported_by_agent, Some(12.0));
        assert_eq!(r.outcome.status, OutcomeStatus::NotReached);
        assert_eq!(r.outcome.cost_per_qualified_lead_usd, None);
        assert!(has(&r.recommendations, "activity_without_result"));
        assert!(has(&r.recommendations, "agent_counter_mismatch"));
        assert!(has(&r.recommendations, "run_success_unverified"));
        assert!(has(&r.recommendations, "budget_nearly_spent"));
    }

    #[test]
    fn no_runs_is_insufficient_data() {
        let now = Utc::now();
        let r = compute_goal_report(&facts(spec("leads_interested", 10.0, 0.0), vec![], now));
        assert_eq!(r.outcome.status, OutcomeStatus::InsufficientData);
        assert_eq!(r.operational.success_rate, None);
        assert_eq!(r.operational.error_rate, None);
        assert_eq!(r.operational.duration_secs, None);
        assert_eq!(r.operational.cost.budget_used_pct, None);
        assert!(has(&r.data_gaps, "no_runs"));
        assert!(has(&r.recommendations, "start_goal"));
        assert!(has(&r.recommendations, "set_budget"));
    }

    #[test]
    fn unknown_metric_is_not_judged_by_agent_counter() {
        let now = Utc::now();
        let r = compute_goal_report(&facts(spec("brand_awareness", 10.0, 5.0), vec![run(GoalRunStatus::Running, 50.0, now)], now));
        assert_eq!(r.outcome.basis, MetricBasis::SelfReported);
        assert_eq!(r.outcome.achieved, None);
        assert_eq!(r.outcome.reported_by_agent, Some(50.0));
        assert_eq!(r.outcome.status, OutcomeStatus::InsufficientData);
        assert!(has(&r.data_gaps, "metric_not_verifiable"));
    }

    #[test]
    fn reached_goal_with_funnel_and_costs() {
        let now = Utc::now();
        let mut f = facts(spec("leads_qualified", 2.0, 10.0), vec![run(GoalRunStatus::Running, 2.0, now)], now);
        f.leads = LeadFacts { cold: 6, interested: 2, qualified: 1, converted: 1, lost: 0, simulated: 3 };
        f.deals.won = 1;
        f.deals.revenue_by_currency.insert("RUB".into(), 5000.0);
        f.tool_calls = ToolCallFacts { succeeded: 8, failed: 2, denied: 1 };
        f.spend = SpendFacts { calls: 10, failed_calls: 0, priced_usd: 2.0, unpriced_calls: 0 };
        let r = compute_goal_report(&f);
        assert_eq!(r.outcome.status, OutcomeStatus::Reached);
        assert_eq!(r.outcome.achieved, Some(2.0));
        assert_eq!(r.outcome.leads_total, 10);
        assert_eq!(r.outcome.leads_interested, 4);
        assert_eq!(r.outcome.interested_to_qualified, Some(0.5));
        assert_eq!(r.outcome.qualified_to_won, Some(0.5));
        assert_eq!(r.outcome.lead_to_won, Some(0.1));
        assert_eq!(r.outcome.cost_per_qualified_lead_usd, Some(1.0));
        assert_eq!(r.outcome.cost_per_won_deal_usd, Some(2.0));
        assert_eq!(r.outcome.leads_simulated_excluded, 3);
        assert_eq!(r.operational.success_rate, Some(0.8));
        assert_eq!(r.operational.error_rate, Some(0.1));
        assert_eq!(r.operational.cost.budget_used_pct, Some(20.0));
        assert!(has(&r.recommendations, "goal_reached"));
        assert!(!has(&r.recommendations, "agent_counter_mismatch"));
        assert!(has(&r.data_gaps, "simulated_leads_excluded"));
        assert!(has(&r.data_gaps, "no_planned_cost_estimate"));
        assert!(has(&r.data_gaps, "cost_reduction_not_measured"));
        assert_eq!(r.outcome.cost_reduction, None);
        assert_eq!(r.outcome.time_saved_hours, None);
    }

    #[test]
    fn unpriced_calls_make_spend_a_lower_bound() {
        let now = Utc::now();
        let mut f = facts(spec("leads_interested", 10.0, 1.0), vec![run(GoalRunStatus::Running, 0.0, now)], now);
        f.spend = SpendFacts { calls: 4, failed_calls: 3, priced_usd: 0.1, unpriced_calls: 2 };
        let r = compute_goal_report(&f);
        assert!(r.operational.cost.actual_is_lower_bound);
        assert_eq!(r.operational.cost.planned_estimate_usd, None);
        assert_eq!(r.operational.error_rate, Some(0.75));
        assert!(has(&r.data_gaps, "external_spend_not_tracked"));
        assert!(has(&r.data_gaps, "unpriced_model_calls"));
        assert!(has(&r.recommendations, "price_unpriced_models"));
        assert!(has(&r.recommendations, "high_error_rate"));
    }

    #[test]
    fn deadline_passed_and_mixed_revenue() {
        let now = Utc::now();
        let mut s = spec("revenue", 1000.0, 5.0);
        s.deadline = Some(now - Duration::days(1));
        let mut f = facts(s, vec![run(GoalRunStatus::Running, 0.0, now)], now);
        f.deals.revenue_by_currency.insert("RUB".into(), 500.0);
        f.deals.revenue_by_currency.insert("USD".into(), 50.0);
        let r = compute_goal_report(&f);
        assert!(r.outcome.deadline_passed);
        assert_eq!(r.outcome.status, OutcomeStatus::InsufficientData);
        assert!(has(&r.data_gaps, "revenue_mixed_currencies"));
        assert!(has(&r.recommendations, "deadline_passed"));

        let mut f2 = f.clone();
        f2.deals.revenue_by_currency.remove("USD");
        let r2 = compute_goal_report(&f2);
        assert_eq!(r2.outcome.achieved, Some(500.0));
        assert_eq!(r2.outcome.status, OutcomeStatus::NotReached);
        assert_eq!(r2.outcome.deviation, Some(-500.0));
    }

    #[test]
    fn external_spend_baseline_and_planned_estimate() {
        let now = Utc::now();
        let mut s = spec("leads_qualified", 4.0, 100.0);
        s.constraints = json!({
            "planned_cost_usd": 40.0,
            "baseline": { "cost_per_unit_usd": 25.0, "manual_hours_per_unit": 1.5 }
        });
        let mut f = facts(s, vec![run(GoalRunStatus::Running, 4.0, now)], now);
        f.leads = LeadFacts { qualified: 4, ..Default::default() };
        f.spend = SpendFacts { calls: 10, failed_calls: 0, priced_usd: 5.0, unpriced_calls: 0 };
        f.external.entries = 2;
        f.external.by_currency.insert("USD".into(), 45.0);
        let r = compute_goal_report(&f);
        let c = &r.operational.cost;
        assert_eq!(c.model_usd, 5.0);
        assert_eq!(c.external_usd, 45.0);
        assert_eq!(c.actual_usd, 50.0);
        assert_eq!(c.planned_estimate_usd, Some(40.0));
        assert_eq!(c.estimate_deviation_usd, Some(10.0));
        assert_eq!(c.budget_used_pct, Some(50.0));
        assert_eq!(r.outcome.cost_per_qualified_lead_usd, Some(12.5));
        assert_eq!(r.outcome.cost_reduction, Some(50.0));
        assert_eq!(r.outcome.time_saved_hours, Some(6.0));
        for code in [
            "no_planned_cost_estimate",
            "external_spend_not_tracked",
            "cost_reduction_not_measured",
            "time_saved_not_measured",
        ] {
            assert!(!has(&r.data_gaps, code), "{code}");
        }

        // Unconverted spend makes the USD total incomplete.
        let mut f2 = f.clone();
        f2.external.by_currency.insert("RUB".into(), 3000.0);
        let r2 = compute_goal_report(&f2);
        assert_eq!(r2.operational.cost.actual_usd, 50.0);
        assert!(has(&r2.data_gaps, "external_spend_non_usd"));
        assert_eq!(r2.outcome.cost_reduction, None);
        assert!(has(&r2.data_gaps, "cost_reduction_not_measured"));
        assert_eq!(r2.outcome.time_saved_hours, Some(6.0));

        // Unpriced model calls: same.
        let mut f3 = f.clone();
        f3.spend.unpriced_calls = 1;
        assert_eq!(compute_goal_report(&f3).outcome.cost_reduction, None);
    }

    #[test]
    fn invalid_baseline_and_planned_cost_are_reported() {
        let now = Utc::now();
        let mut s = spec("brand_awareness", 4.0, 0.0);
        s.constraints = json!({
            "planned_cost_usd": "lots",
            "baseline": { "cost_per_unit_usd": -3, "manual_hours_per_unit": 2 }
        });
        let r = compute_goal_report(&facts(s, vec![run(GoalRunStatus::Running, 1.0, now)], now));
        assert!(has(&r.data_gaps, "planned_cost_invalid"));
        assert!(has(&r.data_gaps, "baseline_invalid"));
        assert!(has(&r.data_gaps, "cost_reduction_not_measured"));
        // Baseline exists but the metric is not verifiable.
        assert!(has(&r.data_gaps, "time_saved_not_measured"));
        assert_eq!(r.outcome.time_saved_hours, None);
        assert_eq!(r.operational.cost.planned_estimate_usd, None);
    }

    #[test]
    fn new_external_spend_is_validated() {
        let now = Utc::now();
        let ok = NewExternalSpend {
            amount: 12.5,
            currency: " usd ".into(),
            category: "".into(),
            description: " ads ".into(),
            occurred_at: None,
        };
        let e = ok.clone().into_entry(Uuid::nil(), now).unwrap();
        assert_eq!(e.currency, "USD");
        assert_eq!(e.category, "other");
        assert_eq!(e.description, "ads");
        assert_eq!(e.occurred_at, now);
        for bad in [
            NewExternalSpend { amount: 0.0, ..ok.clone() },
            NewExternalSpend { amount: f64::NAN, ..ok.clone() },
            NewExternalSpend { currency: "dollars".into(), ..ok.clone() },
            NewExternalSpend { occurred_at: Some(now + Duration::days(1)), ..ok.clone() },
        ] {
            assert!(bad.into_entry(Uuid::nil(), now).is_err());
        }
    }

    #[test]
    fn failure_reasons_sorted_and_capped() {
        let now = Utc::now();
        let mut f = facts(spec("leads_interested", 1.0, 1.0), vec![run(GoalRunStatus::Running, 0.0, now)], now);
        f.failures = (0..15)
            .map(|i| FailureReason { source: "tool".into(), reason: format!("e{i}"), count: i })
            .collect();
        let r = compute_goal_report(&f);
        assert_eq!(r.failure_reasons.len(), 10);
        assert_eq!(r.failure_reasons[0].count, 14);
        assert!(r.failure_reasons.iter().all(|x| x.count > 0));
    }
}
