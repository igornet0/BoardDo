use boarddo_shared::{
    Channel, ChannelKind, Connection, CreateWorkflowRequest, Execution, ExecutionChangelog,
    ExecutionChangelogEntry, ExecutionStatus, NodeExecution, Stream, StreamDirection, Trigger,
    TriggerKind, UpdateWorkflowRequest, WorkflowDefinition, WorkflowRecord, WorkflowSchedule,
    WorkflowStatus, WorkflowSummary,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{FromRow, SqlitePool, sqlite::SqlitePoolOptions};
use uuid::Uuid;

use crate::triggers::schedule::{next_run_after, schedules_from_definition};

pub(crate) mod content;
mod custom_tools;
mod goals;
mod marketing;

#[derive(Clone)]
pub struct Storage {
    pool: SqlitePool,
}

#[derive(FromRow)]
struct WorkflowRow {
    id: String,
    name: String,
    description: String,
    version: i64,
    status: String,
    definition: String,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct ExecutionRow {
    id: String,
    workflow_id: String,
    workflow_version: i64,
    status: String,
    trigger_type: String,
    trigger_data: String,
    error: Option<String>,
    started_at: String,
    finished_at: Option<String>,
}

#[derive(FromRow)]
struct ChangelogRow {
    execution_id: String,
    workflow_id: String,
    trigger_type: String,
    trigger_data: String,
    status: String,
    error: Option<String>,
    entries: String,
    started_at: String,
    finished_at: Option<String>,
}

#[derive(FromRow)]
struct ScheduleRow {
    id: String,
    workflow_id: String,
    node_id: String,
    cron: String,
    timezone: String,
    enabled: i64,
    next_run_at: String,
    last_run_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct ConnectionRow {
    id: String,
    name: String,
    #[sqlx(rename = "type")]
    connection_type: String,
    config: String,
    secret_ref: Option<String>,
    enabled: i64,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct ChannelRow {
    id: String,
    name: String,
    description: String,
    kind: String,
    enabled: i64,
    config: String,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct StreamRow {
    id: String,
    channel_id: String,
    name: String,
    description: String,
    direction: String,
    enabled: i64,
    config: String,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct TriggerRow {
    id: String,
    stream_id: String,
    name: String,
    description: String,
    kind: String,
    enabled: i64,
    workflow_id: Option<String>,
    config: String,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct NodeExecutionRow {
    id: String,
    execution_id: String,
    node_id: String,
    status: String,
    input: String,
    output: Option<String>,
    error: Option<String>,
    started_at: String,
    finished_at: Option<String>,
    duration_ms: Option<i64>,
}

impl Storage {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// Test helper / shared pool for secret store.
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS workflows (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                version INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT 'draft',
                definition TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS workflow_versions (
                id TEXT PRIMARY KEY NOT NULL,
                workflow_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                definition TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(workflow_id, version),
                FOREIGN KEY(workflow_id) REFERENCES workflows(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS executions (
                id TEXT PRIMARY KEY NOT NULL,
                workflow_id TEXT NOT NULL,
                workflow_version INTEGER NOT NULL,
                status TEXT NOT NULL,
                trigger_type TEXT NOT NULL DEFAULT 'manual',
                trigger_data TEXT NOT NULL DEFAULT '{}',
                error TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                FOREIGN KEY(workflow_id) REFERENCES workflows(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS node_executions (
                id TEXT PRIMARY KEY NOT NULL,
                execution_id TEXT NOT NULL,
                node_id TEXT NOT NULL,
                status TEXT NOT NULL,
                input TEXT NOT NULL,
                output TEXT,
                error TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                duration_ms INTEGER,
                FOREIGN KEY(execution_id) REFERENCES executions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_changelogs (
                execution_id TEXT PRIMARY KEY NOT NULL,
                workflow_id TEXT NOT NULL,
                trigger_type TEXT NOT NULL DEFAULT 'manual',
                trigger_data TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL,
                error TEXT,
                entries TEXT NOT NULL DEFAULT '[]',
                started_at TEXT NOT NULL,
                finished_at TEXT,
                FOREIGN KEY(execution_id) REFERENCES executions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS workflow_schedules (
                id TEXT PRIMARY KEY NOT NULL,
                workflow_id TEXT NOT NULL,
                node_id TEXT NOT NULL,
                cron TEXT NOT NULL,
                timezone TEXT NOT NULL DEFAULT 'UTC',
                enabled INTEGER NOT NULL DEFAULT 1,
                next_run_at TEXT NOT NULL,
                last_run_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(workflow_id, node_id),
                FOREIGN KEY(workflow_id) REFERENCES workflows(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS connections (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                type TEXT NOT NULL,
                config TEXT NOT NULL DEFAULT '{}',
                secret_ref TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS channels (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL UNIQUE,
                description TEXT NOT NULL DEFAULT '',
                kind TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                config TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS streams (
                id TEXT PRIMARY KEY NOT NULL,
                channel_id TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                direction TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                config TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(channel_id, name),
                FOREIGN KEY(channel_id) REFERENCES channels(id)
            );

            CREATE TABLE IF NOT EXISTS triggers (
                id TEXT PRIMARY KEY NOT NULL,
                stream_id TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                kind TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                workflow_id TEXT,
                config TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(stream_id, name),
                FOREIGN KEY(stream_id) REFERENCES streams(id)
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Soft-upgrade existing DBs created before trigger_type existed.
        let _ = sqlx::query(
            "ALTER TABLE executions ADD COLUMN trigger_type TEXT NOT NULL DEFAULT 'manual'",
        )
        .execute(&self.pool)
        .await;

        // Soft-upgrade: changelogs table for DBs created before this feature.
        let _ = sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS execution_changelogs (
                execution_id TEXT PRIMARY KEY NOT NULL,
                workflow_id TEXT NOT NULL,
                trigger_type TEXT NOT NULL DEFAULT 'manual',
                trigger_data TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL,
                error TEXT,
                entries TEXT NOT NULL DEFAULT '[]',
                started_at TEXT NOT NULL,
                finished_at TEXT,
                FOREIGN KEY(execution_id) REFERENCES executions(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await;

        // Soft-upgrade: node runtime state (e.g. last seen GitHub release tag).
        let _ = sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS node_state (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await;

        self.migrate_goals().await?;
        self.migrate_marketing().await?;
        self.migrate_content_engine().await?;
        self.migrate_custom_tools().await?;

        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn list_workflows(&self) -> anyhow::Result<Vec<WorkflowSummary>> {
        let rows: Vec<WorkflowRow> = sqlx::query_as(
            "SELECT id, name, description, version, status, definition, created_at, updated_at
             FROM workflows ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_summary).collect()
    }

    pub async fn get_workflow(&self, id: Uuid) -> anyhow::Result<Option<WorkflowRecord>> {
        let row: Option<WorkflowRow> = sqlx::query_as(
            "SELECT id, name, description, version, status, definition, created_at, updated_at
             FROM workflows WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_record).transpose()
    }

    pub async fn create_workflow(
        &self,
        req: CreateWorkflowRequest,
    ) -> anyhow::Result<WorkflowRecord> {
        let id = Uuid::now_v7();
        let now = Utc::now();
        let version = 1u32;
        let definition = WorkflowDefinition {
            nodes: req.nodes,
            edges: req.edges,
        };
        let definition_json = serde_json::to_string(&definition)?;
        let status = WorkflowStatus::Draft;

        sqlx::query(
            "INSERT INTO workflows (id, name, description, version, status, definition, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&req.name)
        .bind(&req.description)
        .bind(version as i64)
        .bind(status_str(status))
        .bind(&definition_json)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        self.insert_version(id, version, &definition, now).await?;
        self.sync_schedules(id, &definition, status).await?;

        Ok(WorkflowRecord {
            id,
            name: req.name,
            description: req.description,
            version,
            status,
            definition,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn update_workflow(
        &self,
        id: Uuid,
        req: UpdateWorkflowRequest,
    ) -> anyhow::Result<Option<WorkflowRecord>> {
        let existing = match self.get_workflow(id).await? {
            Some(w) => w,
            None => return Ok(None),
        };

        let now = Utc::now();
        let version = existing.version + 1;
        let definition = WorkflowDefinition {
            nodes: req.nodes,
            edges: req.edges,
        };
        let definition_json = serde_json::to_string(&definition)?;

        sqlx::query(
            "UPDATE workflows SET name = ?, description = ?, version = ?, status = ?, definition = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&req.name)
        .bind(&req.description)
        .bind(version as i64)
        .bind(status_str(req.status))
        .bind(&definition_json)
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        self.insert_version(id, version, &definition, now).await?;
        self.sync_schedules(id, &definition, req.status).await?;

        Ok(Some(WorkflowRecord {
            id,
            name: req.name,
            description: req.description,
            version,
            status: req.status,
            definition,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    pub async fn set_workflow_status(
        &self,
        id: Uuid,
        status: WorkflowStatus,
    ) -> anyhow::Result<Option<WorkflowRecord>> {
        let existing = match self.get_workflow(id).await? {
            Some(w) => w,
            None => return Ok(None),
        };
        let now = Utc::now();
        sqlx::query("UPDATE workflows SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status_str(status))
            .bind(now.to_rfc3339())
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        self.sync_schedules(id, &existing.definition, status)
            .await?;
        Ok(Some(WorkflowRecord {
            status,
            updated_at: now,
            ..existing
        }))
    }

    pub async fn delete_workflow(&self, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM workflows WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_workflow_version_definition(
        &self,
        workflow_id: Uuid,
        version: u32,
    ) -> anyhow::Result<Option<WorkflowDefinition>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT definition FROM workflow_versions WHERE workflow_id = ? AND version = ?",
        )
        .bind(workflow_id.to_string())
        .bind(version as i64)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|(d,)| serde_json::from_str(&d).map_err(Into::into))
            .transpose()
    }

    async fn insert_version(
        &self,
        workflow_id: Uuid,
        version: u32,
        definition: &WorkflowDefinition,
        created_at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let version_id = Uuid::now_v7();
        let definition_json = serde_json::to_string(definition)?;
        sqlx::query(
            "INSERT INTO workflow_versions (id, workflow_id, version, definition, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(version_id.to_string())
        .bind(workflow_id.to_string())
        .bind(version as i64)
        .bind(definition_json)
        .bind(created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_execution(
        &self,
        id: Uuid,
        workflow_id: Uuid,
        workflow_version: u32,
        trigger_type: &str,
        trigger: &Value,
    ) -> anyhow::Result<Execution> {
        let now = Utc::now();
        let trigger_json = serde_json::to_string(trigger)?;
        sqlx::query(
            "INSERT INTO executions
             (id, workflow_id, workflow_version, status, trigger_type, trigger_data, started_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(workflow_id.to_string())
        .bind(workflow_version as i64)
        .bind(exec_status_str(ExecutionStatus::Running))
        .bind(trigger_type)
        .bind(trigger_json)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(Execution {
            id,
            workflow_id,
            workflow_version,
            status: ExecutionStatus::Running,
            trigger_type: trigger_type.to_string(),
            trigger: trigger.clone(),
            started_at: now,
            finished_at: None,
            error: None,
            changelog: vec![ExecutionChangelogEntry::accepted(trigger_type, trigger)],
        })
    }

    /// Create the per-run changelog document (one row per execution).
    pub async fn create_execution_changelog(
        &self,
        execution_id: Uuid,
        workflow_id: Uuid,
        trigger_type: &str,
        trigger: &Value,
        initial: &[ExecutionChangelogEntry],
    ) -> anyhow::Result<()> {
        let now = Utc::now();
        let entries = serde_json::to_string(initial)?;
        sqlx::query(
            "INSERT INTO execution_changelogs
             (execution_id, workflow_id, trigger_type, trigger_data, status, entries, started_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(execution_id) DO NOTHING",
        )
        .bind(execution_id.to_string())
        .bind(workflow_id.to_string())
        .bind(trigger_type)
        .bind(serde_json::to_string(trigger)?)
        .bind(exec_status_str(ExecutionStatus::Running))
        .bind(entries)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Append timeline entries and optionally refresh status for a run changelog.
    pub async fn append_execution_changelog(
        &self,
        execution_id: Uuid,
        entries: &[ExecutionChangelogEntry],
        status: Option<ExecutionStatus>,
        error: Option<&str>,
        finished: bool,
    ) -> anyhow::Result<()> {
        if entries.is_empty() && status.is_none() && !finished {
            return Ok(());
        }

        let row: Option<(String,)> =
            sqlx::query_as("SELECT entries FROM execution_changelogs WHERE execution_id = ?")
                .bind(execution_id.to_string())
                .fetch_optional(&self.pool)
                .await?;

        let mut existing: Vec<ExecutionChangelogEntry> = match row {
            Some((raw,)) => serde_json::from_str(&raw).unwrap_or_default(),
            None => Vec::new(),
        };
        existing.extend(entries.iter().cloned());
        let entries_json = serde_json::to_string(&existing)?;

        let finished_at = if finished {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };

        if let Some(status) = status {
            sqlx::query(
                "UPDATE execution_changelogs
                 SET entries = ?, status = ?, error = COALESCE(?, error),
                     finished_at = COALESCE(?, finished_at)
                 WHERE execution_id = ?",
            )
            .bind(&entries_json)
            .bind(exec_status_str(status))
            .bind(error)
            .bind(finished_at)
            .bind(execution_id.to_string())
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                "UPDATE execution_changelogs
                 SET entries = ?, finished_at = COALESCE(?, finished_at)
                 WHERE execution_id = ?",
            )
            .bind(&entries_json)
            .bind(finished_at)
            .bind(execution_id.to_string())
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn finish_execution_changelog(
        &self,
        execution_id: Uuid,
        status: ExecutionStatus,
        error: Option<String>,
        entries: &[ExecutionChangelogEntry],
    ) -> anyhow::Result<()> {
        let finished_at = Utc::now();
        let entries_json = serde_json::to_string(entries)?;
        sqlx::query(
            "UPDATE execution_changelogs
             SET entries = ?, status = ?, error = ?, finished_at = ?
             WHERE execution_id = ?",
        )
        .bind(entries_json)
        .bind(exec_status_str(status))
        .bind(&error)
        .bind(finished_at.to_rfc3339())
        .bind(execution_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_execution_changelog(
        &self,
        execution_id: Uuid,
    ) -> anyhow::Result<Option<ExecutionChangelog>> {
        let row: Option<ChangelogRow> = sqlx::query_as(
            "SELECT execution_id, workflow_id, trigger_type, trigger_data, status, error,
                    entries, started_at, finished_at
             FROM execution_changelogs WHERE execution_id = ?",
        )
        .bind(execution_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_changelog).transpose()
    }

    pub async fn list_execution_changelogs(
        &self,
        workflow_id: Option<Uuid>,
        limit: i64,
    ) -> anyhow::Result<Vec<ExecutionChangelog>> {
        let limit = limit.clamp(1, 500);
        let rows: Vec<ChangelogRow> = if let Some(wf) = workflow_id {
            sqlx::query_as(
                "SELECT execution_id, workflow_id, trigger_type, trigger_data, status, error,
                        entries, started_at, finished_at
                 FROM execution_changelogs
                 WHERE workflow_id = ?
                 ORDER BY started_at DESC LIMIT ?",
            )
            .bind(wf.to_string())
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT execution_id, workflow_id, trigger_type, trigger_data, status, error,
                        entries, started_at, finished_at
                 FROM execution_changelogs
                 ORDER BY started_at DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(row_to_changelog).collect()
    }

    pub async fn finish_execution(
        &self,
        id: Uuid,
        status: ExecutionStatus,
        error: Option<String>,
        node_executions: &[NodeExecution],
    ) -> anyhow::Result<()> {
        let finished_at = Utc::now();
        sqlx::query("UPDATE executions SET status = ?, error = ?, finished_at = ? WHERE id = ?")
            .bind(exec_status_str(status))
            .bind(&error)
            .bind(finished_at.to_rfc3339())
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;

        for ne in node_executions {
            sqlx::query(
                "INSERT INTO node_executions
                 (id, execution_id, node_id, status, input, output, error, started_at, finished_at, duration_ms)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(ne.id.to_string())
            .bind(ne.execution_id.to_string())
            .bind(&ne.node_id)
            .bind(exec_status_str(ne.status))
            .bind(serde_json::to_string(&ne.input)?)
            .bind(ne.output.as_ref().map(serde_json::to_string).transpose()?)
            .bind(&ne.error)
            .bind(ne.started_at.to_rfc3339())
            .bind(ne.finished_at.map(|t| t.to_rfc3339()))
            .bind(ne.duration_ms.map(|d| d as i64))
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn list_executions(&self) -> anyhow::Result<Vec<Execution>> {
        let rows: Vec<ExecutionRow> = sqlx::query_as(
            "SELECT id, workflow_id, workflow_version, status,
                    COALESCE(trigger_type, 'manual') as trigger_type,
                    trigger_data, error, started_at, finished_at
             FROM executions ORDER BY started_at DESC LIMIT 100",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_execution).collect()
    }

    pub async fn list_executions_for_workflow(
        &self,
        workflow_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<Execution>> {
        let limit = limit.clamp(1, 100);
        let rows: Vec<ExecutionRow> = sqlx::query_as(
            "SELECT id, workflow_id, workflow_version, status,
                    COALESCE(trigger_type, 'manual') as trigger_type,
                    trigger_data, error, started_at, finished_at
             FROM executions WHERE workflow_id = ?
             ORDER BY started_at DESC LIMIT ?",
        )
        .bind(workflow_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_execution).collect()
    }

    pub async fn count_executions(&self, workflow_id: Uuid) -> anyhow::Result<u64> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM executions WHERE workflow_id = ?")
                .bind(workflow_id.to_string())
                .fetch_one(&self.pool)
                .await?;
        Ok(count.max(0) as u64)
    }

    pub async fn get_execution(&self, id: Uuid) -> anyhow::Result<Option<Execution>> {
        let row: Option<ExecutionRow> = sqlx::query_as(
            "SELECT id, workflow_id, workflow_version, status,
                    COALESCE(trigger_type, 'manual') as trigger_type,
                    trigger_data, error, started_at, finished_at
             FROM executions WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let mut execution = match row.map(row_to_execution).transpose()? {
            Some(ex) => ex,
            None => return Ok(None),
        };
        if let Some(log) = self.get_execution_changelog(id).await? {
            execution.changelog = log.entries;
        }
        Ok(Some(execution))
    }

    pub async fn get_node_executions(
        &self,
        execution_id: Uuid,
    ) -> anyhow::Result<Vec<NodeExecution>> {
        let rows: Vec<NodeExecutionRow> = sqlx::query_as(
            "SELECT id, execution_id, node_id, status, input, output, error, started_at, finished_at, duration_ms
             FROM node_executions WHERE execution_id = ? ORDER BY started_at ASC",
        )
        .bind(execution_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_node_execution).collect()
    }

    /// Rebuild schedule rows from workflow definition. Enabled only when workflow is `active`.
    pub async fn sync_schedules(
        &self,
        workflow_id: Uuid,
        definition: &WorkflowDefinition,
        status: WorkflowStatus,
    ) -> anyhow::Result<()> {
        let now = Utc::now();
        let enabled = status == WorkflowStatus::Active;

        // Remove stale schedules for this workflow, then upsert current ones.
        sqlx::query("DELETE FROM workflow_schedules WHERE workflow_id = ?")
            .bind(workflow_id.to_string())
            .execute(&self.pool)
            .await?;

        let schedules = match schedules_from_definition(definition) {
            Ok(s) => s,
            Err(errors) => {
                // Keep workflow saveable even if schedule config is incomplete while drafting.
                if enabled {
                    anyhow::bail!("schedule sync failed: {}", errors.join("; "));
                }
                tracing::warn!(
                    %workflow_id,
                    errors = %errors.join("; "),
                    "skipping invalid schedules for non-active workflow"
                );
                return Ok(());
            }
        };

        for (node_id, parsed) in schedules {
            let next = next_run_after(&parsed.cron, &parsed.timezone, now)
                .map_err(|e| anyhow::anyhow!(e))?;
            let id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO workflow_schedules
                 (id, workflow_id, node_id, cron, timezone, enabled, next_run_at, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id.to_string())
            .bind(workflow_id.to_string())
            .bind(&node_id)
            .bind(&parsed.cron)
            .bind(&parsed.timezone)
            .bind(if enabled { 1i64 } else { 0i64 })
            .bind(next.to_rfc3339())
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn list_due_schedules(
        &self,
        now: DateTime<Utc>,
    ) -> anyhow::Result<Vec<WorkflowSchedule>> {
        let rows: Vec<ScheduleRow> = sqlx::query_as(
            "SELECT id, workflow_id, node_id, cron, timezone, enabled, next_run_at, last_run_at, created_at, updated_at
             FROM workflow_schedules
             WHERE enabled = 1 AND next_run_at <= ?
             ORDER BY next_run_at ASC
             LIMIT 50",
        )
        .bind(now.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_schedule).collect()
    }

    /// Atomically claim a due schedule and advance next_run_at.
    pub async fn claim_schedule(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
        next_run_at: DateTime<Utc>,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE workflow_schedules
             SET last_run_at = ?, next_run_at = ?, updated_at = ?
             WHERE id = ? AND enabled = 1 AND next_run_at <= ?",
        )
        .bind(now.to_rfc3339())
        .bind(next_run_at.to_rfc3339())
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_schedule_enabled(&self, id: Uuid, enabled: bool) -> anyhow::Result<()> {
        sqlx::query("UPDATE workflow_schedules SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(if enabled { 1i64 } else { 0i64 })
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_schedules_for_workflow(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<WorkflowSchedule>> {
        let rows: Vec<ScheduleRow> = sqlx::query_as(
            "SELECT id, workflow_id, node_id, cron, timezone, enabled, next_run_at, last_run_at, created_at, updated_at
             FROM workflow_schedules WHERE workflow_id = ?",
        )
        .bind(workflow_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_schedule).collect()
    }

    // ─── Connections (public metadata only) ───────────────────────────────────

    pub async fn insert_connection(
        &self,
        id: Uuid,
        name: &str,
        connection_type: &str,
        config: &serde_json::Value,
        secret_ref: Option<&str>,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO connections (id, name, type, config, secret_ref, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(name)
        .bind(connection_type)
        .bind(serde_json::to_string(config)?)
        .bind(secret_ref)
        .bind(if enabled { 1i64 } else { 0i64 })
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_connection(
        &self,
        id: Uuid,
        name: &str,
        connection_type: &str,
        config: &serde_json::Value,
        secret_ref: Option<&str>,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE connections SET name = ?, type = ?, config = ?, secret_ref = ?, enabled = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(connection_type)
        .bind(serde_json::to_string(config)?)
        .bind(secret_ref)
        .bind(if enabled { 1i64 } else { 0i64 })
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_connection(&self, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM connections WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_connections(&self) -> anyhow::Result<Vec<Connection>> {
        let rows: Vec<ConnectionRow> = sqlx::query_as(
            "SELECT id, name, type, config, secret_ref, enabled, created_at, updated_at
             FROM connections ORDER BY name ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_connection).collect()
    }

    pub async fn get_connection(&self, id: Uuid) -> anyhow::Result<Option<Connection>> {
        let row: Option<ConnectionRow> = sqlx::query_as(
            "SELECT id, name, type, config, secret_ref, enabled, created_at, updated_at
             FROM connections WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_connection).transpose()
    }

    pub async fn list_connections_by_type(
        &self,
        connection_type: &str,
    ) -> anyhow::Result<Vec<Connection>> {
        let rows: Vec<ConnectionRow> = sqlx::query_as(
            "SELECT id, name, type, config, secret_ref, enabled, created_at, updated_at
             FROM connections WHERE type = ? ORDER BY name ASC",
        )
        .bind(connection_type)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_connection).collect()
    }

    pub async fn get_node_state(&self, key: &str) -> anyhow::Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM node_state WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn set_node_state(
        &self,
        key: &str,
        value: &str,
        updated_at: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO node_state (key, value, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key)
        .bind(value)
        .bind(updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Channel / Stream / Trigger (configuration lifecycle) ────────────

    pub async fn insert_channel(&self, ch: &Channel) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO channels (id, name, description, kind, enabled, config, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(ch.id.to_string())
        .bind(&ch.name)
        .bind(&ch.description)
        .bind(ch.kind.as_str())
        .bind(if ch.enabled { 1 } else { 0 })
        .bind(serde_json::to_string(&ch.config)?)
        .bind(ch.created_at.to_rfc3339())
        .bind(ch.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_channel(&self, ch: &Channel) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE channels SET name=?, description=?, kind=?, enabled=?, config=?, updated_at=?
             WHERE id=?",
        )
        .bind(&ch.name)
        .bind(&ch.description)
        .bind(ch.kind.as_str())
        .bind(if ch.enabled { 1 } else { 0 })
        .bind(serde_json::to_string(&ch.config)?)
        .bind(ch.updated_at.to_rfc3339())
        .bind(ch.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_channel(&self, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query("DELETE FROM channels WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn get_channel(&self, id: Uuid) -> anyhow::Result<Option<Channel>> {
        let row: Option<ChannelRow> = sqlx::query_as(
            "SELECT id, name, description, kind, enabled, config, created_at, updated_at
             FROM channels WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_channel).transpose()
    }

    pub async fn find_channel_by_name(&self, name: &str) -> anyhow::Result<Option<Channel>> {
        let row: Option<ChannelRow> = sqlx::query_as(
            "SELECT id, name, description, kind, enabled, config, created_at, updated_at
             FROM channels WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_channel).transpose()
    }

    pub async fn list_channels(&self) -> anyhow::Result<Vec<Channel>> {
        let rows: Vec<ChannelRow> = sqlx::query_as(
            "SELECT id, name, description, kind, enabled, config, created_at, updated_at
             FROM channels ORDER BY name ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_channel).collect()
    }

    pub async fn count_streams_for_channel(&self, channel_id: Uuid) -> anyhow::Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM streams WHERE channel_id = ?")
            .bind(channel_id.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    pub async fn insert_stream(&self, s: &Stream) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO streams (id, channel_id, name, description, direction, enabled, config, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(s.id.to_string())
        .bind(s.channel_id.to_string())
        .bind(&s.name)
        .bind(&s.description)
        .bind(s.direction.as_str())
        .bind(if s.enabled { 1 } else { 0 })
        .bind(serde_json::to_string(&s.config)?)
        .bind(s.created_at.to_rfc3339())
        .bind(s.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_stream(&self, s: &Stream) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE streams SET channel_id=?, name=?, description=?, direction=?, enabled=?, config=?, updated_at=?
             WHERE id=?",
        )
        .bind(s.channel_id.to_string())
        .bind(&s.name)
        .bind(&s.description)
        .bind(s.direction.as_str())
        .bind(if s.enabled { 1 } else { 0 })
        .bind(serde_json::to_string(&s.config)?)
        .bind(s.updated_at.to_rfc3339())
        .bind(s.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_stream(&self, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query("DELETE FROM streams WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn get_stream(&self, id: Uuid) -> anyhow::Result<Option<Stream>> {
        let row: Option<StreamRow> = sqlx::query_as(
            "SELECT id, channel_id, name, description, direction, enabled, config, created_at, updated_at
             FROM streams WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_stream).transpose()
    }

    pub async fn find_stream_by_name(
        &self,
        channel_id: Uuid,
        name: &str,
    ) -> anyhow::Result<Option<Stream>> {
        let row: Option<StreamRow> = sqlx::query_as(
            "SELECT id, channel_id, name, description, direction, enabled, config, created_at, updated_at
             FROM streams WHERE channel_id = ? AND name = ?",
        )
        .bind(channel_id.to_string())
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_stream).transpose()
    }

    pub async fn list_streams(&self, channel_id: Option<Uuid>) -> anyhow::Result<Vec<Stream>> {
        let rows: Vec<StreamRow> = if let Some(cid) = channel_id {
            sqlx::query_as(
                "SELECT id, channel_id, name, description, direction, enabled, config, created_at, updated_at
                 FROM streams WHERE channel_id = ? ORDER BY name ASC",
            )
            .bind(cid.to_string())
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT id, channel_id, name, description, direction, enabled, config, created_at, updated_at
                 FROM streams ORDER BY name ASC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(row_to_stream).collect()
    }

    pub async fn count_triggers_for_stream(&self, stream_id: Uuid) -> anyhow::Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM triggers WHERE stream_id = ?")
            .bind(stream_id.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    pub async fn insert_trigger(&self, t: &Trigger) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO triggers (id, stream_id, name, description, kind, enabled, workflow_id, config, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(t.id.to_string())
        .bind(t.stream_id.to_string())
        .bind(&t.name)
        .bind(&t.description)
        .bind(t.kind.as_str())
        .bind(if t.enabled { 1 } else { 0 })
        .bind(t.workflow_id.map(|id| id.to_string()))
        .bind(serde_json::to_string(&t.config)?)
        .bind(t.created_at.to_rfc3339())
        .bind(t.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_trigger(&self, t: &Trigger) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE triggers SET stream_id=?, name=?, description=?, kind=?, enabled=?, workflow_id=?, config=?, updated_at=?
             WHERE id=?",
        )
        .bind(t.stream_id.to_string())
        .bind(&t.name)
        .bind(&t.description)
        .bind(t.kind.as_str())
        .bind(if t.enabled { 1 } else { 0 })
        .bind(t.workflow_id.map(|id| id.to_string()))
        .bind(serde_json::to_string(&t.config)?)
        .bind(t.updated_at.to_rfc3339())
        .bind(t.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_trigger(&self, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query("DELETE FROM triggers WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn get_trigger(&self, id: Uuid) -> anyhow::Result<Option<Trigger>> {
        let row: Option<TriggerRow> = sqlx::query_as(
            "SELECT id, stream_id, name, description, kind, enabled, workflow_id, config, created_at, updated_at
             FROM triggers WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_trigger).transpose()
    }

    pub async fn find_trigger_by_name(
        &self,
        stream_id: Uuid,
        name: &str,
    ) -> anyhow::Result<Option<Trigger>> {
        let row: Option<TriggerRow> = sqlx::query_as(
            "SELECT id, stream_id, name, description, kind, enabled, workflow_id, config, created_at, updated_at
             FROM triggers WHERE stream_id = ? AND name = ?",
        )
        .bind(stream_id.to_string())
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_trigger).transpose()
    }

    pub async fn list_triggers(&self, stream_id: Option<Uuid>) -> anyhow::Result<Vec<Trigger>> {
        let rows: Vec<TriggerRow> = if let Some(sid) = stream_id {
            sqlx::query_as(
                "SELECT id, stream_id, name, description, kind, enabled, workflow_id, config, created_at, updated_at
                 FROM triggers WHERE stream_id = ? ORDER BY name ASC",
            )
            .bind(sid.to_string())
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT id, stream_id, name, description, kind, enabled, workflow_id, config, created_at, updated_at
                 FROM triggers ORDER BY name ASC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(row_to_trigger).collect()
    }

    pub async fn list_active_workflows(&self) -> anyhow::Result<Vec<WorkflowRecord>> {
        let rows: Vec<WorkflowRow> = sqlx::query_as(
            "SELECT id, name, description, version, status, definition, created_at, updated_at
             FROM workflows WHERE status = 'active' ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_record).collect()
    }
}

fn status_str(s: WorkflowStatus) -> &'static str {
    match s {
        WorkflowStatus::Draft => "draft",
        WorkflowStatus::Active => "active",
        WorkflowStatus::Archived => "archived",
    }
}

fn parse_status(s: &str) -> WorkflowStatus {
    match s {
        "active" => WorkflowStatus::Active,
        "archived" => WorkflowStatus::Archived,
        _ => WorkflowStatus::Draft,
    }
}

fn exec_status_str(s: ExecutionStatus) -> &'static str {
    match s {
        ExecutionStatus::Pending => "pending",
        ExecutionStatus::Running => "running",
        ExecutionStatus::Completed => "completed",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
    }
}

fn parse_exec_status(s: &str) -> ExecutionStatus {
    match s {
        "pending" => ExecutionStatus::Pending,
        "running" => ExecutionStatus::Running,
        "completed" => ExecutionStatus::Completed,
        "failed" => ExecutionStatus::Failed,
        "cancelled" => ExecutionStatus::Cancelled,
        _ => ExecutionStatus::Pending,
    }
}

pub(crate) fn parse_dt(s: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}

fn row_to_summary(row: WorkflowRow) -> anyhow::Result<WorkflowSummary> {
    Ok(WorkflowSummary {
        id: Uuid::parse_str(&row.id)?,
        name: row.name,
        description: row.description,
        version: row.version as u32,
        status: parse_status(&row.status),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_record(row: WorkflowRow) -> anyhow::Result<WorkflowRecord> {
    Ok(WorkflowRecord {
        id: Uuid::parse_str(&row.id)?,
        name: row.name,
        description: row.description,
        version: row.version as u32,
        status: parse_status(&row.status),
        definition: serde_json::from_str(&row.definition)?,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_execution(row: ExecutionRow) -> anyhow::Result<Execution> {
    Ok(Execution {
        id: Uuid::parse_str(&row.id)?,
        workflow_id: Uuid::parse_str(&row.workflow_id)?,
        workflow_version: row.workflow_version as u32,
        status: parse_exec_status(&row.status),
        trigger_type: row.trigger_type,
        trigger: serde_json::from_str(&row.trigger_data)?,
        started_at: parse_dt(&row.started_at)?,
        finished_at: row.finished_at.as_deref().map(parse_dt).transpose()?,
        error: row.error,
        changelog: Vec::new(),
    })
}

fn row_to_changelog(row: ChangelogRow) -> anyhow::Result<ExecutionChangelog> {
    Ok(ExecutionChangelog {
        execution_id: Uuid::parse_str(&row.execution_id)?,
        workflow_id: Uuid::parse_str(&row.workflow_id)?,
        trigger_type: row.trigger_type,
        trigger: serde_json::from_str(&row.trigger_data).unwrap_or(Value::Null),
        status: parse_exec_status(&row.status),
        error: row.error,
        entries: serde_json::from_str(&row.entries).unwrap_or_default(),
        started_at: parse_dt(&row.started_at)?,
        finished_at: row.finished_at.as_deref().map(parse_dt).transpose()?,
    })
}

fn row_to_schedule(row: ScheduleRow) -> anyhow::Result<WorkflowSchedule> {
    Ok(WorkflowSchedule {
        id: Uuid::parse_str(&row.id)?,
        workflow_id: Uuid::parse_str(&row.workflow_id)?,
        node_id: row.node_id,
        cron: row.cron,
        timezone: row.timezone,
        enabled: row.enabled != 0,
        next_run_at: parse_dt(&row.next_run_at)?,
        last_run_at: row.last_run_at.as_deref().map(parse_dt).transpose()?,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_connection(row: ConnectionRow) -> anyhow::Result<Connection> {
    Ok(Connection {
        id: Uuid::parse_str(&row.id)?,
        name: row.name,
        connection_type: row.connection_type,
        config: serde_json::from_str(&row.config)?,
        enabled: row.enabled != 0,
        has_secret: row.secret_ref.is_some(),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_channel(row: ChannelRow) -> anyhow::Result<Channel> {
    Ok(Channel {
        id: Uuid::parse_str(&row.id)?,
        name: row.name,
        description: row.description,
        kind: ChannelKind::parse(&row.kind)
            .ok_or_else(|| anyhow::anyhow!("invalid channel kind `{}`", row.kind))?,
        enabled: row.enabled != 0,
        config: serde_json::from_str(&row.config)?,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_stream(row: StreamRow) -> anyhow::Result<Stream> {
    Ok(Stream {
        id: Uuid::parse_str(&row.id)?,
        channel_id: Uuid::parse_str(&row.channel_id)?,
        name: row.name,
        description: row.description,
        direction: StreamDirection::parse(&row.direction)
            .ok_or_else(|| anyhow::anyhow!("invalid stream direction `{}`", row.direction))?,
        enabled: row.enabled != 0,
        config: serde_json::from_str(&row.config)?,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_trigger(row: TriggerRow) -> anyhow::Result<Trigger> {
    Ok(Trigger {
        id: Uuid::parse_str(&row.id)?,
        stream_id: Uuid::parse_str(&row.stream_id)?,
        name: row.name,
        description: row.description,
        kind: TriggerKind::parse(&row.kind)
            .ok_or_else(|| anyhow::anyhow!("invalid trigger kind `{}`", row.kind))?,
        enabled: row.enabled != 0,
        workflow_id: row
            .workflow_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?,
        config: serde_json::from_str(&row.config)?,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_node_execution(row: NodeExecutionRow) -> anyhow::Result<NodeExecution> {
    Ok(NodeExecution {
        id: Uuid::parse_str(&row.id)?,
        execution_id: Uuid::parse_str(&row.execution_id)?,
        node_id: row.node_id,
        status: parse_exec_status(&row.status),
        input: serde_json::from_str(&row.input)?,
        output: row
            .output
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        error: row.error,
        started_at: parse_dt(&row.started_at)?,
        finished_at: row.finished_at.as_deref().map(parse_dt).transpose()?,
        duration_ms: row.duration_ms.map(|d| d as u64),
    })
}
