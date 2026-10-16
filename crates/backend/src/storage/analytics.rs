//! Goal analytics facts: counted from the records of what actually happened
//! (audit trail, call journal, executions, ledger, leads, deals), never from
//! the agent's own progress counter.

use std::collections::BTreeMap;

use boarddo_shared::v2::{
    DealFacts, ExecutionFacts, ExperimentFacts, FailureReason, GoalAnalyticsFacts, JournalFacts,
    LeadFacts, SpendFacts, ToolCallFacts,
};
use chrono::Utc;
use uuid::Uuid;

use super::Storage;

/// Longest failure text kept for grouping; longer messages are cut so that
/// the same error with different payload tails still groups together.
const MAX_REASON_CHARS: usize = 200;
/// Distinct failure texts loaded per source.
const MAX_REASONS_PER_SOURCE: i64 = 20;

const RUNS_OF_GOAL: &str = "SELECT id FROM goal_runs WHERE goal_id = ?";

fn n(v: i64) -> u64 {
    v.max(0) as u64
}

fn reasons(source: &str, rows: Vec<(String, i64)>) -> Vec<FailureReason> {
    rows.into_iter()
        .map(|(reason, count)| FailureReason {
            source: source.to_string(),
            reason,
            count: n(count),
        })
        .collect()
}

impl Storage {
    async fn count_by_key(&self, sql: &str, goal_id: &str) -> anyhow::Result<BTreeMap<String, u64>> {
        let rows: Vec<(String, i64)> = sqlx::query_as(sql)
            .bind(goal_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.into_iter().map(|(k, v)| (k, n(v))).collect())
    }

    async fn failure_texts(&self, sql: &str, goal_id: &str) -> anyhow::Result<Vec<(String, i64)>> {
        Ok(sqlx::query_as(sql)
            .bind(goal_id)
            .bind(MAX_REASON_CHARS as i64)
            .bind(MAX_REASONS_PER_SOURCE)
            .fetch_all(self.pool())
            .await?)
    }

    /// Whether the goal metric reached its target on verified data. The
    /// agent's own counter never completes a goal; a metric with no verified
    /// source never completes automatically.
    pub async fn goal_target_verified(&self, goal_id: Uuid) -> anyhow::Result<bool> {
        Ok(self.goal_analytics_facts(goal_id).await?.is_some_and(|f| {
            boarddo_shared::v2::compute_goal_report(&f).outcome.status
                == boarddo_shared::v2::OutcomeStatus::Reached
        }))
    }

    /// Everything the analytics report is computed from. `None` when the goal
    /// does not exist.
    pub async fn goal_analytics_facts(
        &self,
        goal_id: Uuid,
    ) -> anyhow::Result<Option<GoalAnalyticsFacts>> {
        let Some(spec) = self.get_goal_spec(goal_id).await? else {
            return Ok(None);
        };
        let runs = self.list_goal_runs_for_goal(goal_id).await?;
        let gid = goal_id.to_string();

        // ---- tool calls (audit trail) ----
        let audit = self
            .count_by_key(
                &format!(
                    "SELECT kind, COUNT(*) FROM agent_audit
                     WHERE run_id IN ({RUNS_OF_GOAL}) AND kind IN ('act', 'act.failed', 'act.denied')
                     GROUP BY kind"
                ),
                &gid,
            )
            .await?;
        let get = |m: &BTreeMap<String, u64>, k: &str| m.get(k).copied().unwrap_or(0);
        let tool_calls = ToolCallFacts {
            succeeded: get(&audit, "act"),
            failed: get(&audit, "act.failed"),
            denied: get(&audit, "act.denied"),
        };

        // ---- single-execution journal ----
        let journal_rows = self
            .count_by_key(
                &format!(
                    "SELECT status, COUNT(*) FROM tool_call_journal
                     WHERE run_id IN ({RUNS_OF_GOAL}) GROUP BY status"
                ),
                &gid,
            )
            .await?;
        let journal = JournalFacts {
            succeeded: get(&journal_rows, "succeeded"),
            failed: get(&journal_rows, "failed"),
            unresolved: get(&journal_rows, "started"),
        };

        // ---- workflow executions of linked workflows ----
        let linked = format!(
            "SELECT DISTINCT workflow_id FROM goal_run_workflows WHERE run_id IN ({RUNS_OF_GOAL})"
        );
        let exec_rows = self
            .count_by_key(
                &format!(
                    "SELECT CASE WHEN status = 'completed' AND error IS NOT NULL
                                 THEN 'degraded' ELSE status END AS k, COUNT(*)
                     FROM executions
                     WHERE workflow_id IN ({linked}) GROUP BY k"
                ),
                &gid,
            )
            .await?;
        let (workflows_linked,): (i64,) =
            sqlx::query_as(&format!("SELECT COUNT(*) FROM ({linked})"))
                .bind(&gid)
                .fetch_one(self.pool())
                .await?;
        let (workflows_with_completed,): (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(DISTINCT workflow_id) FROM executions
             WHERE status = 'completed' AND error IS NULL AND workflow_id IN ({linked})"
        ))
        .bind(&gid)
        .fetch_one(self.pool())
        .await?;
        let executions = ExecutionFacts {
            completed: get(&exec_rows, "completed"),
            degraded: get(&exec_rows, "degraded"),
            failed: get(&exec_rows, "failed"),
            cancelled: get(&exec_rows, "cancelled"),
            unfinished: get(&exec_rows, "pending") + get(&exec_rows, "running"),
            workflows_with_completed: n(workflows_with_completed),
            workflows_linked: n(workflows_linked),
        };

        // ---- model spend (execution ledger) ----
        let (calls, failed_calls, priced_usd, unpriced_calls): (i64, i64, f64, i64) =
            sqlx::query_as(&format!(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN json_extract(data, '$.ok') = 0 THEN 1 ELSE 0 END), 0),
                        CAST(COALESCE(SUM(json_extract(data, '$.cost_usd')), 0) AS REAL),
                        COALESCE(SUM(CASE WHEN json_extract(data, '$.cost_usd') IS NULL THEN 1 ELSE 0 END), 0)
                 FROM execution_ledger WHERE goal_run_id IN ({RUNS_OF_GOAL})"
            ))
            .bind(&gid)
            .fetch_one(self.pool())
            .await?;
        let spend = SpendFacts {
            calls: n(calls),
            failed_calls: n(failed_calls),
            priced_usd,
            unpriced_calls: n(unpriced_calls),
        };

        let external = self.external_spend_facts(goal_id).await?;

        // ---- leads ----
        let real = self
            .count_by_key(
                "SELECT stage, COUNT(*) FROM marketing_leads
                 WHERE goal_id = ? AND simulated = 0 GROUP BY stage",
                &gid,
            )
            .await?;
        let (simulated,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM marketing_leads WHERE goal_id = ? AND simulated <> 0",
        )
        .bind(&gid)
        .fetch_one(self.pool())
        .await?;
        let leads = LeadFacts {
            cold: get(&real, "cold"),
            interested: get(&real, "interested"),
            qualified: get(&real, "qualified"),
            converted: get(&real, "converted"),
            lost: get(&real, "lost"),
            simulated: n(simulated),
        };

        // ---- deals from the goal's real leads ----
        let real_leads = "SELECT id FROM marketing_leads WHERE goal_id = ? AND simulated = 0";
        let deal_rows: Vec<(String, Option<String>, Option<f64>)> = sqlx::query_as(&format!(
            "SELECT stage, json_extract(data, '$.currency'), CAST(json_extract(data, '$.amount') AS REAL)
             FROM deals WHERE lead_id IN ({real_leads})"
        ))
        .bind(&gid)
        .fetch_all(self.pool())
        .await?;
        let mut deals = DealFacts::default();
        for (stage, currency, amount) in deal_rows {
            match stage.as_str() {
                "won" => {
                    deals.won += 1;
                    let currency = currency
                        .filter(|c| !c.trim().is_empty())
                        .unwrap_or_else(|| "unknown".into());
                    *deals.revenue_by_currency.entry(currency).or_insert(0.0) +=
                        amount.unwrap_or(0.0);
                }
                "lost" => deals.lost += 1,
                _ => deals.open += 1,
            }
        }

        // ---- experiments ----
        let exp = self
            .count_by_key(
                &format!(
                    "SELECT decision, COUNT(*) FROM experiments
                     WHERE run_id IN ({RUNS_OF_GOAL}) GROUP BY decision"
                ),
                &gid,
            )
            .await?;
        let experiments = ExperimentFacts {
            total: exp.values().sum(),
            stopped: get(&exp, "stop"),
            pivoted: get(&exp, "pivot"),
        };

        // ---- failure reasons ----
        let mut failures = Vec::new();
        failures.extend(reasons(
            "tool",
            self.failure_texts(
                "SELECT substr(COALESCE(message, ''), 1, ?2) AS r, COUNT(*) FROM agent_audit
                 WHERE run_id IN (SELECT id FROM goal_runs WHERE goal_id = ?1) AND kind = 'act.failed'
                 GROUP BY r ORDER BY COUNT(*) DESC LIMIT ?3",
                &gid,
            )
            .await?,
        ));
        failures.extend(reasons(
            "policy",
            self.failure_texts(
                "SELECT substr(COALESCE(message, ''), 1, ?2) AS r, COUNT(*) FROM agent_audit
                 WHERE run_id IN (SELECT id FROM goal_runs WHERE goal_id = ?1) AND kind = 'act.denied'
                 GROUP BY r ORDER BY COUNT(*) DESC LIMIT ?3",
                &gid,
            )
            .await?,
        ));
        failures.extend(reasons(
            "budget",
            self.failure_texts(
                "SELECT substr(COALESCE(message, kind), 1, ?2) AS r, COUNT(*) FROM agent_audit
                 WHERE run_id IN (SELECT id FROM goal_runs WHERE goal_id = ?1)
                   AND kind IN ('budget.exhausted', 'budget.daily_exhausted')
                 GROUP BY r ORDER BY COUNT(*) DESC LIMIT ?3",
                &gid,
            )
            .await?,
        ));
        failures.extend(reasons(
            "execution",
            self.failure_texts(
                "SELECT substr(COALESCE(error, 'failed without error text'), 1, ?2) AS r, COUNT(*)
                 FROM executions
                 WHERE (status = 'failed' OR (status = 'completed' AND error IS NOT NULL))
                   AND workflow_id IN (
                     SELECT DISTINCT workflow_id FROM goal_run_workflows
                     WHERE run_id IN (SELECT id FROM goal_runs WHERE goal_id = ?1))
                 GROUP BY r ORDER BY COUNT(*) DESC LIMIT ?3",
                &gid,
            )
            .await?,
        ));
        failures.extend(reasons(
            "model",
            self.failure_texts(
                "SELECT substr(COALESCE(json_extract(data, '$.error'), 'failed without error text'), 1, ?2) AS r,
                        COUNT(*)
                 FROM execution_ledger
                 WHERE goal_run_id IN (SELECT id FROM goal_runs WHERE goal_id = ?1)
                   AND json_extract(data, '$.ok') = 0
                 GROUP BY r ORDER BY COUNT(*) DESC LIMIT ?3",
                &gid,
            )
            .await?,
        ));
        failures.extend(reasons(
            "run",
            self.failure_texts(
                "SELECT substr(error, 1, ?2) AS r, COUNT(*) FROM goal_runs
                 WHERE goal_id = ?1 AND error IS NOT NULL AND error <> ''
                 GROUP BY r ORDER BY COUNT(*) DESC LIMIT ?3",
                &gid,
            )
            .await?,
        ));
        failures.extend(reasons(
            "deal",
            self.failure_texts(
                "SELECT substr(COALESCE(json_extract(data, '$.lost_reason'), 'lost without reason'), 1, ?2) AS r,
                        COUNT(*)
                 FROM deals
                 WHERE stage = 'lost' AND lead_id IN (
                     SELECT id FROM marketing_leads WHERE goal_id = ?1 AND simulated = 0)
                 GROUP BY r ORDER BY COUNT(*) DESC LIMIT ?3",
                &gid,
            )
            .await?,
        ));

        Ok(Some(GoalAnalyticsFacts {
            spec,
            runs,
            tool_calls,
            journal,
            executions,
            spend,
            external,
            leads,
            deals,
            experiments,
            failures,
            now: Utc::now(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::{
        AgentAuditEntry, CapabilityBudget, CreateGoalRequest, Deal, DealStage, GoalRun,
        GoalRunStatus, GoalSpec, Lead, LeadStage, LedgerEntry, LedgerScope, OutcomeStatus,
        PolicyBundle, compute_goal_report,
    };
    use boarddo_shared::{CreateWorkflowRequest, ExecutionStatus};
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn storage() -> Storage {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let s = Storage::from_pool(pool);
        s.migrate().await.unwrap();
        s
    }

    async fn goal(s: &Storage, metric: &str, target: f64) -> (GoalSpec, GoalRun) {
        let spec = crate::agent_runtime::spec_from_request(CreateGoalRequest {
            title: "Growth".into(),
            text: "10 leads".into(),
            metric: metric.into(),
            target,
            deadline: None,
            constraints: json!({}),
            budget: CapabilityBudget::default(),
            policy: PolicyBundle::default(),
            agent_type_id: "agent.growth".into(),
            instructions: String::new(),
            tools: vec![],
            tick_interval_secs: Some(30),
            openai_connection_id: None,
            template_id: None,
        });
        s.insert_goal_spec(&spec).await.unwrap();
        let run = GoalRun {
            id: Uuid::now_v7(),
            goal_id: spec.id,
            status: GoalRunStatus::Running,
            current: 0.0,
            target: spec.target,
            strategy: Default::default(),
            actions_today: 0,
            actions_day: "2026-10-16".into(),
            tick_count: 0,
            last_tick_at: None,
            next_tick_at: Some(Utc::now()),
            last_observe: None,
            last_think: None,
            error: None,
            started_at: Utc::now(),
            finished_at: None,
            replan: Default::default(),
        };
        s.insert_goal_run(&run).await.unwrap();
        (spec, run)
    }

    async fn audit(s: &Storage, run_id: Uuid, kind: &str, message: Option<&str>) {
        s.insert_audit(&AgentAuditEntry {
            id: Uuid::now_v7(),
            run_id,
            at: Utc::now(),
            kind: kind.into(),
            message: message.map(Into::into),
            payload: None,
        })
        .await
        .unwrap();
    }

    async fn lead(s: &Storage, spec: &GoalSpec, run: &GoalRun, stage: LeadStage, simulated: bool) -> Uuid {
        let id = Uuid::now_v7();
        s.insert_lead(&Lead {
            id,
            goal_id: spec.id,
            run_id: run.id,
            campaign_id: None,
            experiment_id: None,
            source: "telegram".into(),
            source_reference: None,
            chat_id: None,
            account_id: None,
            stage,
            score: 0,
            score_reasons: vec![],
            first_seen_at: Utc::now(),
            last_activity_at: Utc::now(),
            metadata: Default::default(),
            simulated,
        })
        .await
        .unwrap();
        id
    }

    async fn deal(s: &Storage, lead_id: Uuid, stage: DealStage, amount: f64, lost_reason: Option<&str>) {
        s.save_deal(&Deal {
            id: Uuid::now_v7(),
            title: "deal".into(),
            product_id: None,
            lead_id: Some(lead_id),
            contact: String::new(),
            source: "telegram".into(),
            stage,
            quantity: 1,
            amount,
            currency: "RUB".into(),
            notes: String::new(),
            lost_reason: lost_reason.map(Into::into),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
        })
        .await
        .unwrap();
    }

    async fn ledger(s: &Storage, run_id: Uuid, cost: Option<f64>, ok: bool) {
        s.insert_ledger_entry(&LedgerEntry {
            id: Uuid::now_v7(),
            at: Utc::now(),
            provider: "openai".into(),
            model: "m".into(),
            kind: "chat".into(),
            input_tokens: 10,
            output_tokens: 10,
            cost_usd: cost,
            duration_ms: 5,
            ok,
            error: (!ok).then(|| "HTTP 500".to_string()),
            scope: LedgerScope {
                goal_run_id: Some(run_id),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn unknown_goal_has_no_facts() {
        let s = storage().await;
        assert!(s.goal_analytics_facts(Uuid::now_v7()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn activity_and_agent_counter_do_not_count_as_result() {
        let s = storage().await;
        let (spec, mut run) = goal(&s, "leads_interested", 2.0).await;
        for _ in 0..5 {
            audit(&s, run.id, "act", None).await;
        }
        audit(&s, run.id, "act.failed", Some("HTTP 403 forbidden")).await;
        audit(&s, run.id, "act.failed", Some("HTTP 403 forbidden")).await;
        audit(&s, run.id, "act.denied", Some("approval required")).await;
        run.current = 2.0;
        s.update_goal_run(&run).await.unwrap();
        // Simulated leads never count toward the outcome.
        lead(&s, &spec, &run, LeadStage::Qualified, true).await;
        lead(&s, &spec, &run, LeadStage::Interested, true).await;

        let facts = s.goal_analytics_facts(spec.id).await.unwrap().unwrap();
        assert_eq!(facts.tool_calls.succeeded, 5);
        assert_eq!(facts.tool_calls.failed, 2);
        assert_eq!(facts.tool_calls.denied, 1);
        assert_eq!(facts.leads.simulated, 2);
        let tool = facts.failures.iter().find(|f| f.source == "tool").unwrap();
        assert_eq!((tool.reason.as_str(), tool.count), ("HTTP 403 forbidden", 2));
        assert!(facts.failures.iter().any(|f| f.source == "policy" && f.reason == "approval required"));

        let report = compute_goal_report(&facts);
        assert_eq!(report.outcome.achieved, Some(0.0));
        assert_eq!(report.outcome.reported_by_agent, Some(2.0));
        assert_eq!(report.outcome.leads_simulated_excluded, 2);
        assert_ne!(report.outcome.status, OutcomeStatus::Reached);
        let codes: Vec<&str> = report.recommendations.iter().map(|n| n.code.as_str()).collect();
        assert!(codes.contains(&"agent_counter_mismatch"), "{codes:?}");
        assert!(codes.contains(&"activity_without_result"), "{codes:?}");
    }

    #[tokio::test]
    async fn real_leads_deals_spend_and_executions_are_counted() {
        let s = storage().await;
        let (spec, run) = goal(&s, "leads_qualified", 2.0).await;
        let l1 = lead(&s, &spec, &run, LeadStage::Qualified, false).await;
        let l2 = lead(&s, &spec, &run, LeadStage::Converted, false).await;
        let l3 = lead(&s, &spec, &run, LeadStage::Interested, false).await;
        lead(&s, &spec, &run, LeadStage::Cold, false).await;
        let fake = lead(&s, &spec, &run, LeadStage::Converted, true).await;
        deal(&s, l2, DealStage::Won, 1000.0, None).await;
        deal(&s, l1, DealStage::Proposal, 500.0, None).await;
        deal(&s, l3, DealStage::Lost, 300.0, Some("too expensive")).await;
        deal(&s, fake, DealStage::Won, 9999.0, None).await;

        ledger(&s, run.id, Some(0.25), true).await;
        ledger(&s, run.id, None, true).await;
        ledger(&s, run.id, Some(0.5), false).await;
        // Spend of another goal is not attributed here.
        let (_, other) = goal(&s, "leads", 1.0).await;
        ledger(&s, other.id, Some(7.0), true).await;

        let wf = s
            .create_workflow(CreateWorkflowRequest {
                name: "wf".into(),
                description: String::new(),
                nodes: vec![],
                edges: vec![],
            })
            .await
            .unwrap();
        s.link_goal_run_workflow(run.id, wf.id).await.unwrap();
        let (ok, bad, partial) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        s.create_execution(ok, wf.id, wf.version, "manual", &json!({})).await.unwrap();
        s.create_execution(bad, wf.id, wf.version, "manual", &json!({})).await.unwrap();
        s.create_execution(partial, wf.id, wf.version, "manual", &json!({})).await.unwrap();
        s.finish_execution(ok, ExecutionStatus::Completed, None, &[]).await.unwrap();
        s.finish_execution(
            partial,
            ExecutionStatus::Completed,
            Some("node `a`: degraded: window closed".into()),
            &[],
        )
        .await
        .unwrap();
        s.finish_execution(bad, ExecutionStatus::Failed, Some("node x failed".into()), &[])
            .await
            .unwrap();

        let facts = s.goal_analytics_facts(spec.id).await.unwrap().unwrap();
        assert_eq!(facts.leads.qualified, 1);
        assert_eq!(facts.leads.converted, 1);
        assert_eq!(facts.leads.simulated, 1);
        assert_eq!((facts.deals.won, facts.deals.open, facts.deals.lost), (1, 1, 1));
        assert_eq!(facts.deals.revenue_by_currency.get("RUB"), Some(&1000.0));
        assert_eq!(facts.spend.calls, 3);
        assert_eq!(facts.spend.failed_calls, 1);
        assert_eq!(facts.spend.unpriced_calls, 1);
        assert!((facts.spend.priced_usd - 0.75).abs() < 1e-9);
        assert_eq!(facts.executions.completed, 1);
        assert_eq!(facts.executions.degraded, 1);
        assert_eq!(facts.executions.failed, 1);
        assert_eq!(facts.executions.workflows_linked, 1);
        assert!(facts.failures.iter().any(|f| f.source == "deal" && f.reason == "too expensive"));
        assert!(facts.failures.iter().any(|f| f.source == "execution" && f.reason == "node x failed"));
        assert!(facts.failures.iter().any(|f| f.source == "execution" && f.reason.contains("degraded")));
        assert!(facts.failures.iter().any(|f| f.source == "model" && f.reason == "HTTP 500"));

        let report = compute_goal_report(&facts);
        // qualified + converted are "at least qualified".
        assert_eq!(report.outcome.achieved, Some(2.0));
        assert_eq!(report.outcome.status, OutcomeStatus::Reached);
        assert!((report.operational.cost.actual_usd - 0.75).abs() < 1e-9);
        assert!(report.operational.cost.actual_is_lower_bound);
        assert_eq!(report.operational.cost.planned_estimate_usd, None);
    }

    #[tokio::test]
    async fn external_spend_is_stored_per_goal_and_reaches_the_report() {
        use boarddo_shared::v2::NewExternalSpend;
        let s = storage().await;
        let (spec, run) = goal(&s, "leads_interested", 2.0).await;
        let (other, _) = goal(&s, "leads_interested", 2.0).await;
        ledger(&s, run.id, Some(0.5), true).await;
        let new = |amount: f64, currency: &str| NewExternalSpend {
            amount,
            currency: currency.into(),
            category: "Ads".into(),
            description: "campaign".into(),
            occurred_at: None,
        };
        for (g, amount, cur) in [(spec.id, 10.0, "usd"), (spec.id, 2.5, "USD"), (spec.id, 300.0, "RUB"), (other.id, 99.0, "USD")] {
            s.insert_external_spend(&new(amount, cur).into_entry(g, Utc::now()).unwrap()).await.unwrap();
        }
        assert!(new(-1.0, "USD").into_entry(spec.id, Utc::now()).is_err());
        assert!(new(1.0, "dollars").into_entry(spec.id, Utc::now()).is_err());

        let list = s.list_external_spend(spec.id).await.unwrap();
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|e| e.goal_id == spec.id && e.category == "ads"));

        let facts = s.goal_analytics_facts(spec.id).await.unwrap().unwrap();
        assert_eq!(facts.external.entries, 3);
        assert_eq!(facts.external.by_currency.get("USD"), Some(&12.5));
        assert_eq!(facts.external.by_currency.get("RUB"), Some(&300.0));
        let report = boarddo_shared::v2::compute_goal_report(&facts);
        assert_eq!(report.operational.cost.external_usd, 12.5);
        assert!((report.operational.cost.actual_usd - 13.0).abs() < 1e-9);

        let spend = s.goal_spend(spec.id, Utc::now() - chrono::Duration::hours(1)).await.unwrap();
        assert!((spend.total_usd - 13.0).abs() < 1e-9);
        assert_eq!(spend.unconverted_currencies, vec!["RUB".to_string()]);
    }

    #[tokio::test]
    async fn target_is_verified_only_by_real_leads() {
        let s = storage().await;
        let (spec, mut run) = goal(&s, "leads_interested", 2.0).await;
        run.current = 5.0;
        s.update_goal_run(&run).await.unwrap();
        lead(&s, &spec, &run, LeadStage::Interested, true).await;
        lead(&s, &spec, &run, LeadStage::Qualified, true).await;
        assert!(!s.goal_target_verified(spec.id).await.unwrap(), "agent counter and simulated leads");
        lead(&s, &spec, &run, LeadStage::Interested, false).await;
        assert!(!s.goal_target_verified(spec.id).await.unwrap());
        lead(&s, &spec, &run, LeadStage::Qualified, false).await;
        assert!(s.goal_target_verified(spec.id).await.unwrap());

        let (spec, mut run) = goal(&s, "posts_published", 1.0).await;
        run.current = 10.0;
        s.update_goal_run(&run).await.unwrap();
        assert!(!s.goal_target_verified(spec.id).await.unwrap(), "unverifiable metric");
    }
}
