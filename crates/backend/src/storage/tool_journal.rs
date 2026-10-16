//! Tool call journal: one row per idempotency key of a single-execution tool
//! call, so a repeated plan step, a retried tick or a double approval can not
//! send, publish or delete twice.

use chrono::Utc;
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use super::Storage;

/// Outcome of trying to take ownership of a call.
#[derive(Debug, Clone, PartialEq)]
pub enum JournalClaim {
    /// First attempt with this key: the caller must execute and then finish.
    Fresh,
    /// Previous attempt was refused upstream without effect: execute again.
    Reclaimed,
    /// Already succeeded: reuse this result, do not execute.
    Done(Value),
    /// Started and never finished (in flight, or the process died mid-call).
    InFlight,
    /// Failed with an outcome that may have taken effect.
    FailedUncertain(String),
}

/// One journal row as shown to an operator.
#[derive(Debug, Clone, serde::Serialize, FromRow)]
pub struct JournalEntry {
    pub idempotency_key: String,
    pub run_id: String,
    pub type_id: String,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub retry_safe: bool,
    pub attempts: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// Operator decision on a call whose outcome the runtime could not establish.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum JournalResolution {
    /// The action did happen: later repeats reuse `result` and do not execute.
    TookEffect { result: Value },
    /// The action certainly did not happen: the next call may execute again.
    NoEffect { note: String },
}

#[derive(FromRow)]
struct JournalRow {
    status: String,
    result: Option<String>,
    error: Option<String>,
}

impl Storage {
    pub async fn migrate_tool_journal(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS tool_call_journal (
                idempotency_key TEXT PRIMARY KEY NOT NULL,
                run_id TEXT NOT NULL,
                type_id TEXT NOT NULL,
                status TEXT NOT NULL,
                result TEXT,
                error TEXT,
                retry_safe INTEGER NOT NULL DEFAULT 0,
                attempts INTEGER NOT NULL DEFAULT 1,
                started_at TEXT NOT NULL,
                finished_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_tool_journal_run ON tool_call_journal(run_id, started_at);
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Atomically claims `key`. Only `Fresh` / `Reclaimed` allow execution.
    pub async fn tool_journal_claim(
        &self,
        key: &str,
        run_id: Uuid,
        type_id: &str,
    ) -> anyhow::Result<JournalClaim> {
        let now = Utc::now().to_rfc3339();
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO tool_call_journal
                (idempotency_key, run_id, type_id, status, started_at)
             VALUES (?, ?, ?, 'started', ?)",
        )
        .bind(key)
        .bind(run_id.to_string())
        .bind(type_id)
        .bind(&now)
        .execute(self.pool())
        .await?
        .rows_affected();
        if inserted == 1 {
            return Ok(JournalClaim::Fresh);
        }

        let reclaimed = sqlx::query(
            "UPDATE tool_call_journal
             SET status = 'started', attempts = attempts + 1, error = NULL,
                 retry_safe = 0, started_at = ?, finished_at = NULL
             WHERE idempotency_key = ? AND status = 'failed' AND retry_safe = 1",
        )
        .bind(&now)
        .bind(key)
        .execute(self.pool())
        .await?
        .rows_affected();
        if reclaimed == 1 {
            return Ok(JournalClaim::Reclaimed);
        }

        let row: JournalRow = sqlx::query_as(
            "SELECT status, result, error FROM tool_call_journal WHERE idempotency_key = ?",
        )
        .bind(key)
        .fetch_one(self.pool())
        .await?;
        Ok(match row.status.as_str() {
            "succeeded" => JournalClaim::Done(match row.result {
                Some(r) => serde_json::from_str(&r)?,
                None => Value::Null,
            }),
            "failed" => JournalClaim::FailedUncertain(row.error.unwrap_or_default()),
            _ => JournalClaim::InFlight,
        })
    }

    /// Records the outcome of a claimed call. `retry_safe` marks a failure
    /// that certainly had no effect.
    pub async fn tool_journal_finish(
        &self,
        key: &str,
        outcome: &Result<Value, String>,
        retry_safe: bool,
    ) -> anyhow::Result<()> {
        let (status, result, error) = match outcome {
            Ok(v) => ("succeeded", Some(serde_json::to_string(v)?), None),
            Err(e) => ("failed", None, Some(e.clone())),
        };
        sqlx::query(
            "UPDATE tool_call_journal
             SET status = ?, result = ?, error = ?, retry_safe = ?, finished_at = ?
             WHERE idempotency_key = ? AND status = 'started'",
        )
        .bind(status)
        .bind(result)
        .bind(error)
        .bind(retry_safe && outcome.is_err())
        .bind(Utc::now().to_rfc3339())
        .bind(key)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn tool_journal_list(&self, run_id: Option<Uuid>) -> anyhow::Result<Vec<JournalEntry>> {
        const COLS: &str = "idempotency_key, run_id, type_id, status, result, error, retry_safe, \
                            attempts, started_at, finished_at FROM tool_call_journal";
        let rows = match run_id {
            Some(id) => {
                sqlx::query_as(&format!("SELECT {COLS} WHERE run_id = ? ORDER BY started_at DESC"))
                    .bind(id.to_string())
                    .fetch_all(self.pool())
                    .await?
            }
            None => {
                sqlx::query_as(&format!(
                    "SELECT {COLS} WHERE status != 'succeeded' ORDER BY started_at DESC LIMIT 500"
                ))
                .fetch_all(self.pool())
                .await?
            }
        };
        Ok(rows)
    }

    /// Settles a call the runtime refuses to repeat (`started`, or `failed`
    /// with an uncertain outcome). Succeeded and retry-safe rows are left as
    /// they are. Returns false when nothing matched.
    pub async fn tool_journal_resolve(
        &self,
        key: &str,
        resolution: &JournalResolution,
    ) -> anyhow::Result<bool> {
        let (status, result, error, retry_safe) = match resolution {
            JournalResolution::TookEffect { result } => {
                ("succeeded", Some(serde_json::to_string(result)?), None, false)
            }
            JournalResolution::NoEffect { note } => (
                "failed",
                None,
                Some(format!("resolved by operator: no effect ({note})")),
                true,
            ),
        };
        let n = sqlx::query(
            "UPDATE tool_call_journal
             SET status = ?, result = ?, error = ?, retry_safe = ?, finished_at = ?
             WHERE idempotency_key = ?
               AND (status = 'started' OR (status = 'failed' AND retry_safe = 0))",
        )
        .bind(status)
        .bind(result)
        .bind(error)
        .bind(retry_safe)
        .bind(Utc::now().to_rfc3339())
        .bind(key)
        .execute(self.pool())
        .await?
        .rows_affected();
        Ok(n == 1)
    }

    /// Marks calls stuck in `started` since before `cutoff` (the process died
    /// mid-call) as failed with an unknown outcome. They stay non-repeatable
    /// until an operator resolves them.
    pub async fn tool_journal_expire_stale(
        &self,
        cutoff: chrono::DateTime<Utc>,
    ) -> anyhow::Result<u64> {
        Ok(sqlx::query(
            "UPDATE tool_call_journal
             SET status = 'failed', retry_safe = 0, finished_at = ?,
                 error = 'outcome unknown: call never finished (interrupted); resolve manually'
             WHERE status = 'started' AND started_at < ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(cutoff.to_rfc3339())
        .execute(self.pool())
        .await?
        .rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn storage() -> Storage {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let s = Storage::from_pool(pool);
        s.migrate_tool_journal().await.unwrap();
        s
    }

    #[tokio::test]
    async fn stale_call_becomes_uncertain_then_operator_settles_it() {
        let s = storage().await;
        let run = Uuid::now_v7();
        assert_eq!(s.tool_journal_claim("k1", run, "telegram.send_message").await.unwrap(), JournalClaim::Fresh);
        assert_eq!(s.tool_journal_claim("k2", run, "telegram.send_message").await.unwrap(), JournalClaim::Fresh);

        // Not old enough yet.
        assert_eq!(s.tool_journal_expire_stale(Utc::now() - chrono::Duration::hours(1)).await.unwrap(), 0);
        assert_eq!(s.tool_journal_expire_stale(Utc::now() + chrono::Duration::seconds(1)).await.unwrap(), 2);
        assert!(matches!(
            s.tool_journal_claim("k1", run, "telegram.send_message").await.unwrap(),
            JournalClaim::FailedUncertain(e) if e.contains("outcome unknown")
        ));

        // k1 was delivered: repeats reuse the recorded result.
        let took = JournalResolution::TookEffect { result: json!({"message_id": 5}) };
        assert!(s.tool_journal_resolve("k1", &took).await.unwrap());
        assert_eq!(
            s.tool_journal_claim("k1", run, "telegram.send_message").await.unwrap(),
            JournalClaim::Done(json!({"message_id": 5}))
        );
        // A succeeded row can not be overwritten.
        let none = JournalResolution::NoEffect { note: "checked chat".into() };
        assert!(!s.tool_journal_resolve("k1", &none).await.unwrap());

        // k2 never went out: the next call may execute.
        assert!(s.tool_journal_resolve("k2", &none).await.unwrap());
        assert_eq!(s.tool_journal_claim("k2", run, "telegram.send_message").await.unwrap(), JournalClaim::Reclaimed);

        let listed = s.tool_journal_list(Some(run)).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert!(!s.tool_journal_resolve("missing", &none).await.unwrap());
    }
}
