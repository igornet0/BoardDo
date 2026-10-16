//! Execution ledger runtime: records every model call (tokens, $ and latency)
//! with the objective / item / goal run / workflow execution that caused it.
//!
//! Attribution travels as a task-local [`LedgerScope`]: callers wrap work in
//! [`scoped`], nested scopes merge, and code that crosses a `tokio::spawn`
//! captures [`current`] before spawning and re-enters it inside the task.
//! Writes go through a background task so a slow database never delays a
//! model reply; before [`init`] (e.g. in unit tests) entries are dropped.
//!
//! Budget limits travel the same way ([`budgeted`]). A call made under a limit
//! first [`reserve`]s its worst-case cost; the reservation is refused when
//! recorded spend + open reservations + worst case would exceed the limit, or
//! when spend can not be established. The call's actual cost is written
//! synchronously ([`record_now`]) before the reservation is released, so
//! concurrent calls never see the budget larger than it is.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use boarddo_shared::v2::{LedgerEntry, LedgerScope, estimate_cost_usd};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use uuid::Uuid;

use crate::storage::{GoalSpend, Storage};

static SINK: OnceLock<UnboundedSender<LedgerEntry>> = OnceLock::new();
static GUARD: OnceLock<BudgetGuard> = OnceLock::new();

tokio::task_local! {
    static SCOPE: LedgerScope;
    static BUDGET: Vec<BudgetLimit>;
}

/// Starts the writer task. Call once after migrations.
pub fn init(storage: Storage) {
    let _ = GUARD.set(BudgetGuard::new(storage.clone()));
    let (tx, mut rx) = unbounded_channel::<LedgerEntry>();
    if SINK.set(tx).is_err() {
        return;
    }
    tokio::spawn(async move {
        while let Some(entry) = rx.recv().await {
            if let Err(err) = storage.insert_ledger_entry(&entry).await {
                tracing::warn!(error = %err, model = %entry.model, "ledger.write_failed");
            }
        }
    });
}

/// The attribution of the current task, if any scope is active.
pub fn current() -> Option<LedgerScope> {
    SCOPE.try_with(|s| s.clone()).ok()
}

/// Runs `fut` with `scope` merged over the current one.
pub async fn scoped<F: Future>(scope: LedgerScope, fut: F) -> F::Output {
    let merged = current().unwrap_or_default().merged(&scope);
    SCOPE.scope(merged, fut).await
}

/// Re-enters a scope captured with [`current`] (e.g. inside `tokio::spawn`).
pub async fn within<F: Future>(scope: Option<LedgerScope>, fut: F) -> F::Output {
    match scope {
        Some(s) => scoped(s, fut).await,
        None => fut.await,
    }
}

/// What a budget is counted over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudgetKey {
    /// Every run of one goal.
    Goal(Uuid),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetLimit {
    pub key: BudgetKey,
    pub label: String,
    /// Lifetime cap.
    pub total_usd: Option<f64>,
    /// Cap per UTC day.
    pub daily_usd: Option<f64>,
}

fn positive(v: f64) -> Option<f64> {
    (v.is_finite() && v > 0.0).then_some(v)
}

impl BudgetLimit {
    /// `None` when neither cap is set (0 means "no cap" in goal policy).
    pub fn for_goal(goal_id: Uuid, label: &str, total_usd: f64, daily_usd: f64) -> Option<Self> {
        let (total_usd, daily_usd) = (positive(total_usd), positive(daily_usd));
        (total_usd.is_some() || daily_usd.is_some()).then(|| Self {
            key: BudgetKey::Goal(goal_id),
            label: label.to_string(),
            total_usd,
            daily_usd,
        })
    }
}

/// Limits active for the current task (outermost first).
pub fn budget_limits() -> Vec<BudgetLimit> {
    BUDGET.try_with(|b| b.clone()).unwrap_or_default()
}

/// Runs `fut` with `limits` added to the active ones.
pub async fn budgeted<F: Future>(limits: Vec<BudgetLimit>, fut: F) -> F::Output {
    let mut all = budget_limits();
    all.extend(limits);
    BUDGET.scope(all, fut).await
}

/// Attribution and limits of the current task, to carry across `tokio::spawn`.
#[derive(Debug, Clone, Default)]
pub struct Captured {
    scope: Option<LedgerScope>,
    limits: Vec<BudgetLimit>,
}

pub fn capture() -> Captured {
    Captured {
        scope: current(),
        limits: budget_limits(),
    }
}

/// Re-enters what [`capture`] took (inside the spawned task).
pub async fn enter<F: Future>(c: Captured, fut: F) -> F::Output {
    BUDGET.scope(c.limits, within(c.scope, fut)).await
}

fn day_start(now: DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_utc())
        .unwrap_or(now)
}

type Reserved = Arc<std::sync::Mutex<HashMap<BudgetKey, f64>>>;

/// Admission control for spend under [`BudgetLimit`]s.
pub struct BudgetGuard {
    storage: Storage,
    /// Serializes check-and-reserve so two calls can not both pass on the
    /// same remaining budget.
    check: tokio::sync::Mutex<()>,
    reserved: Reserved,
}

/// Worst-case cost held against limits until dropped.
#[must_use]
pub struct Reservation {
    reserved: Reserved,
    held: Vec<(BudgetKey, f64)>,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut map = self.reserved.lock().unwrap_or_else(|e| e.into_inner());
        for (key, usd) in &self.held {
            if let Some(v) = map.get_mut(key) {
                *v -= usd;
                if *v <= 1e-12 {
                    map.remove(key);
                }
            }
        }
    }
}

fn fmt_usd(v: f64) -> String {
    format!("${v:.4}")
}

impl BudgetGuard {
    pub fn new(storage: Storage) -> Self {
        Self {
            storage,
            check: tokio::sync::Mutex::new(()),
            reserved: Arc::default(),
        }
    }

    fn reserved_for(&self, key: BudgetKey) -> f64 {
        let map = self.reserved.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&key).copied().unwrap_or(0.0)
    }

    /// Recorded spend for `limit`; `Err` when it can not be established
    /// exactly (unreadable, unpriced calls, unconverted currency).
    async fn known_spend(&self, limit: &BudgetLimit, since: DateTime<Utc>) -> Result<GoalSpend, String> {
        let BudgetKey::Goal(goal_id) = limit.key;
        let spend = self.storage.goal_spend(goal_id, since).await.map_err(|e| {
            format!("budget `{}`: spend could not be read, call refused: {e}", limit.label)
        })?;
        if spend.unpriced_calls > 0 {
            return Err(format!(
                "budget `{}`: {} recorded call(s) have unknown cost, so remaining budget is unknown; call refused",
                limit.label, spend.unpriced_calls
            ));
        }
        if !spend.unconverted_currencies.is_empty() {
            return Err(format!(
                "budget `{}`: external spend in {} is not converted to USD, so remaining budget is unknown; call refused (record it in USD)",
                limit.label,
                spend.unconverted_currencies.join(", ")
            ));
        }
        Ok(spend)
    }

    /// Checks every limit with `worst` added; `>` refuses when
    /// `strict`, `>=` otherwise (no headroom left at all).
    async fn check_limits(&self, limits: &[BudgetLimit], worst: f64, strict: bool) -> Result<(), String> {
        let since = day_start(Utc::now());
        let over = |used: f64, cap: f64| if strict { used > cap } else { used >= cap };
        let mut seen = Vec::new();
        for limit in limits {
            if seen.contains(&limit.key) {
                continue;
            }
            seen.push(limit.key);
            let spend = self.known_spend(limit, since).await?;
            let reserved = self.reserved_for(limit.key);
            if let Some(cap) = limit.total_usd {
                let used = spend.total_usd + reserved;
                if over(used + worst, cap) {
                    return Err(format!(
                        "budget `{}` exhausted: spent {} + in flight {} + this call up to {} over limit {}",
                        limit.label,
                        fmt_usd(spend.total_usd),
                        fmt_usd(reserved),
                        fmt_usd(worst),
                        fmt_usd(cap)
                    ));
                }
            }
            if let Some(cap) = limit.daily_usd {
                // Reservations are all for calls running now, i.e. today.
                let used = spend.since_usd + reserved;
                if over(used + worst, cap) {
                    return Err(format!(
                        "daily budget `{}` exhausted: spent today {} + in flight {} + this call up to {} over limit {}",
                        limit.label,
                        fmt_usd(spend.since_usd),
                        fmt_usd(reserved),
                        fmt_usd(worst),
                        fmt_usd(cap)
                    ));
                }
            }
        }
        Ok(())
    }

    pub async fn reserve(&self, limits: &[BudgetLimit], worst_usd: f64) -> Result<Reservation, String> {
        if !worst_usd.is_finite() || worst_usd < 0.0 {
            return Err(format!("budget: invalid cost estimate {worst_usd}; call refused"));
        }
        let _check = self.check.lock().await;
        self.check_limits(limits, worst_usd, true).await?;
        let mut held: Vec<(BudgetKey, f64)> = Vec::new();
        for l in limits {
            if !held.iter().any(|(k, _)| *k == l.key) {
                held.push((l.key, worst_usd));
            }
        }
        let mut map = self.reserved.lock().unwrap_or_else(|e| e.into_inner());
        for (key, usd) in &held {
            *map.entry(*key).or_insert(0.0) += usd;
        }
        drop(map);
        Ok(Reservation {
            reserved: self.reserved.clone(),
            held,
        })
    }

    /// Smallest headroom left across `limits` (recorded + in flight
    /// subtracted, never below 0). Same fail-closed rules as [`Self::reserve`]:
    /// unknown spend ⇒ `Err`.
    pub async fn remaining(&self, limits: &[BudgetLimit]) -> Result<f64, String> {
        let since = day_start(Utc::now());
        let mut left = f64::INFINITY;
        for limit in limits {
            let spend = self.known_spend(limit, since).await?;
            let reserved = self.reserved_for(limit.key);
            if let Some(cap) = limit.total_usd {
                left = left.min(cap - spend.total_usd - reserved);
            }
            if let Some(cap) = limit.daily_usd {
                left = left.min(cap - spend.since_usd - reserved);
            }
        }
        Ok(left.max(0.0))
    }

    /// `Err` when any limit is already used up (recorded + in flight).
    pub async fn headroom(&self, limits: &[BudgetLimit]) -> Result<(), String> {
        self.check_limits(limits, 0.0, false).await
    }

    pub async fn record_now(&self, entry: LedgerEntry) {
        if let Err(err) = self.storage.insert_ledger_entry(&entry).await {
            tracing::error!(error = %err, model = %entry.model, "ledger.write_failed, retrying via writer");
            record(entry);
        }
    }
}

const NO_GUARD: &str = "budget guard is not initialised, spend can not be checked; call refused";

/// Reserves `worst_usd` under the active limits. No limits ⇒ always allowed.
pub async fn reserve(worst_usd: f64) -> Result<Option<Reservation>, String> {
    let limits = budget_limits();
    if limits.is_empty() {
        return Ok(None);
    }
    match GUARD.get() {
        Some(g) => g.reserve(&limits, worst_usd).await.map(Some),
        None => Err(NO_GUARD.into()),
    }
}

/// USD still spendable under the active limits; `Ok(None)` when no limit
/// applies. `Err` when spend can not be established (caller must refuse).
pub async fn remaining_usd() -> Result<Option<f64>, String> {
    let limits = budget_limits();
    if limits.is_empty() {
        return Ok(None);
    }
    match GUARD.get() {
        Some(g) => g.remaining(&limits).await.map(Some),
        None => Err(NO_GUARD.into()),
    }
}

/// Why no further spend is allowed under the active limits.
#[derive(Debug, Clone, PartialEq)]
pub enum Exhausted {
    /// Lifetime cap reached, or spend can not be established.
    Total(String),
    /// Only today's cap is reached; spend may resume at `resume_at`.
    Daily { reason: String, resume_at: DateTime<Utc> },
}

/// `Err` when an active limit has no headroom left.
pub async fn headroom() -> Result<(), Exhausted> {
    let limits = budget_limits();
    if limits.is_empty() {
        return Ok(());
    }
    let Some(g) = GUARD.get() else {
        return Err(Exhausted::Total(NO_GUARD.into()));
    };
    let lifetime: Vec<BudgetLimit> = limits
        .iter()
        .map(|l| BudgetLimit {
            daily_usd: None,
            ..l.clone()
        })
        .collect();
    g.headroom(&lifetime).await.map_err(Exhausted::Total)?;
    g.headroom(&limits).await.map_err(|reason| Exhausted::Daily {
        reason,
        resume_at: day_start(Utc::now()) + chrono::Duration::days(1),
    })
}

/// Writes `entry` before returning (budgeted calls), else queues it.
pub async fn record_now(entry: LedgerEntry) {
    match GUARD.get() {
        Some(g) => g.record_now(entry).await,
        None => record(entry),
    }
}

/// Builds the ledger row for one chat call from its OpenAI-shaped `usage`.
pub fn chat_entry(
    provider: &str,
    model: &str,
    usage: Option<&Value>,
    duration: Duration,
    error: Option<&str>,
) -> LedgerEntry {
    let tokens = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let input_tokens = tokens("prompt_tokens");
    let output_tokens = tokens("completion_tokens");
    LedgerEntry {
        id: Uuid::new_v4(),
        at: Utc::now(),
        provider: provider.to_string(),
        model: model.to_string(),
        kind: "chat".into(),
        input_tokens,
        output_tokens,
        cost_usd: estimate_cost_usd(model, input_tokens, output_tokens),
        duration_ms: duration.as_millis() as u64,
        ok: error.is_none(),
        error: error.map(|e| e.chars().take(500).collect()),
        scope: current().unwrap_or_default(),
    }
}

pub fn record(entry: LedgerEntry) {
    if let Some(tx) = SINK.get() {
        let _ = tx.send(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn scopes_nest_and_cross_spawn() {
        assert!(current().is_none());
        let obj = Uuid::new_v4();
        let wf = Uuid::new_v4();
        let entry = scoped(
            LedgerScope {
                objective_id: Some(obj),
                ..Default::default()
            },
            async move {
                let captured = current();
                tokio::spawn(within(captured, async move {
                    scoped(
                        LedgerScope {
                            workflow_id: Some(wf),
                            ..Default::default()
                        },
                        async {
                            chat_entry(
                                "openai",
                                "gpt-4o",
                                Some(&json!({"prompt_tokens": 1000, "completion_tokens": 100})),
                                Duration::from_millis(42),
                                None,
                            )
                        },
                    )
                    .await
                }))
                .await
                .unwrap()
            },
        )
        .await;
        assert_eq!(entry.scope.objective_id, Some(obj));
        assert_eq!(entry.scope.workflow_id, Some(wf));
        assert_eq!(entry.input_tokens, 1000);
        assert_eq!(entry.output_tokens, 100);
        assert_eq!(entry.duration_ms, 42);
        assert!(entry.ok);
        assert!((entry.cost_usd.unwrap() - 0.0035).abs() < 1e-9);
    }

    #[test]
    fn failed_call_without_usage() {
        let e = chat_entry("anthropic", "claude-sonnet-4-5", None, Duration::ZERO, Some("boom"));
        assert!(!e.ok);
        assert_eq!(e.error.as_deref(), Some("boom"));
        assert_eq!(e.cost_usd, Some(0.0));
        assert_eq!(e.scope, LedgerScope::default());
    }

    mod budget {
        use super::*;
        use boarddo_shared::v2::{CapabilityBudget, GoalRun, GoalRunStatus, GoalSpec, PolicyBundle};

        async fn storage() -> Storage {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            let s = Storage::from_pool(pool);
            s.migrate_goals().await.unwrap();
            s.migrate_ledger().await.unwrap();
            s.migrate_external_spend().await.unwrap();
            s
        }

        async fn goal(s: &Storage) -> Uuid {
            let now = Utc::now();
            let spec = GoalSpec {
                id: Uuid::now_v7(),
                title: "g".into(),
                text: "g".into(),
                metric: "m".into(),
                target: 1.0,
                deadline: None,
                constraints: json!({}),
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
            s.insert_goal_spec(&spec).await.unwrap();
            spec.id
        }

        async fn run(s: &Storage, goal_id: Uuid) -> Uuid {
            let now = Utc::now();
            let run = GoalRun {
                id: Uuid::now_v7(),
                goal_id,
                status: GoalRunStatus::Running,
                current: 0.0,
                target: 1.0,
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
            s.insert_goal_run(&run).await.unwrap();
            run.id
        }

        async fn spend(s: &Storage, run_id: Uuid, at: DateTime<Utc>, cost: Option<f64>) {
            let mut e = chat_entry("openai", "gpt-4o", None, Duration::ZERO, None);
            e.at = at;
            e.cost_usd = cost;
            e.scope.goal_run_id = Some(run_id);
            s.insert_ledger_entry(&e).await.unwrap();
        }

        fn limit(goal_id: Uuid, total: f64, daily: f64) -> Vec<BudgetLimit> {
            BudgetLimit::for_goal(goal_id, "g", total, daily).into_iter().collect()
        }

        #[test]
        fn zero_caps_mean_no_limit() {
            let id = Uuid::now_v7();
            assert!(BudgetLimit::for_goal(id, "g", 0.0, 0.0).is_none());
            assert!(BudgetLimit::for_goal(id, "g", -1.0, f64::NAN).is_none());
            let l = BudgetLimit::for_goal(id, "g", 0.0, 2.0).unwrap();
            assert_eq!((l.total_usd, l.daily_usd), (None, Some(2.0)));
        }

        #[tokio::test]
        async fn total_cap_counts_every_run_of_the_goal() {
            let s = storage().await;
            let g = goal(&s).await;
            let other = goal(&s).await;
            let (r1, r2, r3) = (run(&s, g).await, run(&s, g).await, run(&s, other).await);
            let old = Utc::now() - chrono::Duration::days(10);
            spend(&s, r1, old, Some(0.4)).await;
            spend(&s, r2, Utc::now(), Some(0.4)).await;
            spend(&s, r3, Utc::now(), Some(50.0)).await;
            let guard = BudgetGuard::new(s.clone());
            let limits = limit(g, 1.0, 0.0);

            drop(guard.reserve(&limits, 0.15).await.unwrap());
            let err = guard.reserve(&limits, 0.25).await.err().unwrap();
            assert!(err.starts_with("budget `g` exhausted"), "{err}");
            assert!(guard.headroom(&limits).await.is_ok());

            spend(&s, r1, Utc::now(), Some(0.2)).await;
            assert!(guard.headroom(&limits).await.is_err(), "spent == cap leaves no headroom");
            assert!(guard.reserve(&limits, 0.0).await.is_ok(), "zero-cost call fits exactly");
        }

        #[tokio::test]
        async fn remaining_is_tightest_cap_minus_spend_and_in_flight() {
            let s = storage().await;
            let g = goal(&s).await;
            let r = run(&s, g).await;
            spend(&s, r, day_start(Utc::now()) - chrono::Duration::hours(1), Some(3.0)).await;
            spend(&s, r, Utc::now(), Some(0.25)).await;
            let guard = BudgetGuard::new(s.clone());
            // total 10 - 3.25 = 6.75; daily 1 - 0.25 = 0.75 → daily wins.
            let limits = limit(g, 10.0, 1.0);
            assert!((guard.remaining(&limits).await.unwrap() - 0.75).abs() < 1e-9);
            let held = guard.reserve(&limits, 0.5).await.unwrap();
            assert!((guard.remaining(&limits).await.unwrap() - 0.25).abs() < 1e-9);
            drop(held);
            spend(&s, r, Utc::now(), Some(5.0)).await;
            assert_eq!(guard.remaining(&limits).await.unwrap(), 0.0, "never negative");
        }

        #[tokio::test]
        async fn remaining_refuses_when_spend_is_unknown() {
            let s = storage().await;
            let g = goal(&s).await;
            let r = run(&s, g).await;
            spend(&s, r, Utc::now(), None).await;
            let guard = BudgetGuard::new(s);
            let err = guard.remaining(&limit(g, 100.0, 0.0)).await.unwrap_err();
            assert!(err.contains("unknown cost"), "{err}");
        }

        #[tokio::test]
        async fn daily_cap_ignores_earlier_days() {
            let s = storage().await;
            let g = goal(&s).await;
            let r = run(&s, g).await;
            spend(&s, r, day_start(Utc::now()) - chrono::Duration::hours(1), Some(5.0)).await;
            let guard = BudgetGuard::new(s.clone());
            let limits = limit(g, 0.0, 1.0);

            drop(guard.reserve(&limits, 0.9).await.unwrap());
            spend(&s, r, Utc::now(), Some(0.5)).await;
            let err = guard.reserve(&limits, 0.6).await.err().unwrap();
            assert!(err.starts_with("daily budget `g` exhausted"), "{err}");
            assert!(guard.reserve(&limits, 0.5).await.is_ok());
        }

        fn external(goal_id: Uuid, amount: f64, currency: &str, at: DateTime<Utc>) -> boarddo_shared::v2::ExternalSpendEntry {
            boarddo_shared::v2::NewExternalSpend {
                amount,
                currency: currency.into(),
                category: "ads".into(),
                description: String::new(),
                occurred_at: Some(at),
            }
            .into_entry(goal_id, Utc::now())
            .unwrap()
        }

        #[tokio::test]
        async fn external_usd_spend_counts_against_caps() {
            let s = storage().await;
            let g = goal(&s).await;
            let r = run(&s, g).await;
            spend(&s, r, Utc::now(), Some(0.2)).await;
            let old = day_start(Utc::now()) - chrono::Duration::hours(1);
            s.insert_external_spend(&external(g, 0.5, "usd", old)).await.unwrap();
            s.insert_external_spend(&external(goal(&s).await, 90.0, "USD", Utc::now())).await.unwrap();
            let guard = BudgetGuard::new(s.clone());

            let err = guard.reserve(&limit(g, 1.0, 0.0), 0.35).await.err().unwrap();
            assert!(err.starts_with("budget `g` exhausted"), "{err}");
            assert!(guard.reserve(&limit(g, 1.0, 0.0), 0.3).await.is_ok());
            // Yesterday's external spend is outside the daily window.
            assert!(guard.reserve(&limit(g, 0.0, 0.3), 0.05).await.is_ok());
            s.insert_external_spend(&external(g, 0.1, "USD", Utc::now())).await.unwrap();
            let err = guard.reserve(&limit(g, 0.0, 0.3), 0.01).await.err().unwrap();
            assert!(err.starts_with("daily budget `g` exhausted"), "{err}");
        }

        #[tokio::test]
        async fn non_usd_external_spend_blocks_capped_goal() {
            let s = storage().await;
            let g = goal(&s).await;
            s.insert_external_spend(&external(g, 1000.0, "RUB", Utc::now())).await.unwrap();
            let guard = BudgetGuard::new(s);
            let err = guard.reserve(&limit(g, 100.0, 0.0), 0.0).await.err().unwrap();
            assert!(err.contains("RUB is not converted to USD"), "{err}");
        }

        #[tokio::test]
        async fn unknown_cost_blocks_further_spend() {
            let s = storage().await;
            let g = goal(&s).await;
            let r = run(&s, g).await;
            spend(&s, r, Utc::now(), None).await;
            let guard = BudgetGuard::new(s);
            let err = guard.reserve(&limit(g, 100.0, 0.0), 0.0).await.err().unwrap();
            assert!(err.contains("unknown cost"), "{err}");
        }

        #[tokio::test]
        async fn unreadable_spend_refuses() {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            let guard = BudgetGuard::new(Storage::from_pool(pool));
            let err = guard.reserve(&limit(Uuid::now_v7(), 1.0, 0.0), 0.01).await.err().unwrap();
            assert!(err.contains("could not be read"), "{err}");
        }

        #[tokio::test]
        async fn invalid_estimate_is_refused() {
            let s = storage().await;
            let g = goal(&s).await;
            let guard = BudgetGuard::new(s);
            let limits = limit(g, 1.0, 0.0);
            for bad in [f64::NAN, f64::INFINITY, -0.01] {
                let err = guard.reserve(&limits, bad).await.err().unwrap();
                assert!(err.contains("invalid cost estimate"), "{err}");
            }
        }

        #[tokio::test]
        async fn concurrent_reservations_share_the_budget() {
            let s = storage().await;
            let g = goal(&s).await;
            let guard = Arc::new(BudgetGuard::new(s));
            // Same goal listed twice (nested scopes) is held once.
            let mut limits = limit(g, 1.0, 1.0);
            limits.extend(limit(g, 1.0, 1.0));

            let tasks: Vec<_> = (0..8)
                .map(|_| {
                    let (guard, limits) = (guard.clone(), limits.clone());
                    tokio::spawn(async move { guard.reserve(&limits, 0.3).await })
                })
                .collect();
            let mut held = Vec::new();
            for t in tasks {
                if let Ok(r) = t.await.unwrap() {
                    held.push(r);
                }
            }
            assert_eq!(held.len(), 3, "only 3 × 0.3 fit under 1.0");
            assert!(guard.headroom(&limits).await.is_ok());
            assert!(guard.reserve(&limits, 0.3).await.is_err());
            held.pop();
            let again = guard.reserve(&limits, 0.3).await;
            assert!(again.is_ok(), "released reservation frees its share");
            drop(again);
            drop(held);
            assert_eq!(guard.reserved_for(BudgetKey::Goal(g)), 0.0);
        }

        #[tokio::test]
        async fn limits_follow_spawned_tasks() {
            assert!(budget_limits().is_empty());
            assert!(matches!(reserve(1.0).await, Ok(None)), "no limits ⇒ no reservation");
            assert_eq!(headroom().await, Ok(()));

            let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
            let seen = budgeted(limit(a, 1.0, 0.0), async move {
                budgeted(limit(b, 2.0, 0.0), async {
                    let c = capture();
                    tokio::spawn(enter(c, async { budget_limits() })).await.unwrap()
                })
                .await
            })
            .await;
            let keys: Vec<_> = seen.iter().map(|l| l.key).collect();
            assert_eq!(keys, vec![BudgetKey::Goal(a), BudgetKey::Goal(b)]);
            assert!(budget_limits().is_empty());
        }

        #[tokio::test]
        async fn recorded_spend_counts_immediately() {
            let s = storage().await;
            let g = goal(&s).await;
            let r = run(&s, g).await;
            let guard = BudgetGuard::new(s);
            let limits = limit(g, 1.0, 0.0);
            let res = guard.reserve(&limits, 0.6).await.unwrap();
            let mut e = chat_entry("openai", "gpt-4o", None, Duration::ZERO, None);
            e.cost_usd = Some(0.6);
            e.scope.goal_run_id = Some(r);
            guard.record_now(e).await;
            drop(res);
            assert!(guard.reserve(&limits, 0.5).await.is_err());
            assert!(guard.reserve(&limits, 0.4).await.is_ok());
        }
    }
}
