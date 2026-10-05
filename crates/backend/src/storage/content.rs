//! Content Engine persistence (projects, graph, versions, publications).

use boarddo_shared::v2::{
    AudienceProfile, BrandProfile, BrandVoice, ContentBrief, ContentCalendarEntry,
    ContentChannelId, ContentEngineSnapshot, ContentFormatType, ContentGraphEdge,
    ContentIdea, ContentInsight, ContentItem, ContentLifecycle, ContentObjective,
    ContentProject, ContentPublication, ContentSourceRef, ContentStrategy, ContentVersion,
    CtaType, item_from_legacy_draft, ContentDraft,
};
use chrono::Utc;
use serde_json::{json, Map, Value};
use sqlx::FromRow;
use uuid::Uuid;

use super::{Storage, parse_dt};

#[derive(FromRow)]
struct ProjectRow {
    id: String,
    goal_id: String,
    run_id: Option<String>,
    name: String,
    brand_id: Option<String>,
    channels: String,
    metadata: String,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct BrandRow {
    id: String,
    project_id: String,
    name: String,
    payload: String,
    updated_at: String,
}

#[derive(FromRow)]
struct AudienceRow {
    id: String,
    project_id: String,
    payload: String,
}

#[derive(FromRow)]
struct ItemRow {
    id: String,
    project_id: String,
    goal_id: String,
    run_id: String,
    parent_content_id: Option<String>,
    idea_id: Option<String>,
    brief_id: Option<String>,
    audience_id: Option<String>,
    pillar_id: Option<String>,
    experiment_id: Option<String>,
    title: String,
    body: String,
    format: String,
    channel: String,
    objective: String,
    lifecycle: String,
    cta: Option<String>,
    explainability: String,
    quality_gates: String,
    scheduled_at: Option<String>,
    legacy_draft_id: Option<String>,
    metadata: String,
    created_at: String,
    updated_at: String,
}

#[derive(FromRow)]
struct VersionRow {
    id: String,
    content_id: String,
    version: i64,
    stage: String,
    body: String,
    created_by: String,
    created_at: String,
    note: Option<String>,
}

#[derive(FromRow)]
struct IdeaRow {
    id: String,
    project_id: String,
    goal_id: String,
    run_id: String,
    payload: String,
    created_at: String,
}

#[derive(FromRow)]
struct BriefRow {
    id: String,
    payload: String,
    created_at: String,
}

#[derive(FromRow)]
struct StrategyRow {
    id: String,
    project_id: String,
    goal_id: String,
    payload: String,
    updated_at: String,
}

#[derive(FromRow)]
struct PublicationRow {
    id: String,
    content_id: String,
    channel: String,
    account_id: Option<String>,
    external_id: Option<String>,
    published_at: String,
    status: String,
    simulated: i64,
    metrics: String,
    idempotency_key: String,
}

#[derive(FromRow)]
struct InsightRow {
    id: String,
    project_id: String,
    run_id: String,
    payload: String,
    created_at: String,
}

#[derive(FromRow)]
struct CalendarRow {
    id: String,
    project_id: String,
    content_id: String,
    payload: String,
}

impl Storage {
    pub async fn migrate_content_engine(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS content_projects (
                id TEXT PRIMARY KEY NOT NULL,
                goal_id TEXT NOT NULL,
                run_id TEXT,
                name TEXT NOT NULL,
                brand_id TEXT,
                channels TEXT NOT NULL DEFAULT '[]',
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS brand_profiles (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                name TEXT NOT NULL,
                payload TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audience_profiles (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                payload TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS content_items (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                goal_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                parent_content_id TEXT,
                idea_id TEXT,
                brief_id TEXT,
                audience_id TEXT,
                pillar_id TEXT,
                experiment_id TEXT,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                format TEXT NOT NULL,
                channel TEXT NOT NULL,
                objective TEXT NOT NULL,
                lifecycle TEXT NOT NULL,
                cta TEXT,
                explainability TEXT NOT NULL DEFAULT '{}',
                quality_gates TEXT NOT NULL DEFAULT '{}',
                scheduled_at TEXT,
                legacy_draft_id TEXT,
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS content_versions (
                id TEXT PRIMARY KEY NOT NULL,
                content_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                stage TEXT NOT NULL,
                body TEXT NOT NULL,
                created_by TEXT NOT NULL,
                created_at TEXT NOT NULL,
                note TEXT
            );

            CREATE TABLE IF NOT EXISTS content_ideas (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                goal_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS content_briefs (
                id TEXT PRIMARY KEY NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS content_strategies (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                goal_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS content_publications (
                id TEXT PRIMARY KEY NOT NULL,
                content_id TEXT NOT NULL,
                channel TEXT NOT NULL,
                account_id TEXT,
                external_id TEXT,
                published_at TEXT NOT NULL,
                status TEXT NOT NULL,
                simulated INTEGER NOT NULL DEFAULT 0,
                metrics TEXT NOT NULL DEFAULT '{}',
                idempotency_key TEXT NOT NULL UNIQUE
            );

            CREATE TABLE IF NOT EXISTS content_insights (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS content_calendar (
                id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                content_id TEXT NOT NULL,
                payload TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_content_items_run ON content_items(run_id);
            CREATE INDEX IF NOT EXISTS idx_content_items_parent ON content_items(parent_content_id);
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn ensure_content_project(
        &self,
        goal_id: Uuid,
        run_id: Uuid,
        name: &str,
        channels: &[String],
    ) -> anyhow::Result<ContentProject> {
        if let Some(p) = self.get_content_project_for_goal(goal_id).await? {
            return Ok(p);
        }
        let now = Utc::now();
        let project = ContentProject {
            id: Uuid::now_v7(),
            goal_id,
            run_id: Some(run_id),
            name: name.into(),
            brand_id: None,
            channels: channels.to_vec(),
            metadata: Default::default(),
            created_at: now,
            updated_at: now,
        };
        sqlx::query(
            "INSERT INTO content_projects (id, goal_id, run_id, name, brand_id, channels, metadata, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(project.id.to_string())
        .bind(goal_id.to_string())
        .bind(run_id.to_string())
        .bind(&project.name)
        .bind(None::<String>)
        .bind(serde_json::to_string(&channels)?)
        .bind("{}")
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(project)
    }

    pub async fn get_content_project_for_goal(
        &self,
        goal_id: Uuid,
    ) -> anyhow::Result<Option<ContentProject>> {
        let row: Option<ProjectRow> =
            sqlx::query_as("SELECT * FROM content_projects WHERE goal_id = ? ORDER BY updated_at DESC LIMIT 1")
                .bind(goal_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        row.map(row_to_project).transpose()
    }

    pub async fn upsert_brand_profile(&self, brand: &BrandProfile) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO brand_profiles (id, project_id, name, payload, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET name = excluded.name, payload = excluded.payload, updated_at = excluded.updated_at
            "#,
        )
        .bind(brand.id.to_string())
        .bind(brand.project_id.to_string())
        .bind(&brand.name)
        .bind(serde_json::to_string(brand)?)
        .bind(brand.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_brand_for_project(
        &self,
        project_id: Uuid,
    ) -> anyhow::Result<Option<BrandProfile>> {
        let row: Option<BrandRow> =
            sqlx::query_as("SELECT * FROM brand_profiles WHERE project_id = ? LIMIT 1")
                .bind(project_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        row.map(|r| serde_json::from_str(&r.payload).map_err(Into::into))
            .transpose()
    }

    pub async fn insert_audience(&self, aud: &AudienceProfile) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO audience_profiles (id, project_id, payload) VALUES (?, ?, ?)")
            .bind(aud.id.to_string())
            .bind(aud.project_id.to_string())
            .bind(serde_json::to_string(aud)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_audiences(&self, project_id: Uuid) -> anyhow::Result<Vec<AudienceProfile>> {
        let rows: Vec<AudienceRow> =
            sqlx::query_as("SELECT * FROM audience_profiles WHERE project_id = ?")
                .bind(project_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|r| -> anyhow::Result<AudienceProfile> {
                Ok(serde_json::from_str(&r.payload)?)
            })
            .collect()
    }

    pub async fn upsert_strategy(&self, strategy: &ContentStrategy) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO content_strategies (id, project_id, goal_id, payload, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET payload = excluded.payload, updated_at = excluded.updated_at
            "#,
        )
        .bind(strategy.id.to_string())
        .bind(strategy.project_id.to_string())
        .bind(strategy.goal_id.to_string())
        .bind(serde_json::to_string(strategy)?)
        .bind(strategy.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_strategy_for_project(
        &self,
        project_id: Uuid,
    ) -> anyhow::Result<Option<ContentStrategy>> {
        let row: Option<StrategyRow> = sqlx::query_as(
            "SELECT * FROM content_strategies WHERE project_id = ? ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(project_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| serde_json::from_str(&r.payload).map_err(Into::into))
            .transpose()
    }

    pub async fn insert_idea(&self, idea: &ContentIdea) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO content_ideas (id, project_id, goal_id, run_id, payload, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(idea.id.to_string())
        .bind(idea.project_id.to_string())
        .bind(idea.goal_id.to_string())
        .bind(idea.run_id.to_string())
        .bind(serde_json::to_string(idea)?)
        .bind(idea.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_ideas(&self, run_id: Uuid) -> anyhow::Result<Vec<ContentIdea>> {
        let rows: Vec<IdeaRow> =
            sqlx::query_as("SELECT * FROM content_ideas WHERE run_id = ? ORDER BY created_at ASC")
                .bind(run_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|r| -> anyhow::Result<ContentIdea> {
                Ok(serde_json::from_str(&r.payload)?)
            })
            .collect()
    }

    pub async fn insert_brief(&self, brief: &ContentBrief) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO content_briefs (id, payload, created_at) VALUES (?, ?, ?)")
            .bind(brief.id.to_string())
            .bind(serde_json::to_string(brief)?)
            .bind(brief.created_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_content_item(&self, item: &ContentItem) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO content_items (
                id, project_id, goal_id, run_id, parent_content_id, idea_id, brief_id,
                audience_id, pillar_id, experiment_id, title, body, format, channel, objective,
                lifecycle, cta, explainability, quality_gates, scheduled_at, legacy_draft_id,
                metadata, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(item.id.to_string())
        .bind(item.project_id.to_string())
        .bind(item.goal_id.to_string())
        .bind(item.run_id.to_string())
        .bind(item.parent_content_id.map(|id| id.to_string()))
        .bind(item.idea_id.map(|id| id.to_string()))
        .bind(item.brief_id.map(|id| id.to_string()))
        .bind(item.audience_id.map(|id| id.to_string()))
        .bind(item.pillar_id.map(|id| id.to_string()))
        .bind(item.experiment_id.map(|id| id.to_string()))
        .bind(&item.title)
        .bind(&item.body)
        .bind(item.format.as_str())
        .bind(item.channel.as_str())
        .bind(item.objective.as_str())
        .bind(item.lifecycle.as_str())
        .bind(item.cta.map(|c| c.as_str().to_string()))
        .bind(serde_json::to_string(&item.explainability)?)
        .bind(serde_json::to_string(&item.quality_gates)?)
        .bind(item.scheduled_at.map(|d| d.to_rfc3339()))
        .bind(item.legacy_draft_id.map(|id| id.to_string()))
        .bind(serde_json::to_string(&item.metadata)?)
        .bind(item.created_at.to_rfc3339())
        .bind(item.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_content_item(&self, item: &ContentItem) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE content_items SET
                title = ?, body = ?, lifecycle = ?, quality_gates = ?, scheduled_at = ?,
                metadata = ?, updated_at = ?, parent_content_id = ?, brief_id = ?
            WHERE id = ?
            "#,
        )
        .bind(&item.title)
        .bind(&item.body)
        .bind(item.lifecycle.as_str())
        .bind(serde_json::to_string(&item.quality_gates)?)
        .bind(item.scheduled_at.map(|d| d.to_rfc3339()))
        .bind(serde_json::to_string(&item.metadata)?)
        .bind(item.updated_at.to_rfc3339())
        .bind(item.parent_content_id.map(|id| id.to_string()))
        .bind(item.brief_id.map(|id| id.to_string()))
        .bind(item.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_content_item(&self, id: Uuid) -> anyhow::Result<Option<ContentItem>> {
        let row: Option<ItemRow> = sqlx::query_as("SELECT * FROM content_items WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_item).transpose()
    }

    pub async fn list_content_items(&self, run_id: Uuid) -> anyhow::Result<Vec<ContentItem>> {
        let rows: Vec<ItemRow> =
            sqlx::query_as("SELECT * FROM content_items WHERE run_id = ? ORDER BY created_at ASC")
                .bind(run_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter().map(row_to_item).collect()
    }

    pub async fn insert_content_version(&self, ver: &ContentVersion) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO content_versions (id, content_id, version, stage, body, created_by, created_at, note) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(ver.id.to_string())
        .bind(ver.content_id.to_string())
        .bind(ver.version as i64)
        .bind(&ver.stage)
        .bind(&ver.body)
        .bind(&ver.created_by)
        .bind(ver.created_at.to_rfc3339())
        .bind(&ver.note)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn next_content_version(&self, content_id: Uuid) -> anyhow::Result<u32> {
        let n: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(version), 0) FROM content_versions WHERE content_id = ?",
        )
        .bind(content_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok((n.0 as u32) + 1)
    }

    pub async fn insert_publication(
        &self,
        pub_rec: &ContentPublication,
        idempotency_key: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO content_publications (
                id, content_id, channel, account_id, external_id, published_at, status,
                simulated, metrics, idempotency_key
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(pub_rec.id.to_string())
        .bind(pub_rec.content_id.to_string())
        .bind(pub_rec.channel.as_str())
        .bind(&pub_rec.account_id)
        .bind(&pub_rec.external_id)
        .bind(pub_rec.published_at.to_rfc3339())
        .bind(&pub_rec.status)
        .bind(if pub_rec.simulated { 1 } else { 0 })
        .bind(serde_json::to_string(&pub_rec.metrics)?)
        .bind(idempotency_key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_publication_by_idempotency(
        &self,
        key: &str,
    ) -> anyhow::Result<Option<ContentPublication>> {
        let row: Option<PublicationRow> =
            sqlx::query_as("SELECT * FROM content_publications WHERE idempotency_key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        row.map(row_to_publication).transpose()
    }

    pub async fn latest_publication_for_content(
        &self,
        content_id: Uuid,
    ) -> anyhow::Result<Option<ContentPublication>> {
        let row: Option<PublicationRow> = sqlx::query_as(
            "SELECT * FROM content_publications WHERE content_id = ? ORDER BY published_at DESC LIMIT 1",
        )
        .bind(content_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_publication).transpose()
    }

    pub async fn insert_insight(&self, insight: &ContentInsight) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO content_insights (id, project_id, run_id, payload, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(insight.id.to_string())
        .bind(insight.project_id.to_string())
        .bind(insight.run_id.to_string())
        .bind(serde_json::to_string(insight)?)
        .bind(insight.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_insights(&self, run_id: Uuid) -> anyhow::Result<Vec<ContentInsight>> {
        let rows: Vec<InsightRow> =
            sqlx::query_as("SELECT * FROM content_insights WHERE run_id = ? ORDER BY created_at DESC")
                .bind(run_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|r| -> anyhow::Result<ContentInsight> {
                Ok(serde_json::from_str(&r.payload)?)
            })
            .collect()
    }

    pub async fn insert_calendar_entry(&self, entry: &ContentCalendarEntry) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO content_calendar (id, project_id, content_id, payload) VALUES (?, ?, ?, ?)",
        )
        .bind(entry.id.to_string())
        .bind(entry.project_id.to_string())
        .bind(entry.content_id.to_string())
        .bind(serde_json::to_string(entry)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_calendar(&self, project_id: Uuid) -> anyhow::Result<Vec<ContentCalendarEntry>> {
        let rows: Vec<CalendarRow> =
            sqlx::query_as("SELECT * FROM content_calendar WHERE project_id = ?")
                .bind(project_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|r| -> anyhow::Result<ContentCalendarEntry> {
                Ok(serde_json::from_str(&r.payload)?)
            })
            .collect()
    }

    /// Backward compatibility: mirror legacy marketing drafts into content graph.
    pub async fn sync_legacy_drafts_to_content(
        &self,
        goal_id: Uuid,
        run_id: Uuid,
        project_name: &str,
        drafts: &[ContentDraft],
    ) -> anyhow::Result<ContentProject> {
        let project = self
            .ensure_content_project(goal_id, run_id, project_name, &["telegram".into()])
            .await?;
        for draft in drafts {
            if self.get_content_item(draft.id).await?.is_some() {
                continue;
            }
            let mut item = item_from_legacy_draft(draft, project.id, "Telegram draft");
            item.title = if draft.body.len() > 48 {
                format!("{}…", &draft.body.chars().take(48).collect::<String>())
            } else {
                draft.body.clone()
            };
            self.insert_content_item(&item).await?;
        }
        Ok(project)
    }

    pub async fn content_engine_snapshot(
        &self,
        goal_id: Uuid,
        run_id: Uuid,
    ) -> anyhow::Result<ContentEngineSnapshot> {
        let drafts = self.list_content_drafts(run_id).await.unwrap_or_default();
        let project = if let Some(p) = self.get_content_project_for_goal(goal_id).await? {
            p
        } else {
            self.sync_legacy_drafts_to_content(goal_id, run_id, "Content Campaign", &drafts)
                .await?
        };
        let brand = self.get_brand_for_project(project.id).await?;
        let audiences = self.list_audiences(project.id).await.unwrap_or_default();
        let strategy = self.get_strategy_for_project(project.id).await?;
        let ideas = self.list_ideas(run_id).await.unwrap_or_default();
        let mut items = self.list_content_items(run_id).await.unwrap_or_default();
        if items.is_empty() && !drafts.is_empty() {
            let _ = self
                .sync_legacy_drafts_to_content(goal_id, run_id, &project.name, &drafts)
                .await?;
            items = self.list_content_items(run_id).await?;
        }
        let calendar = self.list_calendar(project.id).await.unwrap_or_default();
        let insights = self.list_insights(run_id).await.unwrap_or_default();
        let mut publications = Vec::new();
        for item in &items {
            if let Some(p) = self.latest_publication_for_content(item.id).await? {
                publications.push(p);
            }
        }
        let graph_edges: Vec<ContentGraphEdge> = items
            .iter()
            .filter_map(|i| {
                i.parent_content_id.map(|pid| ContentGraphEdge {
                    parent_id: pid,
                    child_id: i.id,
                    relation: "derivative".into(),
                })
            })
            .collect();
        Ok(ContentEngineSnapshot {
            project: Some(project),
            brand,
            audiences,
            strategy,
            ideas,
            items,
            calendar,
            publications,
            insights,
            graph_edges,
        })
    }
}

fn row_to_project(row: ProjectRow) -> anyhow::Result<ContentProject> {
    Ok(ContentProject {
        id: Uuid::parse_str(&row.id)?,
        goal_id: Uuid::parse_str(&row.goal_id)?,
        run_id: row.run_id.as_deref().map(Uuid::parse_str).transpose()?,
        name: row.name,
        brand_id: row.brand_id.as_deref().map(Uuid::parse_str).transpose()?,
        channels: serde_json::from_str(&row.channels).unwrap_or_default(),
        metadata: serde_json::from_str(&row.metadata).unwrap_or_default(),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_item(row: ItemRow) -> anyhow::Result<ContentItem> {
    Ok(ContentItem {
        id: Uuid::parse_str(&row.id)?,
        project_id: Uuid::parse_str(&row.project_id)?,
        goal_id: Uuid::parse_str(&row.goal_id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        parent_content_id: row
            .parent_content_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?,
        idea_id: row.idea_id.as_deref().map(Uuid::parse_str).transpose()?,
        brief_id: row.brief_id.as_deref().map(Uuid::parse_str).transpose()?,
        audience_id: row.audience_id.as_deref().map(Uuid::parse_str).transpose()?,
        pillar_id: row.pillar_id.as_deref().map(Uuid::parse_str).transpose()?,
        experiment_id: row
            .experiment_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?,
        title: row.title,
        body: row.body,
        format: ContentFormatType::parse(&row.format),
        channel: ContentChannelId::parse(&row.channel),
        objective: ContentObjective::parse(&row.objective),
        lifecycle: ContentLifecycle::parse(&row.lifecycle),
        cta: row.cta.as_deref().map(|_| CtaType::Reply),
        explainability: serde_json::from_str(&row.explainability).unwrap_or_default(),
        quality_gates: serde_json::from_str(&row.quality_gates).unwrap_or_default(),
        scheduled_at: row.scheduled_at.as_deref().map(parse_dt).transpose()?,
        legacy_draft_id: row.legacy_draft_id.as_deref().map(Uuid::parse_str).transpose()?,
        metadata: serde_json::from_str(&row.metadata).unwrap_or_default(),
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
    })
}

fn row_to_publication(row: PublicationRow) -> anyhow::Result<ContentPublication> {
    Ok(ContentPublication {
        id: Uuid::parse_str(&row.id)?,
        content_id: Uuid::parse_str(&row.content_id)?,
        channel: ContentChannelId::parse(&row.channel),
        account_id: row.account_id,
        external_id: row.external_id,
        published_at: parse_dt(&row.published_at)?,
        status: row.status,
        simulated: row.simulated != 0,
        metrics: serde_json::from_str(&row.metrics).unwrap_or_default(),
    })
}

pub fn default_brand_for_project(project_id: Uuid, product: &str) -> BrandProfile {
    BrandProfile {
        id: Uuid::now_v7(),
        project_id,
        name: product.into(),
        description: format!("{product} — goal-driven automation platform."),
        products: vec![product.into()],
        value_proposition: "Turn business goals into autonomous loops.".into(),
        positioning: "AI Content Operating System on BoardDo runtime.".into(),
        tone_of_voice: BrandVoice::default(),
        vocabulary: vec!["goal".into(), "workflow".into(), "agent".into()],
        forbidden_terms: vec!["guaranteed results".into(), "get rich".into()],
        differentiators: vec!["Goal runtime".into(), "Policy + approval".into()],
        competitors: vec!["Zapier".into(), "Make".into()],
        brand_rules: Map::new(),
        examples: vec![],
        updated_at: Utc::now(),
    }
}
