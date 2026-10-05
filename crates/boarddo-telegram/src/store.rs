use chrono::Utc;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use tglib::{
    TelegramAccount, TelegramAccountId, TelegramAccountPermissions, TelegramAccountStatus,
    TelegramAuditEvent, TelegramAuditStatus, TelegramScenarioExecution,
};

pub struct TelegramSessionStore {
    pool: SqlitePool,
}

impl TelegramSessionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS telegram_accounts (
                id TEXT PRIMARY KEY NOT NULL,
                status TEXT NOT NULL,
                phone_masked TEXT,
                username TEXT,
                display_name TEXT,
                profile_json TEXT NOT NULL DEFAULT '{}',
                desired_running INTEGER NOT NULL DEFAULT 1,
                error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS telegram_account_permissions (
                account_id TEXT PRIMARY KEY NOT NULL,
                read_messages INTEGER NOT NULL DEFAULT 1,
                send_messages INTEGER NOT NULL DEFAULT 1,
                forward_messages INTEGER NOT NULL DEFAULT 1,
                edit_messages INTEGER NOT NULL DEFAULT 1,
                delete_messages INTEGER NOT NULL DEFAULT 1,
                manage_chats INTEGER NOT NULL DEFAULT 1,
                FOREIGN KEY(account_id) REFERENCES telegram_accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS telegram_scenario_executions (
                id TEXT PRIMARY KEY NOT NULL,
                execution_id TEXT NOT NULL,
                workflow_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                event_key TEXT NOT NULL,
                status TEXT NOT NULL,
                error TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS telegram_audit_log (
                id TEXT PRIMARY KEY NOT NULL,
                account_id TEXT NOT NULL,
                scenario_id TEXT,
                execution_id TEXT,
                event_type TEXT NOT NULL,
                action TEXT NOT NULL,
                target TEXT,
                status TEXT NOT NULL,
                error TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS telegram_idempotency (
                account_id TEXT NOT NULL,
                event_key TEXT NOT NULL,
                execution_id TEXT,
                created_at TEXT NOT NULL,
                PRIMARY KEY (account_id, event_key)
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_account(&self, account: &TelegramAccount) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO telegram_accounts
             (id, status, phone_masked, username, display_name, profile_json, desired_running, error, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, '{}', ?, ?, ?, ?)",
        )
        .bind(account.id.to_string())
        .bind(account.status.as_str())
        .bind(&account.phone_masked)
        .bind(&account.username)
        .bind(&account.display_name)
        .bind(if account.desired_running { 1 } else { 0 })
        .bind(&account.error)
        .bind(account.created_at.to_rfc3339())
        .bind(account.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        self.upsert_permissions(account.id, &account.permissions)
            .await?;
        Ok(())
    }

    pub async fn upsert_permissions(
        &self,
        account_id: TelegramAccountId,
        p: &TelegramAccountPermissions,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO telegram_account_permissions
             (account_id, read_messages, send_messages, forward_messages, edit_messages, delete_messages, manage_chats)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(account_id) DO UPDATE SET
                read_messages = excluded.read_messages,
                send_messages = excluded.send_messages,
                forward_messages = excluded.forward_messages,
                edit_messages = excluded.edit_messages,
                delete_messages = excluded.delete_messages,
                manage_chats = excluded.manage_chats",
        )
        .bind(account_id.to_string())
        .bind(p.read_messages as i64)
        .bind(p.send_messages as i64)
        .bind(p.forward_messages as i64)
        .bind(p.edit_messages as i64)
        .bind(p.delete_messages as i64)
        .bind(p.manage_chats as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_account(&self, account: &TelegramAccount) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE telegram_accounts SET
                status = ?, phone_masked = ?, username = ?, display_name = ?,
                desired_running = ?, error = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(account.status.as_str())
        .bind(&account.phone_masked)
        .bind(&account.username)
        .bind(&account.display_name)
        .bind(if account.desired_running { 1 } else { 0 })
        .bind(&account.error)
        .bind(account.updated_at.to_rfc3339())
        .bind(account.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_account(
        &self,
        id: TelegramAccountId,
    ) -> anyhow::Result<Option<TelegramAccount>> {
        let row = sqlx::query(
            "SELECT a.id, a.status, a.phone_masked, a.username, a.display_name,
                    a.desired_running, a.error, a.created_at, a.updated_at,
                    COALESCE(p.read_messages, 1), COALESCE(p.send_messages, 1),
                    COALESCE(p.forward_messages, 1), COALESCE(p.edit_messages, 1),
                    COALESCE(p.delete_messages, 1), COALESCE(p.manage_chats, 1)
             FROM telegram_accounts a
             LEFT JOIN telegram_account_permissions p ON p.account_id = a.id
             WHERE a.id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_account).transpose()
    }

    pub async fn list_accounts(&self) -> anyhow::Result<Vec<TelegramAccount>> {
        let rows = sqlx::query(
            "SELECT a.id, a.status, a.phone_masked, a.username, a.display_name,
                    a.desired_running, a.error, a.created_at, a.updated_at,
                    COALESCE(p.read_messages, 1), COALESCE(p.send_messages, 1),
                    COALESCE(p.forward_messages, 1), COALESCE(p.edit_messages, 1),
                    COALESCE(p.delete_messages, 1), COALESCE(p.manage_chats, 1)
             FROM telegram_accounts a
             LEFT JOIN telegram_account_permissions p ON p.account_id = a.id
             ORDER BY a.created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_account).collect()
    }

    pub async fn delete_account(&self, id: TelegramAccountId) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM telegram_accounts WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn claim_idempotency(
        &self,
        account_id: TelegramAccountId,
        event_key: &str,
    ) -> anyhow::Result<bool> {
        let res = sqlx::query(
            "INSERT OR IGNORE INTO telegram_idempotency (account_id, event_key, created_at)
             VALUES (?, ?, ?)",
        )
        .bind(account_id.to_string())
        .bind(event_key)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn insert_audit(&self, event: &TelegramAuditEvent) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO telegram_audit_log
             (id, account_id, scenario_id, execution_id, event_type, action, target, status, error, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(event.id.to_string())
        .bind(event.account_id.to_string())
        .bind(event.scenario_id.map(|id| id.to_string()))
        .bind(event.execution_id.map(|id| id.to_string()))
        .bind(&event.event_type)
        .bind(&event.action)
        .bind(&event.target)
        .bind(event.status.as_str())
        .bind(&event.error)
        .bind(event.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_audit(
        &self,
        account_id: Option<TelegramAccountId>,
        limit: i64,
    ) -> anyhow::Result<Vec<TelegramAuditEvent>> {
        let rows = if let Some(id) = account_id {
            sqlx::query(
                "SELECT id, account_id, scenario_id, execution_id, event_type, action, target, status, error, created_at
                 FROM telegram_audit_log WHERE account_id = ? ORDER BY created_at DESC LIMIT ?",
            )
            .bind(id.to_string())
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, account_id, scenario_id, execution_id, event_type, action, target, status, error, created_at
                 FROM telegram_audit_log ORDER BY created_at DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(row_to_audit).collect()
    }

    pub async fn insert_scenario_execution(
        &self,
        row: &TelegramScenarioExecution,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO telegram_scenario_executions
             (id, execution_id, workflow_id, account_id, event_key, status, error, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(row.id.to_string())
        .bind(row.execution_id.to_string())
        .bind(row.workflow_id.to_string())
        .bind(row.account_id.to_string())
        .bind(&row.event_key)
        .bind(&row.status)
        .bind(&row.error)
        .bind(row.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn parse_uuid(s: &str) -> anyhow::Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| anyhow::anyhow!(e))
}

fn parse_time(s: &str) -> DateTimeUtc {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

type DateTimeUtc = chrono::DateTime<Utc>;

fn row_to_account(row: SqliteRow) -> anyhow::Result<TelegramAccount> {
    let id = parse_uuid(row.get::<String, _>(0).as_str())?;
    Ok(TelegramAccount {
        id: TelegramAccountId(id),
        status: TelegramAccountStatus::parse(row.get::<String, _>(1).as_str()),
        phone_masked: row.get(2),
        username: row.get(3),
        display_name: row.get(4),
        desired_running: row.get::<i64, _>(5) != 0,
        error: row.get(6),
        created_at: parse_time(row.get::<String, _>(7).as_str()),
        updated_at: parse_time(row.get::<String, _>(8).as_str()),
        permissions: TelegramAccountPermissions {
            read_messages: row.get::<i64, _>(9) != 0,
            send_messages: row.get::<i64, _>(10) != 0,
            forward_messages: row.get::<i64, _>(11) != 0,
            edit_messages: row.get::<i64, _>(12) != 0,
            delete_messages: row.get::<i64, _>(13) != 0,
            manage_chats: row.get::<i64, _>(14) != 0,
        },
    })
}

fn row_to_audit(row: SqliteRow) -> anyhow::Result<TelegramAuditEvent> {
    Ok(TelegramAuditEvent {
        id: parse_uuid(row.get::<String, _>(0).as_str())?,
        account_id: TelegramAccountId(parse_uuid(row.get::<String, _>(1).as_str())?),
        scenario_id: row
            .get::<Option<String>, _>(2)
            .and_then(|s| Uuid::parse_str(&s).ok()),
        execution_id: row
            .get::<Option<String>, _>(3)
            .and_then(|s| Uuid::parse_str(&s).ok()),
        event_type: row.get(4),
        action: row.get(5),
        target: row.get(6),
        status: TelegramAuditStatus::parse(row.get::<String, _>(7).as_str()),
        error: row.get(8),
        created_at: parse_time(row.get::<String, _>(9).as_str()),
    })
}
