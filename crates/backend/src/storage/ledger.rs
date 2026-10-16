//! Execution ledger persistence — one row per model call.
//! Rows are JSON documents plus indexed columns for filtering.

use boarddo_shared::v2::{ExternalSpendEntry, ExternalSpendFacts, LedgerEntry, LedgerQuery};
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use super::Storage;

#[derive(FromRow)]
struct DataRow {
    data: String,
}

/// Model spend of every run of one goal plus its recorded external USD spend.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GoalSpend {
    pub total_usd: f64,
    /// Spend at or after the `since` passed to [`Storage::goal_spend`].
    pub since_usd: f64,
    /// Calls whose cost is unknown and therefore missing from the sums.
    pub unpriced_calls: i64,
    /// Currencies of external spend that can not be added to USD sums.
    pub unconverted_currencies: Vec<String>,
}

#[derive(FromRow)]
struct ModelSpendRow {
    total_usd: f64,
    since_usd: f64,
    unpriced_calls: i64,
}

impl Storage {
    pub async fn migrate_ledger(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS execution_ledger (
                id TEXT PRIMARY KEY NOT NULL,
                at TEXT NOT NULL,
                model TEXT NOT NULL,
                objective_id TEXT,
                goal_run_id TEXT,
                workflow_id TEXT,
                execution_id TEXT,
                data TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_ledger_at ON execution_ledger(at);
            CREATE INDEX IF NOT EXISTS idx_ledger_objective ON execution_ledger(objective_id, at);
            CREATE INDEX IF NOT EXISTS idx_ledger_goal_run ON execution_ledger(goal_run_id, at);
            CREATE INDEX IF NOT EXISTS idx_ledger_workflow ON execution_ledger(workflow_id, at);
            CREATE INDEX IF NOT EXISTS idx_ledger_execution ON execution_ledger(execution_id);
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn insert_ledger_entry(&self, e: &LedgerEntry) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO execution_ledger
                (id, at, model, objective_id, goal_run_id, workflow_id, execution_id, data)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(e.id.to_string())
        .bind(e.at.to_rfc3339())
        .bind(&e.model)
        .bind(e.scope.objective_id.map(|v| v.to_string()))
        .bind(e.scope.goal_run_id.map(|v| v.to_string()))
        .bind(e.scope.workflow_id.map(|v| v.to_string()))
        .bind(e.scope.execution_id.map(|v| v.to_string()))
        .bind(serde_json::to_string(e)?)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn goal_spend(
        &self,
        goal_id: Uuid,
        since: DateTime<Utc>,
    ) -> anyhow::Result<GoalSpend> {
        let row: ModelSpendRow = sqlx::query_as(
            "SELECT
                CAST(COALESCE(SUM(json_extract(data, '$.cost_usd')), 0) AS REAL) AS total_usd,
                CAST(COALESCE(SUM(CASE WHEN at >= ? THEN json_extract(data, '$.cost_usd') END), 0) AS REAL) AS since_usd,
                COUNT(CASE WHEN json_extract(data, '$.cost_usd') IS NULL THEN 1 END) AS unpriced_calls
             FROM execution_ledger
             WHERE goal_run_id IN (SELECT id FROM goal_runs WHERE goal_id = ?)",
        )
        .bind(since.to_rfc3339())
        .bind(goal_id.to_string())
        .fetch_one(self.pool())
        .await?;
        let external: Vec<(String, f64, f64)> = sqlx::query_as(
            "SELECT currency,
                    CAST(SUM(amount) AS REAL),
                    CAST(COALESCE(SUM(CASE WHEN occurred_at >= ? THEN amount END), 0) AS REAL)
             FROM goal_external_spend WHERE goal_id = ? GROUP BY currency",
        )
        .bind(since.to_rfc3339())
        .bind(goal_id.to_string())
        .fetch_all(self.pool())
        .await?;
        let mut out = GoalSpend {
            total_usd: row.total_usd,
            since_usd: row.since_usd,
            unpriced_calls: row.unpriced_calls,
            unconverted_currencies: Vec::new(),
        };
        for (currency, total, since_amount) in external {
            if currency == "USD" {
                out.total_usd += total;
                out.since_usd += since_amount;
            } else {
                out.unconverted_currencies.push(currency);
            }
        }
        Ok(out)
    }

    pub async fn migrate_external_spend(&self) -> anyhow::Result<()> {
        sqlx::raw_sql(
            r#"
            CREATE TABLE IF NOT EXISTS goal_external_spend (
                id TEXT PRIMARY KEY,
                goal_id TEXT NOT NULL,
                amount REAL NOT NULL CHECK (amount > 0),
                currency TEXT NOT NULL,
                category TEXT NOT NULL,
                description TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_external_spend_goal ON goal_external_spend(goal_id, occurred_at);
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn insert_external_spend(&self, e: &ExternalSpendEntry) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO goal_external_spend
                (id, goal_id, amount, currency, category, description, occurred_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(e.id.to_string())
        .bind(e.goal_id.to_string())
        .bind(e.amount)
        .bind(&e.currency)
        .bind(&e.category)
        .bind(&e.description)
        .bind(e.occurred_at.to_rfc3339())
        .bind(e.created_at.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Newest first.
    pub async fn list_external_spend(&self, goal_id: Uuid) -> anyhow::Result<Vec<ExternalSpendEntry>> {
        let rows: Vec<(String, String, f64, String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, goal_id, amount, currency, category, description, occurred_at, created_at
             FROM goal_external_spend WHERE goal_id = ? ORDER BY occurred_at DESC, id DESC",
        )
        .bind(goal_id.to_string())
        .fetch_all(self.pool())
        .await?;
        let ts = |v: &str| -> anyhow::Result<DateTime<Utc>> {
            Ok(DateTime::parse_from_rfc3339(v)?.with_timezone(&Utc))
        };
        rows.into_iter()
            .map(|(id, goal, amount, currency, category, description, occurred, created)| {
                Ok(ExternalSpendEntry {
                    id: id.parse()?,
                    goal_id: goal.parse()?,
                    amount,
                    currency,
                    category,
                    description,
                    occurred_at: ts(&occurred)?,
                    created_at: ts(&created)?,
                })
            })
            .collect()
    }

    pub async fn external_spend_facts(&self, goal_id: Uuid) -> anyhow::Result<ExternalSpendFacts> {
        let rows: Vec<(String, f64, i64)> = sqlx::query_as(
            "SELECT currency, CAST(SUM(amount) AS REAL), COUNT(*)
             FROM goal_external_spend WHERE goal_id = ? GROUP BY currency",
        )
        .bind(goal_id.to_string())
        .fetch_all(self.pool())
        .await?;
        let mut f = ExternalSpendFacts::default();
        for (currency, sum, count) in rows {
            f.entries += count.max(0) as u64;
            f.by_currency.insert(currency, sum);
        }
        Ok(f)
    }

    /// Newest first. `limit = None` returns every matching row.
    pub async fn list_ledger(
        &self,
        q: &LedgerQuery,
        limit: Option<i64>,
    ) -> anyhow::Result<Vec<LedgerEntry>> {
        let mut sql = String::from("SELECT data FROM execution_ledger WHERE 1 = 1");
        let mut binds: Vec<String> = Vec::new();
        // RFC 3339 strings from `to_rfc3339()` share one format, so they sort
        // lexicographically in time order.
        if let Some(v) = q.since {
            sql.push_str(" AND at >= ?");
            binds.push(v.to_rfc3339());
        }
        if let Some(v) = q.until {
            sql.push_str(" AND at < ?");
            binds.push(v.to_rfc3339());
        }
        for (col, val) in [
            ("objective_id", q.objective_id),
            ("goal_run_id", q.goal_run_id),
            ("workflow_id", q.workflow_id),
            ("execution_id", q.execution_id),
        ] {
            if let Some(v) = val {
                sql.push_str(&format!(" AND {col} = ?"));
                binds.push(v.to_string());
            }
        }
        if let Some(m) = q.model.as_deref().filter(|m| !m.is_empty()) {
            sql.push_str(" AND model = ?");
            binds.push(m.to_string());
        }
        sql.push_str(" ORDER BY at DESC");
        if let Some(n) = limit {
            sql.push_str(&format!(" LIMIT {}", n.max(0)));
        }
        let mut query = sqlx::query_as::<_, DataRow>(&sql);
        for b in binds {
            query = query.bind(b);
        }
        let rows = query.fetch_all(self.pool()).await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| match serde_json::from_str(&r.data) {
                Ok(v) => Some(v),
                Err(err) => {
                    tracing::warn!(error = %err, "ledger.row_corrupt");
                    None
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::LedgerScope;
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    async fn storage() -> Storage {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let storage = Storage::from_pool(pool);
        storage.migrate_ledger().await.unwrap();
        storage.migrate_external_spend().await.unwrap();
        storage
    }

    fn entry(model: &str, ago_min: i64, scope: LedgerScope) -> LedgerEntry {
        LedgerEntry {
            id: Uuid::new_v4(),
            at: Utc::now() - Duration::minutes(ago_min),
            provider: "openai".into(),
            model: model.into(),
            kind: "chat".into(),
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: Some(0.001),
            duration_ms: 50,
            ok: true,
            error: None,
            scope,
        }
    }

    #[tokio::test]
    async fn roundtrip_and_filters() {
        let s = storage().await;
        let obj = Uuid::new_v4();
        let wf = Uuid::new_v4();
        let a = entry(
            "gpt-4o",
            30,
            LedgerScope {
                objective_id: Some(obj),
                item_id: Some("i1".into()),
                ..Default::default()
            },
        );
        let b = entry(
            "claude-sonnet-4-5",
            10,
            LedgerScope {
                workflow_id: Some(wf),
                ..Default::default()
            },
        );
        let c = entry("gpt-4o", 1, LedgerScope::default());
        for e in [&a, &b, &c] {
            s.insert_ledger_entry(e).await.unwrap();
        }

        let all = s.list_ledger(&LedgerQuery::default(), None).await.unwrap();
        assert_eq!(all, vec![c.clone(), b.clone(), a.clone()]);
        assert_eq!(
            s.list_ledger(&LedgerQuery::default(), Some(1)).await.unwrap(),
            vec![c.clone()]
        );

        let by_obj = LedgerQuery {
            objective_id: Some(obj),
            ..Default::default()
        };
        assert_eq!(s.list_ledger(&by_obj, None).await.unwrap(), vec![a.clone()]);

        let by_wf = LedgerQuery {
            workflow_id: Some(wf),
            ..Default::default()
        };
        assert_eq!(s.list_ledger(&by_wf, None).await.unwrap(), vec![b.clone()]);

        let recent = LedgerQuery {
            since: Some(Utc::now() - Duration::minutes(15)),
            model: Some("gpt-4o".into()),
            ..Default::default()
        };
        assert_eq!(s.list_ledger(&recent, None).await.unwrap(), vec![c]);
    }
}
