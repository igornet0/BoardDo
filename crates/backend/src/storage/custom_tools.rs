//! Custom Python tools — registry persistence.

use boarddo_shared::v2::{
    CustomTool, CustomToolVersion, ToolExecutionRecord, ToolOrigin, ToolVersionStatus,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use super::{Storage, parse_dt};

#[derive(FromRow)]
struct ToolRow {
    id: String,
    name: String,
    description: String,
    language: String,
    origin: String,
    created_by: String,
    active_version_id: Option<String>,
    tags: String,
    execution_stats: String,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct VersionRow {
    id: String,
    tool_id: String,
    version: i64,
    source_code: String,
    input_schema: String,
    output_schema: String,
    permissions: String,
    requirements: String,
    runtime_config: String,
    status: String,
    purpose: String,
    specification: String,
    test_cases: String,
    repair_history: String,
    created_at: String,
}

#[derive(FromRow)]
struct ExecRow {
    id: String,
    tool_id: String,
    version: i64,
    status: String,
    duration_ms: i64,
    output: Option<String>,
    error_type: Option<String>,
    message: Option<String>,
    traceback: Option<String>,
    logs: String,
    resource_usage: String,
    created_at: String,
}

impl Storage {
    pub async fn migrate_custom_tools(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS custom_tools (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL UNIQUE,
                description TEXT NOT NULL DEFAULT '',
                language TEXT NOT NULL DEFAULT 'python',
                origin TEXT NOT NULL DEFAULT 'AGENT',
                created_by TEXT NOT NULL DEFAULT 'system',
                active_version_id TEXT,
                tags TEXT NOT NULL DEFAULT '[]',
                execution_stats TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS custom_tool_versions (
                id TEXT PRIMARY KEY NOT NULL,
                tool_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                source_code TEXT NOT NULL,
                input_schema TEXT NOT NULL DEFAULT '{}',
                output_schema TEXT NOT NULL DEFAULT '{}',
                permissions TEXT NOT NULL DEFAULT '{}',
                requirements TEXT NOT NULL DEFAULT '[]',
                runtime_config TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'DRAFT',
                purpose TEXT NOT NULL DEFAULT '',
                specification TEXT NOT NULL DEFAULT '{}',
                test_cases TEXT NOT NULL DEFAULT '[]',
                repair_history TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                UNIQUE(tool_id, version),
                FOREIGN KEY(tool_id) REFERENCES custom_tools(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS custom_tool_executions (
                id TEXT PRIMARY KEY NOT NULL,
                tool_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                status TEXT NOT NULL,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                output TEXT,
                error_type TEXT,
                message TEXT,
                traceback TEXT,
                logs TEXT NOT NULL DEFAULT '',
                resource_usage TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                FOREIGN KEY(tool_id) REFERENCES custom_tools(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_custom_tool_versions_tool
                ON custom_tool_versions(tool_id, version DESC);
            CREATE INDEX IF NOT EXISTS idx_custom_tool_exec_tool
                ON custom_tool_executions(tool_id, created_at DESC);
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_custom_tools(&self) -> anyhow::Result<Vec<CustomTool>> {
        let rows: Vec<ToolRow> = sqlx::query_as(
            "SELECT id, name, description, language, origin, created_by, active_version_id,
                    tags, execution_stats, created_at, updated_at
             FROM custom_tools ORDER BY updated_at DESC",
        )
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(row_to_tool).collect()
    }

    pub async fn find_custom_tools(&self, query: &str, limit: u32) -> anyhow::Result<Vec<CustomTool>> {
        let pattern = format!("%{query}%");
        let rows: Vec<ToolRow> = sqlx::query_as(
            "SELECT id, name, description, language, origin, created_by, active_version_id,
                    tags, execution_stats, created_at, updated_at
             FROM custom_tools
             WHERE name LIKE ?1 OR description LIKE ?1 OR tags LIKE ?1
             ORDER BY updated_at DESC LIMIT ?2",
        )
        .bind(&pattern)
        .bind(limit as i64)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(row_to_tool).collect()
    }

    pub async fn get_custom_tool(&self, id: Uuid) -> anyhow::Result<Option<CustomTool>> {
        let row: Option<ToolRow> = sqlx::query_as(
            "SELECT id, name, description, language, origin, created_by, active_version_id,
                    tags, execution_stats, created_at, updated_at
             FROM custom_tools WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(self.pool())
        .await?;
        row.map(row_to_tool).transpose()
    }

    pub async fn get_custom_tool_by_name(&self, name: &str) -> anyhow::Result<Option<CustomTool>> {
        let row: Option<ToolRow> = sqlx::query_as(
            "SELECT id, name, description, language, origin, created_by, active_version_id,
                    tags, execution_stats, created_at, updated_at
             FROM custom_tools WHERE name = ?1",
        )
        .bind(name)
        .fetch_optional(self.pool())
        .await?;
        row.map(row_to_tool).transpose()
    }

    pub async fn insert_custom_tool(&self, tool: &CustomTool) -> anyhow::Result<()> {
        sqlx::query(
            r#"INSERT INTO custom_tools
               (id, name, description, language, origin, created_by, active_version_id,
                tags, execution_stats, created_at, updated_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"#,
        )
        .bind(tool.id.to_string())
        .bind(&tool.name)
        .bind(&tool.description)
        .bind(tool.language.as_str())
        .bind(tool.origin.as_str())
        .bind(&tool.created_by)
        .bind(tool.active_version_id.map(|u| u.to_string()))
        .bind(serde_json::to_string(&tool.tags)?)
        .bind(serde_json::to_string(&tool.execution_stats)?)
        .bind(tool.created_at.to_rfc3339())
        .bind(tool.updated_at.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn update_custom_tool_meta(&self, tool: &CustomTool) -> anyhow::Result<()> {
        sqlx::query(
            r#"UPDATE custom_tools SET
               description = ?2, active_version_id = ?3, tags = ?4,
               execution_stats = ?5, updated_at = ?6
               WHERE id = ?1"#,
        )
        .bind(tool.id.to_string())
        .bind(&tool.description)
        .bind(tool.active_version_id.map(|u| u.to_string()))
        .bind(serde_json::to_string(&tool.tags)?)
        .bind(serde_json::to_string(&tool.execution_stats)?)
        .bind(tool.updated_at.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn delete_custom_tool(&self, id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query("DELETE FROM custom_tools WHERE id = ?1")
            .bind(id.to_string())
            .execute(self.pool())
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn insert_custom_tool_version(&self, ver: &CustomToolVersion) -> anyhow::Result<()> {
        sqlx::query(
            r#"INSERT INTO custom_tool_versions
               (id, tool_id, version, source_code, input_schema, output_schema, permissions,
                requirements, runtime_config, status, purpose, specification, test_cases,
                repair_history, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"#,
        )
        .bind(ver.id.to_string())
        .bind(ver.tool_id.to_string())
        .bind(ver.version as i64)
        .bind(&ver.source_code)
        .bind(ver.input_schema.to_string())
        .bind(ver.output_schema.to_string())
        .bind(serde_json::to_string(&ver.permissions)?)
        .bind(serde_json::to_string(&ver.requirements)?)
        .bind(serde_json::to_string(&ver.runtime_config)?)
        .bind(ver.status.as_str())
        .bind(&ver.purpose)
        .bind(ver.specification.to_string())
        .bind(serde_json::to_string(&ver.test_cases)?)
        .bind(serde_json::to_string(&ver.repair_history)?)
        .bind(ver.created_at.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn update_custom_tool_version(&self, ver: &CustomToolVersion) -> anyhow::Result<()> {
        sqlx::query(
            r#"UPDATE custom_tool_versions SET
               source_code = ?3, status = ?4, repair_history = ?5
               WHERE tool_id = ?1 AND version = ?2"#,
        )
        .bind(ver.tool_id.to_string())
        .bind(ver.version as i64)
        .bind(&ver.source_code)
        .bind(ver.status.as_str())
        .bind(serde_json::to_string(&ver.repair_history)?)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_custom_tool_versions(
        &self,
        tool_id: Uuid,
    ) -> anyhow::Result<Vec<CustomToolVersion>> {
        let rows: Vec<VersionRow> = sqlx::query_as(
            "SELECT id, tool_id, version, source_code, input_schema, output_schema, permissions,
                    requirements, runtime_config, status, purpose, specification, test_cases,
                    repair_history, created_at
             FROM custom_tool_versions WHERE tool_id = ?1 ORDER BY version DESC",
        )
        .bind(tool_id.to_string())
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(row_to_version).collect()
    }

    pub async fn get_custom_tool_version(
        &self,
        tool_id: Uuid,
        version: u32,
    ) -> anyhow::Result<Option<CustomToolVersion>> {
        let row: Option<VersionRow> = sqlx::query_as(
            "SELECT id, tool_id, version, source_code, input_schema, output_schema, permissions,
                    requirements, runtime_config, status, purpose, specification, test_cases,
                    repair_history, created_at
             FROM custom_tool_versions WHERE tool_id = ?1 AND version = ?2",
        )
        .bind(tool_id.to_string())
        .bind(version as i64)
        .fetch_optional(self.pool())
        .await?;
        row.map(row_to_version).transpose()
    }

    pub async fn get_active_custom_tool_version(
        &self,
        tool: &CustomTool,
    ) -> anyhow::Result<Option<CustomToolVersion>> {
        let Some(vid) = tool.active_version_id else {
            return Ok(None);
        };
        let row: Option<VersionRow> = sqlx::query_as(
            "SELECT id, tool_id, version, source_code, input_schema, output_schema, permissions,
                    requirements, runtime_config, status, purpose, specification, test_cases,
                    repair_history, created_at
             FROM custom_tool_versions WHERE id = ?1",
        )
        .bind(vid.to_string())
        .fetch_optional(self.pool())
        .await?;
        row.map(row_to_version).transpose()
    }

    pub async fn next_custom_tool_version(&self, tool_id: Uuid) -> anyhow::Result<u32> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(version), 0) FROM custom_tool_versions WHERE tool_id = ?1",
        )
        .bind(tool_id.to_string())
        .fetch_one(self.pool())
        .await?;
        Ok((row.0 as u32) + 1)
    }

    pub async fn demote_active_versions(&self, tool_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE custom_tool_versions SET status = 'ARCHIVED'
             WHERE tool_id = ?1 AND status = 'ACTIVE'",
        )
        .bind(tool_id.to_string())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn insert_custom_tool_execution(
        &self,
        rec: &ToolExecutionRecord,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"INSERT INTO custom_tool_executions
               (id, tool_id, version, status, duration_ms, output, error_type, message,
                traceback, logs, resource_usage, created_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"#,
        )
        .bind(rec.id.to_string())
        .bind(rec.tool_id.to_string())
        .bind(rec.version as i64)
        .bind(&rec.status)
        .bind(rec.duration_ms as i64)
        .bind(rec.output.as_ref().map(|v| v.to_string()))
        .bind(&rec.error_type)
        .bind(&rec.message)
        .bind(&rec.traceback)
        .bind(&rec.logs)
        .bind(serde_json::to_string(&rec.resource_usage)?)
        .bind(rec.created_at.to_rfc3339())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_custom_tool_executions(
        &self,
        tool_id: Uuid,
        limit: u32,
    ) -> anyhow::Result<Vec<ToolExecutionRecord>> {
        let rows: Vec<ExecRow> = sqlx::query_as(
            "SELECT id, tool_id, version, status, duration_ms, output, error_type, message,
                    traceback, logs, resource_usage, created_at
             FROM custom_tool_executions WHERE tool_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )
        .bind(tool_id.to_string())
        .bind(limit as i64)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter().map(row_to_exec).collect()
    }

    pub async fn record_custom_tool_stats(
        &self,
        tool_id: Uuid,
        success: bool,
        duration_ms: u64,
        last_error: Option<String>,
    ) -> anyhow::Result<()> {
        let Some(mut tool) = self.get_custom_tool(tool_id).await? else {
            return Ok(());
        };
        let stats = &mut tool.execution_stats;
        stats.execution_count += 1;
        if success {
            stats.success_count += 1;
            stats.last_success_at = Some(Utc::now());
        } else {
            stats.failure_count += 1;
            stats.last_error = last_error;
        }
        let n = stats.execution_count;
        stats.average_duration_ms =
            ((stats.average_duration_ms * (n - 1)) + duration_ms) / n.max(1);
        tool.updated_at = Utc::now();
        self.update_custom_tool_meta(&tool).await
    }
}

fn row_to_tool(row: ToolRow) -> anyhow::Result<CustomTool> {
    use boarddo_shared::v2::ToolLanguage;
    Ok(CustomTool {
        id: Uuid::parse_str(&row.id)?,
        name: row.name,
        description: row.description,
        language: ToolLanguage::parse(&row.language),
        origin: ToolOrigin::parse(&row.origin),
        created_by: row.created_by,
        active_version_id: row
            .active_version_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?,
        tags: serde_json::from_str(&row.tags).unwrap_or_default(),
        execution_stats: serde_json::from_str(&row.execution_stats).unwrap_or_default(),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_version(row: VersionRow) -> anyhow::Result<CustomToolVersion> {
    use boarddo_shared::v2::{ToolPermissions, ToolRuntimeConfig, ToolVersionStatus};
    Ok(CustomToolVersion {
        id: Uuid::parse_str(&row.id)?,
        tool_id: Uuid::parse_str(&row.tool_id)?,
        version: row.version as u32,
        source_code: row.source_code,
        input_schema: serde_json::from_str(&row.input_schema).unwrap_or(json!({})),
        output_schema: serde_json::from_str(&row.output_schema).unwrap_or(json!({})),
        permissions: serde_json::from_str(&row.permissions).unwrap_or_default(),
        requirements: serde_json::from_str(&row.requirements).unwrap_or_default(),
        runtime_config: serde_json::from_str(&row.runtime_config).unwrap_or_default(),
        status: ToolVersionStatus::parse(&row.status),
        purpose: row.purpose,
        specification: serde_json::from_str(&row.specification).unwrap_or(json!({})),
        test_cases: serde_json::from_str(&row.test_cases).unwrap_or_default(),
        repair_history: serde_json::from_str(&row.repair_history).unwrap_or_default(),
        created_at: parse_dt(&row.created_at)?,
    })
}

fn row_to_exec(row: ExecRow) -> anyhow::Result<ToolExecutionRecord> {
    Ok(ToolExecutionRecord {
        id: Uuid::parse_str(&row.id)?,
        tool_id: Uuid::parse_str(&row.tool_id)?,
        version: row.version as u32,
        status: row.status,
        duration_ms: row.duration_ms as u64,
        output: row
            .output
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        error_type: row.error_type,
        message: row.message,
        traceback: row.traceback,
        logs: row.logs,
        resource_usage: serde_json::from_str(&row.resource_usage).unwrap_or_default(),
        created_at: parse_dt(&row.created_at)?,
    })
}
