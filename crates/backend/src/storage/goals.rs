//! Goal / GoalRun / experiments / memory / approvals / audit persistence.

use boarddo_shared::v2::{
    AgentApproval, AgentAuditEntry, AgentMemoryEntry, Experiment, ExperimentDecision, GoalRun,
    GoalRunStatus, GoalSpec, PolicyBundle,
};
use boarddo_shared::v2::ApprovalDecision;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use sqlx::FromRow;
use uuid::Uuid;

use super::{Storage, parse_dt};

#[derive(FromRow)]
struct GoalSpecRow {
    id: String,
    title: String,
    text: String,
    metric: String,
    target: f64,
    deadline: Option<String>,
    constraints: String,
    budget: String,
    policy: String,
    agent_type_id: String,
    instructions: String,
    tools: String,
    tick_interval_secs: i64,
    openai_connection_id: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct GoalRunRow {
    id: String,
    goal_id: String,
    status: String,
    current: f64,
    target: f64,
    strategy: String,
    actions_today: i64,
    actions_day: String,
    tick_count: i64,
    last_tick_at: Option<String>,
    next_tick_at: Option<String>,
    last_observe: Option<String>,
    last_think: Option<String>,
    error: Option<String>,
    started_at: String,
    finished_at: Option<String>,
}

#[derive(FromRow)]
struct ExperimentRow {
    id: String,
    run_id: String,
    number: i64,
    hypothesis: String,
    actions: String,
    metrics: String,
    decision: String,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct MemoryRow {
    id: String,
    run_id: String,
    key: String,
    value: String,
    kind: String,
    created_at: String,
}

#[derive(FromRow)]
struct AuditRow {
    id: String,
    run_id: String,
    at: String,
    kind: String,
    message: Option<String>,
    payload: Option<String>,
}

#[derive(FromRow)]
struct ApprovalRow {
    id: String,
    run_id: String,
    goal_id: String,
    title: String,
    description: Option<String>,
    capability: String,
    tool_type_id: String,
    payload: String,
    status: String,
    created_at: String,
    resolved_at: Option<String>,
}

impl Storage {
    pub async fn migrate_goals(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS goal_specs (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL,
                text TEXT NOT NULL,
                metric TEXT NOT NULL,
                target REAL NOT NULL,
                deadline TEXT,
                constraints TEXT NOT NULL DEFAULT '{}',
                budget TEXT NOT NULL DEFAULT '{}',
                policy TEXT NOT NULL DEFAULT '{}',
                agent_type_id TEXT NOT NULL,
                instructions TEXT NOT NULL DEFAULT '',
                tools TEXT NOT NULL DEFAULT '[]',
                tick_interval_secs INTEGER NOT NULL DEFAULT 60,
                openai_connection_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS goal_runs (
                id TEXT PRIMARY KEY NOT NULL,
                goal_id TEXT NOT NULL,
                status TEXT NOT NULL,
                current REAL NOT NULL DEFAULT 0,
                target REAL NOT NULL,
                strategy TEXT NOT NULL DEFAULT '{}',
                actions_today INTEGER NOT NULL DEFAULT 0,
                actions_day TEXT NOT NULL,
                tick_count INTEGER NOT NULL DEFAULT 0,
                last_tick_at TEXT,
                next_tick_at TEXT,
                last_observe TEXT,
                last_think TEXT,
                error TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                FOREIGN KEY(goal_id) REFERENCES goal_specs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS experiments (
                id TEXT PRIMARY KEY NOT NULL,
                run_id TEXT NOT NULL,
                number INTEGER NOT NULL,
                hypothesis TEXT NOT NULL,
                actions TEXT NOT NULL DEFAULT '{}',
                metrics TEXT NOT NULL DEFAULT '{}',
                decision TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS agent_memory (
                id TEXT PRIMARY KEY NOT NULL,
                run_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'short',
                created_at TEXT NOT NULL,
                UNIQUE(run_id, key),
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS agent_audit (
                id TEXT PRIMARY KEY NOT NULL,
                run_id TEXT NOT NULL,
                at TEXT NOT NULL,
                kind TEXT NOT NULL,
                message TEXT,
                payload TEXT,
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS agent_approvals (
                id TEXT PRIMARY KEY NOT NULL,
                run_id TEXT NOT NULL,
                goal_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                capability TEXT NOT NULL,
                tool_type_id TEXT NOT NULL,
                payload TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                resolved_at TEXT,
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS goal_run_workflows (
                run_id TEXT NOT NULL,
                workflow_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (run_id, workflow_id),
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_goal_spec(&self, spec: &GoalSpec) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO goal_specs (
                id, title, text, metric, target, deadline, constraints, budget, policy,
                agent_type_id, instructions, tools, tick_interval_secs, openai_connection_id,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(spec.id.to_string())
        .bind(&spec.title)
        .bind(&spec.text)
        .bind(&spec.metric)
        .bind(spec.target)
        .bind(spec.deadline.map(|d| d.to_rfc3339()))
        .bind(serde_json::to_string(&spec.constraints)?)
        .bind(serde_json::to_string(&spec.budget)?)
        .bind(serde_json::to_string(&spec.policy)?)
        .bind(&spec.agent_type_id)
        .bind(&spec.instructions)
        .bind(serde_json::to_string(&spec.tools)?)
        .bind(spec.tick_interval_secs as i64)
        .bind(spec.openai_connection_id.map(|id| id.to_string()))
        .bind(spec.created_at.to_rfc3339())
        .bind(spec.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_goal_spec(&self, spec: &GoalSpec) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE goal_specs SET
                title = ?, text = ?, metric = ?, target = ?, deadline = ?,
                constraints = ?, budget = ?, policy = ?, agent_type_id = ?,
                instructions = ?, tools = ?, tick_interval_secs = ?,
                openai_connection_id = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&spec.title)
        .bind(&spec.text)
        .bind(&spec.metric)
        .bind(spec.target)
        .bind(spec.deadline.map(|d| d.to_rfc3339()))
        .bind(serde_json::to_string(&spec.constraints)?)
        .bind(serde_json::to_string(&spec.budget)?)
        .bind(serde_json::to_string(&spec.policy)?)
        .bind(&spec.agent_type_id)
        .bind(&spec.instructions)
        .bind(serde_json::to_string(&spec.tools)?)
        .bind(spec.tick_interval_secs as i64)
        .bind(spec.openai_connection_id.map(|id| id.to_string()))
        .bind(spec.updated_at.to_rfc3339())
        .bind(spec.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_goal_spec(&self, id: Uuid) -> anyhow::Result<bool> {
        let res = sqlx::query("DELETE FROM goal_specs WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn get_goal_spec(&self, id: Uuid) -> anyhow::Result<Option<GoalSpec>> {
        let row: Option<GoalSpecRow> = sqlx::query_as("SELECT * FROM goal_specs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_spec).transpose()
    }

    pub async fn list_goal_specs(&self) -> anyhow::Result<Vec<GoalSpec>> {
        let rows: Vec<GoalSpecRow> =
            sqlx::query_as("SELECT * FROM goal_specs ORDER BY updated_at DESC")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter().map(row_to_spec).collect()
    }

    pub async fn insert_goal_run(&self, run: &GoalRun) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO goal_runs (
                id, goal_id, status, current, target, strategy, actions_today, actions_day,
                tick_count, last_tick_at, next_tick_at, last_observe, last_think, error,
                started_at, finished_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(run.id.to_string())
        .bind(run.goal_id.to_string())
        .bind(run.status.as_str())
        .bind(run.current)
        .bind(run.target)
        .bind(serde_json::to_string(&run.strategy)?)
        .bind(run.actions_today as i64)
        .bind(&run.actions_day)
        .bind(run.tick_count as i64)
        .bind(run.last_tick_at.map(|d| d.to_rfc3339()))
        .bind(run.next_tick_at.map(|d| d.to_rfc3339()))
        .bind(&run.last_observe)
        .bind(&run.last_think)
        .bind(&run.error)
        .bind(run.started_at.to_rfc3339())
        .bind(run.finished_at.map(|d| d.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_goal_run(&self, run: &GoalRun) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE goal_runs SET
                status = ?, current = ?, target = ?, strategy = ?, actions_today = ?,
                actions_day = ?, tick_count = ?, last_tick_at = ?, next_tick_at = ?,
                last_observe = ?, last_think = ?, error = ?, finished_at = ?
            WHERE id = ?
            "#,
        )
        .bind(run.status.as_str())
        .bind(run.current)
        .bind(run.target)
        .bind(serde_json::to_string(&run.strategy)?)
        .bind(run.actions_today as i64)
        .bind(&run.actions_day)
        .bind(run.tick_count as i64)
        .bind(run.last_tick_at.map(|d| d.to_rfc3339()))
        .bind(run.next_tick_at.map(|d| d.to_rfc3339()))
        .bind(&run.last_observe)
        .bind(&run.last_think)
        .bind(&run.error)
        .bind(run.finished_at.map(|d| d.to_rfc3339()))
        .bind(run.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_goal_run(&self, id: Uuid) -> anyhow::Result<Option<GoalRun>> {
        let row: Option<GoalRunRow> = sqlx::query_as("SELECT * FROM goal_runs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_run).transpose()
    }

    pub async fn latest_run_for_goal(&self, goal_id: Uuid) -> anyhow::Result<Option<GoalRun>> {
        let row: Option<GoalRunRow> = sqlx::query_as(
            "SELECT * FROM goal_runs WHERE goal_id = ? ORDER BY started_at DESC LIMIT 1",
        )
        .bind(goal_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_run).transpose()
    }

    pub async fn list_goal_runs(&self) -> anyhow::Result<Vec<GoalRun>> {
        let rows: Vec<GoalRunRow> =
            sqlx::query_as("SELECT * FROM goal_runs ORDER BY started_at DESC")
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter().map(row_to_run).collect()
    }

    pub async fn list_due_goal_runs(&self, now: DateTime<Utc>) -> anyhow::Result<Vec<GoalRun>> {
        let rows: Vec<GoalRunRow> = sqlx::query_as(
            "SELECT * FROM goal_runs WHERE status IN ('running') AND (next_tick_at IS NULL OR next_tick_at <= ?)",
        )
        .bind(now.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_run).collect()
    }

    pub async fn insert_experiment(&self, exp: &Experiment) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO experiments (
                id, run_id, number, hypothesis, actions, metrics, decision, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(exp.id.to_string())
        .bind(exp.run_id.to_string())
        .bind(exp.number as i64)
        .bind(&exp.hypothesis)
        .bind(serde_json::to_string(&exp.actions)?)
        .bind(serde_json::to_string(&exp.metrics)?)
        .bind(exp.decision.as_str())
        .bind(exp.created_at.to_rfc3339())
        .bind(exp.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_experiment(&self, exp: &Experiment) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE experiments SET actions = ?, metrics = ?, decision = ?, updated_at = ? WHERE id = ?",
        )
        .bind(serde_json::to_string(&exp.actions)?)
        .bind(serde_json::to_string(&exp.metrics)?)
        .bind(exp.decision.as_str())
        .bind(exp.updated_at.to_rfc3339())
        .bind(exp.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_experiments(&self, run_id: Uuid) -> anyhow::Result<Vec<Experiment>> {
        let rows: Vec<ExperimentRow> = sqlx::query_as(
            "SELECT * FROM experiments WHERE run_id = ? ORDER BY number ASC",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_experiment).collect()
    }

    pub async fn next_experiment_number(&self, run_id: Uuid) -> anyhow::Result<u32> {
        let n: (i64,) =
            sqlx::query_as("SELECT COALESCE(MAX(number), 0) FROM experiments WHERE run_id = ?")
                .bind(run_id.to_string())
                .fetch_one(&self.pool)
                .await?;
        Ok((n.0 as u32) + 1)
    }

    pub async fn upsert_memory(
        &self,
        run_id: Uuid,
        key: &str,
        value: &Value,
        kind: &str,
    ) -> anyhow::Result<AgentMemoryEntry> {
        let now = Utc::now();
        let existing: Option<(String,)> =
            sqlx::query_as("SELECT id FROM agent_memory WHERE run_id = ? AND key = ?")
                .bind(run_id.to_string())
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        let id = if let Some((id,)) = existing {
            sqlx::query("UPDATE agent_memory SET value = ?, kind = ?, created_at = ? WHERE id = ?")
                .bind(serde_json::to_string(value)?)
                .bind(kind)
                .bind(now.to_rfc3339())
                .bind(&id)
                .execute(&self.pool)
                .await?;
            Uuid::parse_str(&id)?
        } else {
            let id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO agent_memory (id, run_id, key, value, kind, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(id.to_string())
            .bind(run_id.to_string())
            .bind(key)
            .bind(serde_json::to_string(value)?)
            .bind(kind)
            .bind(now.to_rfc3339())
            .execute(&self.pool)
            .await?;
            id
        };
        Ok(AgentMemoryEntry {
            id,
            run_id,
            key: key.into(),
            value: value.clone(),
            kind: kind.into(),
            created_at: now,
        })
    }

    pub async fn get_memory(&self, run_id: Uuid, key: &str) -> anyhow::Result<Option<AgentMemoryEntry>> {
        let row: Option<MemoryRow> =
            sqlx::query_as("SELECT * FROM agent_memory WHERE run_id = ? AND key = ?")
                .bind(run_id.to_string())
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        row.map(row_to_memory).transpose()
    }

    pub async fn list_memory(&self, run_id: Uuid) -> anyhow::Result<Vec<AgentMemoryEntry>> {
        let rows: Vec<MemoryRow> = sqlx::query_as(
            "SELECT * FROM agent_memory WHERE run_id = ? ORDER BY created_at DESC",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_memory).collect()
    }

    pub async fn insert_audit(&self, entry: &AgentAuditEntry) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO agent_audit (id, run_id, at, kind, message, payload) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.id.to_string())
        .bind(entry.run_id.to_string())
        .bind(entry.at.to_rfc3339())
        .bind(&entry.kind)
        .bind(&entry.message)
        .bind(
            entry
                .payload
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_audit(
        &self,
        run_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<AgentAuditEntry>> {
        let rows: Vec<AuditRow> = sqlx::query_as(
            "SELECT * FROM agent_audit WHERE run_id = ? ORDER BY at DESC LIMIT ?",
        )
        .bind(run_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_audit).collect()
    }

    pub async fn insert_approval(&self, a: &AgentApproval) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO agent_approvals (
                id, run_id, goal_id, title, description, capability, tool_type_id,
                payload, status, created_at, resolved_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(a.id.to_string())
        .bind(a.run_id.to_string())
        .bind(a.goal_id.to_string())
        .bind(&a.title)
        .bind(&a.description)
        .bind(&a.capability)
        .bind(&a.tool_type_id)
        .bind(serde_json::to_string(&a.payload)?)
        .bind(approval_status(&a.status))
        .bind(a.created_at.to_rfc3339())
        .bind(a.resolved_at.map(|d| d.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_approval(&self, id: Uuid) -> anyhow::Result<Option<AgentApproval>> {
        let row: Option<ApprovalRow> =
            sqlx::query_as("SELECT * FROM agent_approvals WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        row.map(row_to_approval).transpose()
    }

    pub async fn update_approval(&self, a: &AgentApproval) -> anyhow::Result<()> {
        sqlx::query("UPDATE agent_approvals SET status = ?, resolved_at = ? WHERE id = ?")
            .bind(approval_status(&a.status))
            .bind(a.resolved_at.map(|d| d.to_rfc3339()))
            .bind(a.id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_approvals(
        &self,
        run_id: Option<Uuid>,
        pending_only: bool,
    ) -> anyhow::Result<Vec<AgentApproval>> {
        let rows: Vec<ApprovalRow> = if let Some(run_id) = run_id {
            if pending_only {
                sqlx::query_as(
                    "SELECT * FROM agent_approvals WHERE run_id = ? AND status = 'pending' ORDER BY created_at DESC",
                )
                .bind(run_id.to_string())
                .fetch_all(&self.pool)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT * FROM agent_approvals WHERE run_id = ? ORDER BY created_at DESC",
                )
                .bind(run_id.to_string())
                .fetch_all(&self.pool)
                .await?
            }
        } else if pending_only {
            sqlx::query_as(
                "SELECT * FROM agent_approvals WHERE status = 'pending' ORDER BY created_at DESC",
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as("SELECT * FROM agent_approvals ORDER BY created_at DESC LIMIT 100")
                .fetch_all(&self.pool)
                .await?
        };
        rows.into_iter().map(row_to_approval).collect()
    }

    pub async fn count_pending_approvals(&self, run_id: Uuid) -> anyhow::Result<i64> {
        let n: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_approvals WHERE run_id = ? AND status = 'pending'",
        )
        .bind(run_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(n.0)
    }

    pub async fn link_goal_run_workflow(
        &self,
        run_id: Uuid,
        workflow_id: Uuid,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO goal_run_workflows (run_id, workflow_id, created_at) VALUES (?, ?, ?)",
        )
        .bind(run_id.to_string())
        .bind(workflow_id.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_goal_run_workflow_ids(&self, run_id: Uuid) -> anyhow::Result<Vec<Uuid>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT workflow_id FROM goal_run_workflows WHERE run_id = ?")
                .bind(run_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(id,)| Uuid::parse_str(&id).ok())
            .collect())
    }
}

fn row_to_spec(row: GoalSpecRow) -> anyhow::Result<GoalSpec> {
    Ok(GoalSpec {
        id: Uuid::parse_str(&row.id)?,
        title: row.title,
        text: row.text,
        metric: row.metric,
        target: row.target,
        deadline: row.deadline.as_deref().map(parse_dt).transpose()?,
        constraints: serde_json::from_str(&row.constraints).unwrap_or(Value::Object(Map::new())),
        budget: serde_json::from_str(&row.budget).unwrap_or_default(),
        policy: serde_json::from_str(&row.policy).unwrap_or_default(),
        agent_type_id: row.agent_type_id,
        instructions: row.instructions,
        tools: serde_json::from_str(&row.tools).unwrap_or_default(),
        tick_interval_secs: row.tick_interval_secs.max(5) as u64,
        openai_connection_id: row
            .openai_connection_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok()),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_run(row: GoalRunRow) -> anyhow::Result<GoalRun> {
    Ok(GoalRun {
        id: Uuid::parse_str(&row.id)?,
        goal_id: Uuid::parse_str(&row.goal_id)?,
        status: GoalRunStatus::parse(&row.status),
        current: row.current,
        target: row.target,
        strategy: serde_json::from_str(&row.strategy).unwrap_or_default(),
        actions_today: row.actions_today.max(0) as u32,
        actions_day: row.actions_day,
        tick_count: row.tick_count.max(0) as u32,
        last_tick_at: row.last_tick_at.as_deref().map(parse_dt).transpose()?,
        next_tick_at: row.next_tick_at.as_deref().map(parse_dt).transpose()?,
        last_observe: row.last_observe,
        last_think: row.last_think,
        error: row.error,
        started_at: parse_dt(&row.started_at)?,
        finished_at: row.finished_at.as_deref().map(parse_dt).transpose()?,
    })
}

fn row_to_experiment(row: ExperimentRow) -> anyhow::Result<Experiment> {
    Ok(Experiment {
        id: Uuid::parse_str(&row.id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        number: row.number.max(0) as u32,
        hypothesis: row.hypothesis,
        actions: serde_json::from_str(&row.actions).unwrap_or(json_obj()),
        metrics: serde_json::from_str(&row.metrics).unwrap_or(json_obj()),
        decision: ExperimentDecision::parse(&row.decision),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_memory(row: MemoryRow) -> anyhow::Result<AgentMemoryEntry> {
    Ok(AgentMemoryEntry {
        id: Uuid::parse_str(&row.id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        key: row.key,
        value: serde_json::from_str(&row.value).unwrap_or(Value::Null),
        kind: row.kind,
        created_at: parse_dt(&row.created_at)?,
    })
}

fn row_to_audit(row: AuditRow) -> anyhow::Result<AgentAuditEntry> {
    Ok(AgentAuditEntry {
        id: Uuid::parse_str(&row.id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        at: parse_dt(&row.at)?,
        kind: row.kind,
        message: row.message,
        payload: row
            .payload
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
    })
}

fn row_to_approval(row: ApprovalRow) -> anyhow::Result<AgentApproval> {
    Ok(AgentApproval {
        id: Uuid::parse_str(&row.id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        goal_id: Uuid::parse_str(&row.goal_id)?,
        title: row.title,
        description: row.description,
        capability: row.capability,
        tool_type_id: row.tool_type_id,
        payload: serde_json::from_str(&row.payload).unwrap_or(json_obj()),
        status: parse_approval(&row.status),
        created_at: parse_dt(&row.created_at)?,
        resolved_at: row.resolved_at.as_deref().map(parse_dt).transpose()?,
    })
}

fn json_obj() -> Value {
    Value::Object(Map::new())
}

fn approval_status(d: &ApprovalDecision) -> &'static str {
    match d {
        ApprovalDecision::Pending => "pending",
        ApprovalDecision::Approved => "approved",
        ApprovalDecision::Rejected => "rejected",
        ApprovalDecision::TimedOut => "timed_out",
        ApprovalDecision::Edited => "edited",
    }
}

fn parse_approval(s: &str) -> ApprovalDecision {
    match s {
        "approved" => ApprovalDecision::Approved,
        "rejected" => ApprovalDecision::Rejected,
        "timed_out" => ApprovalDecision::TimedOut,
        "edited" => ApprovalDecision::Edited,
        _ => ApprovalDecision::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::{CapabilityBudget, CreateGoalRequest};
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn goal_spec_and_run_roundtrip() {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let storage = Storage::from_pool(pool);
        storage.migrate_goals().await.unwrap();

        let req = CreateGoalRequest {
            title: "Growth".into(),
            text: "10 leads".into(),
            metric: "leads_interested".into(),
            target: 10.0,
            deadline: None,
            constraints: serde_json::json!({"product": "BoardDo"}),
            budget: CapabilityBudget::default(),
            policy: PolicyBundle::default(),
            agent_type_id: "agent.growth".into(),
            instructions: String::new(),
            tools: vec![],
            tick_interval_secs: Some(30),
            openai_connection_id: None,
            template_id: Some("growth".into()),
        };
        let spec = crate::agent_runtime::spec_from_request(req);
        storage.insert_goal_spec(&spec).await.unwrap();
        let loaded = storage.get_goal_spec(spec.id).await.unwrap().unwrap();
        assert_eq!(loaded.agent_type_id, "agent.growth");
        assert!(!loaded.tools.is_empty());

        let run = GoalRun {
            id: Uuid::now_v7(),
            goal_id: spec.id,
            status: GoalRunStatus::Running,
            current: 0.0,
            target: spec.target,
            strategy: Default::default(),
            actions_today: 0,
            actions_day: "2026-09-16".into(),
            tick_count: 0,
            last_tick_at: None,
            next_tick_at: Some(Utc::now()),
            last_observe: None,
            last_think: None,
            error: None,
            started_at: Utc::now(),
            finished_at: None,
        };
        storage.insert_goal_run(&run).await.unwrap();
        let due = storage.list_due_goal_runs(Utc::now()).await.unwrap();
        assert_eq!(due.len(), 1);
    }
}
