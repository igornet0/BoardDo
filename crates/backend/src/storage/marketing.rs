//! Marketing Goal persistence: leads, research notes, drafts, conversations.

use boarddo_shared::v2::{
    CampaignPlan, ContentDraft, ContentDraftStatus, ConversationIntent, ConversationMessage, Lead,
    LeadStage, Learning, MarketingConversation, MarketingFunnel, MarketingSnapshot, MarketingStage,
    ResearchNote, ResearchNoteType, StrategyDelta, autonomy_from_constraints,
};
use serde_json::{Map, Value, json};
use sqlx::FromRow;
use uuid::Uuid;

use super::{Storage, parse_dt};

#[derive(FromRow)]
struct LeadRow {
    id: String,
    goal_id: String,
    run_id: String,
    campaign_id: Option<String>,
    experiment_id: Option<String>,
    source: String,
    source_reference: Option<String>,
    chat_id: Option<String>,
    account_id: Option<String>,
    stage: String,
    score: i64,
    score_reasons: String,
    first_seen_at: String,
    last_activity_at: String,
    metadata: String,
    simulated: i64,
}

#[derive(FromRow)]
struct NoteRow {
    id: String,
    goal_id: String,
    run_id: String,
    campaign_id: Option<String>,
    note_type: String,
    content: String,
    source_url: Option<String>,
    source_title: Option<String>,
    confidence: f64,
    created_at: String,
    updated_at: String,
    metadata: String,
}

#[derive(FromRow)]
struct DraftRow {
    id: String,
    goal_id: String,
    run_id: String,
    campaign_id: Option<String>,
    experiment_id: Option<String>,
    channel: String,
    body: String,
    status: String,
    idempotency_key: String,
    telegram_message_id: Option<String>,
    account_id: Option<String>,
    chat_id: Option<String>,
    created_at: String,
    updated_at: String,
    metadata: String,
    simulated: i64,
}

#[derive(FromRow)]
struct ConvRow {
    id: String,
    goal_id: String,
    run_id: String,
    campaign_id: Option<String>,
    lead_id: Option<String>,
    experiment_id: Option<String>,
    chat_id: String,
    account_id: Option<String>,
    intent: String,
    messages: String,
    last_action: Option<String>,
    next_action: Option<String>,
    updated_at: String,
    simulated: i64,
}

impl Storage {
    pub async fn migrate_marketing(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS marketing_leads (
                id TEXT PRIMARY KEY NOT NULL,
                goal_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                campaign_id TEXT,
                experiment_id TEXT,
                source TEXT NOT NULL,
                source_reference TEXT,
                chat_id TEXT,
                account_id TEXT,
                stage TEXT NOT NULL DEFAULT 'cold',
                score INTEGER NOT NULL DEFAULT 0,
                score_reasons TEXT NOT NULL DEFAULT '[]',
                first_seen_at TEXT NOT NULL,
                last_activity_at TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                simulated INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS research_notes (
                id TEXT PRIMARY KEY NOT NULL,
                goal_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                campaign_id TEXT,
                note_type TEXT NOT NULL,
                content TEXT NOT NULL,
                source_url TEXT,
                source_title TEXT,
                confidence REAL NOT NULL DEFAULT 0.5,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS content_drafts (
                id TEXT PRIMARY KEY NOT NULL,
                goal_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                campaign_id TEXT,
                experiment_id TEXT,
                channel TEXT NOT NULL DEFAULT 'telegram',
                body TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                idempotency_key TEXT NOT NULL UNIQUE,
                telegram_message_id TEXT,
                account_id TEXT,
                chat_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                simulated INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS marketing_conversations (
                id TEXT PRIMARY KEY NOT NULL,
                goal_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                campaign_id TEXT,
                lead_id TEXT,
                experiment_id TEXT,
                chat_id TEXT NOT NULL,
                account_id TEXT,
                intent TEXT NOT NULL DEFAULT 'unknown',
                messages TEXT NOT NULL DEFAULT '[]',
                last_action TEXT,
                next_action TEXT,
                updated_at TEXT NOT NULL,
                simulated INTEGER NOT NULL DEFAULT 0,
                UNIQUE(run_id, chat_id),
                FOREIGN KEY(run_id) REFERENCES goal_runs(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_marketing_leads_run ON marketing_leads(run_id);
            CREATE INDEX IF NOT EXISTS idx_research_notes_run ON research_notes(run_id);
            CREATE INDEX IF NOT EXISTS idx_content_drafts_run ON content_drafts(run_id);
            CREATE INDEX IF NOT EXISTS idx_marketing_conv_run ON marketing_conversations(run_id);
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_lead(&self, lead: &Lead) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO marketing_leads (
                id, goal_id, run_id, campaign_id, experiment_id, source, source_reference,
                chat_id, account_id, stage, score, score_reasons, first_seen_at, last_activity_at,
                metadata, simulated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(lead.id.to_string())
        .bind(lead.goal_id.to_string())
        .bind(lead.run_id.to_string())
        .bind(lead.campaign_id.map(|id| id.to_string()))
        .bind(lead.experiment_id.map(|id| id.to_string()))
        .bind(&lead.source)
        .bind(&lead.source_reference)
        .bind(&lead.chat_id)
        .bind(&lead.account_id)
        .bind(lead.stage.as_str())
        .bind(lead.score as i64)
        .bind(serde_json::to_string(&lead.score_reasons)?)
        .bind(lead.first_seen_at.to_rfc3339())
        .bind(lead.last_activity_at.to_rfc3339())
        .bind(serde_json::to_string(&lead.metadata)?)
        .bind(if lead.simulated { 1 } else { 0 })
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_lead(&self, lead: &Lead) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE marketing_leads SET
                stage = ?, score = ?, score_reasons = ?, last_activity_at = ?,
                experiment_id = ?, metadata = ?, source_reference = ?
            WHERE id = ?
            "#,
        )
        .bind(lead.stage.as_str())
        .bind(lead.score as i64)
        .bind(serde_json::to_string(&lead.score_reasons)?)
        .bind(lead.last_activity_at.to_rfc3339())
        .bind(lead.experiment_id.map(|id| id.to_string()))
        .bind(serde_json::to_string(&lead.metadata)?)
        .bind(&lead.source_reference)
        .bind(lead.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_lead(&self, id: Uuid) -> anyhow::Result<Option<Lead>> {
        let row: Option<LeadRow> = sqlx::query_as("SELECT * FROM marketing_leads WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_lead).transpose()
    }

    pub async fn find_lead_by_chat(
        &self,
        run_id: Uuid,
        chat_id: &str,
    ) -> anyhow::Result<Option<Lead>> {
        let row: Option<LeadRow> = sqlx::query_as(
            "SELECT * FROM marketing_leads WHERE run_id = ? AND chat_id = ? ORDER BY last_activity_at DESC LIMIT 1",
        )
        .bind(run_id.to_string())
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_lead).transpose()
    }

    pub async fn list_leads(
        &self,
        run_id: Uuid,
        include_simulated: bool,
    ) -> anyhow::Result<Vec<Lead>> {
        let rows: Vec<LeadRow> = if include_simulated {
            sqlx::query_as(
                "SELECT * FROM marketing_leads WHERE run_id = ? ORDER BY last_activity_at DESC",
            )
            .bind(run_id.to_string())
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT * FROM marketing_leads WHERE run_id = ? AND simulated = 0 ORDER BY last_activity_at DESC",
            )
            .bind(run_id.to_string())
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(row_to_lead).collect()
    }

    pub async fn insert_research_note(&self, note: &ResearchNote) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO research_notes (
                id, goal_id, run_id, campaign_id, note_type, content, source_url, source_title,
                confidence, created_at, updated_at, metadata
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(note.id.to_string())
        .bind(note.goal_id.to_string())
        .bind(note.run_id.to_string())
        .bind(note.campaign_id.map(|id| id.to_string()))
        .bind(note.note_type.as_str())
        .bind(&note.content)
        .bind(&note.source_url)
        .bind(&note.source_title)
        .bind(note.confidence as f64)
        .bind(note.created_at.to_rfc3339())
        .bind(note.updated_at.to_rfc3339())
        .bind(serde_json::to_string(&note.metadata)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_research_notes(&self, run_id: Uuid) -> anyhow::Result<Vec<ResearchNote>> {
        let rows: Vec<NoteRow> = sqlx::query_as(
            "SELECT * FROM research_notes WHERE run_id = ? ORDER BY created_at ASC",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_note).collect()
    }

    pub async fn insert_content_draft(&self, draft: &ContentDraft) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO content_drafts (
                id, goal_id, run_id, campaign_id, experiment_id, channel, body, status,
                idempotency_key, telegram_message_id, account_id, chat_id, created_at, updated_at,
                metadata, simulated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(draft.id.to_string())
        .bind(draft.goal_id.to_string())
        .bind(draft.run_id.to_string())
        .bind(draft.campaign_id.map(|id| id.to_string()))
        .bind(draft.experiment_id.map(|id| id.to_string()))
        .bind(&draft.channel)
        .bind(&draft.body)
        .bind(draft.status.as_str())
        .bind(&draft.idempotency_key)
        .bind(&draft.telegram_message_id)
        .bind(&draft.account_id)
        .bind(&draft.chat_id)
        .bind(draft.created_at.to_rfc3339())
        .bind(draft.updated_at.to_rfc3339())
        .bind(serde_json::to_string(&draft.metadata)?)
        .bind(if draft.simulated { 1 } else { 0 })
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_content_draft(&self, draft: &ContentDraft) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE content_drafts SET
                body = ?, status = ?, telegram_message_id = ?, account_id = ?, chat_id = ?,
                updated_at = ?, metadata = ?
            WHERE id = ?
            "#,
        )
        .bind(&draft.body)
        .bind(draft.status.as_str())
        .bind(&draft.telegram_message_id)
        .bind(&draft.account_id)
        .bind(&draft.chat_id)
        .bind(draft.updated_at.to_rfc3339())
        .bind(serde_json::to_string(&draft.metadata)?)
        .bind(draft.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_content_draft(&self, id: Uuid) -> anyhow::Result<Option<ContentDraft>> {
        let row: Option<DraftRow> = sqlx::query_as("SELECT * FROM content_drafts WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_draft).transpose()
    }

    pub async fn get_draft_by_idempotency(
        &self,
        key: &str,
    ) -> anyhow::Result<Option<ContentDraft>> {
        let row: Option<DraftRow> =
            sqlx::query_as("SELECT * FROM content_drafts WHERE idempotency_key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        row.map(row_to_draft).transpose()
    }

    pub async fn list_content_drafts(&self, run_id: Uuid) -> anyhow::Result<Vec<ContentDraft>> {
        let rows: Vec<DraftRow> = sqlx::query_as(
            "SELECT * FROM content_drafts WHERE run_id = ? ORDER BY created_at ASC",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_draft).collect()
    }

    pub async fn upsert_conversation(
        &self,
        conv: &MarketingConversation,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO marketing_conversations (
                id, goal_id, run_id, campaign_id, lead_id, experiment_id, chat_id, account_id,
                intent, messages, last_action, next_action, updated_at, simulated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(run_id, chat_id) DO UPDATE SET
                lead_id = excluded.lead_id,
                experiment_id = excluded.experiment_id,
                intent = excluded.intent,
                messages = excluded.messages,
                last_action = excluded.last_action,
                next_action = excluded.next_action,
                updated_at = excluded.updated_at,
                simulated = excluded.simulated
            "#,
        )
        .bind(conv.id.to_string())
        .bind(conv.goal_id.to_string())
        .bind(conv.run_id.to_string())
        .bind(conv.campaign_id.map(|id| id.to_string()))
        .bind(conv.lead_id.map(|id| id.to_string()))
        .bind(conv.experiment_id.map(|id| id.to_string()))
        .bind(&conv.chat_id)
        .bind(&conv.account_id)
        .bind(conv.intent.as_str())
        .bind(serde_json::to_string(&conv.messages)?)
        .bind(&conv.last_action)
        .bind(&conv.next_action)
        .bind(conv.updated_at.to_rfc3339())
        .bind(if conv.simulated { 1 } else { 0 })
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_conversation_by_chat(
        &self,
        run_id: Uuid,
        chat_id: &str,
    ) -> anyhow::Result<Option<MarketingConversation>> {
        let row: Option<ConvRow> = sqlx::query_as(
            "SELECT * FROM marketing_conversations WHERE run_id = ? AND chat_id = ?",
        )
        .bind(run_id.to_string())
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_conv).transpose()
    }

    pub async fn list_conversations(
        &self,
        run_id: Uuid,
    ) -> anyhow::Result<Vec<MarketingConversation>> {
        let rows: Vec<ConvRow> = sqlx::query_as(
            "SELECT * FROM marketing_conversations WHERE run_id = ? ORDER BY updated_at DESC",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_conv).collect()
    }

    pub async fn marketing_snapshot(
        &self,
        _goal_id: Uuid,
        run_id: Uuid,
        constraints: &Value,
    ) -> anyhow::Result<MarketingSnapshot> {
        let notes = self.list_research_notes(run_id).await?;
        let drafts = self.list_content_drafts(run_id).await?;
        let leads = self.list_leads(run_id, true).await?;
        let conversations = self.list_conversations(run_id).await?;
        let experiments = self.list_experiments(run_id).await?;
        let memory = self.list_memory(run_id).await?;
        let run = self.get_goal_run(run_id).await?;

        let stage = memory
            .iter()
            .find(|m| m.key == "marketing.stage")
            .and_then(|m| m.value.as_str())
            .map(MarketingStage::parse)
            .unwrap_or(MarketingStage::Research);

        let strategy_deltas: Vec<StrategyDelta> = memory
            .iter()
            .filter(|m| m.key.starts_with("strategy_delta"))
            .filter_map(|m| serde_json::from_value(m.value.clone()).ok())
            .collect();

        let learnings: Vec<Learning> = memory
            .iter()
            .filter(|m| m.key.starts_with("learning."))
            .filter_map(|m| serde_json::from_value(m.value.clone()).ok())
            .collect();

        let plan: Option<CampaignPlan> = memory
            .iter()
            .find(|m| m.key == "campaign.plan")
            .and_then(|m| serde_json::from_value(m.value.clone()).ok());

        let current_objective = memory
            .iter()
            .find(|m| m.key == "current_objective")
            .and_then(|m| {
                m.value
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| Some(m.value.to_string()))
            })
            .or_else(|| run.as_ref().and_then(|r| r.last_think.clone()));

        let channel_weights = run
            .as_ref()
            .map(|r| r.strategy.clone())
            .unwrap_or_default();

        let autonomy_level = autonomy_from_constraints(constraints);

        let mut funnel = MarketingFunnel::default();
        funnel.research_notes = notes.len() as u32;
        funnel.audience_notes = notes
            .iter()
            .filter(|n| n.note_type == ResearchNoteType::Audience)
            .count() as u32;
        funnel.pain_notes = notes
            .iter()
            .filter(|n| n.note_type == ResearchNoteType::Pain)
            .count() as u32;
        funnel.hypotheses = experiments.len() as u32;
        funnel.drafts = drafts.len() as u32;
        funnel.pending_approval = drafts
            .iter()
            .filter(|d| d.status == ContentDraftStatus::PendingApproval)
            .count() as u32;
        funnel.published = drafts
            .iter()
            .filter(|d| d.status == ContentDraftStatus::Published)
            .count() as u32;
        funnel.conversations = conversations.len() as u32;
        let prod_leads: Vec<_> = leads.iter().filter(|l| !l.simulated).collect();
        funnel.leads = prod_leads.len() as u32;
        funnel.interested = prod_leads
            .iter()
            .filter(|l| matches!(l.stage, LeadStage::Interested | LeadStage::Qualified | LeadStage::Converted))
            .count() as u32;
        funnel.qualified = prod_leads
            .iter()
            .filter(|l| matches!(l.stage, LeadStage::Qualified | LeadStage::Converted))
            .count() as u32;
        funnel.conversions = prod_leads
            .iter()
            .filter(|l| l.stage == LeadStage::Converted)
            .count() as u32;

        let mut stage_progress = Map::new();
        for s in MarketingStage::all() {
            let done = match s {
                MarketingStage::Research => funnel.research_notes > 0,
                MarketingStage::Audience => funnel.audience_notes > 0,
                MarketingStage::Hypotheses => funnel.hypotheses >= 3,
                MarketingStage::Content => funnel.drafts >= 3,
                MarketingStage::Approval => {
                    drafts.iter().any(|d| {
                        matches!(
                            d.status,
                            ContentDraftStatus::Approved | ContentDraftStatus::Published
                        )
                    }) || funnel.pending_approval == 0 && funnel.drafts >= 3
                }
                MarketingStage::Publish => funnel.published > 0,
                MarketingStage::Engage => funnel.conversations > 0,
                MarketingStage::Lead => funnel.leads > 0,
                MarketingStage::Analytics => experiments.iter().any(|e| {
                    e.metrics
                        .get("leads")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        > 0
                        || e.metrics
                            .get("posts")
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                            > 0
                }),
                MarketingStage::Learn => !strategy_deltas.is_empty() || !learnings.is_empty(),
            };
            let status = if *s == stage {
                if done {
                    "done"
                } else {
                    "running"
                }
            } else if done {
                "done"
            } else {
                "pending"
            };
            stage_progress.insert(s.as_str().into(), json!(status));
        }

        let plan_ready = plan.as_ref().map(|p| p.ready).unwrap_or(false);

        Ok(MarketingSnapshot {
            stage,
            autonomy_level,
            autonomy_mode: autonomy_level.mode_label().into(),
            funnel,
            leads,
            research_notes: notes,
            drafts,
            conversations,
            strategy_deltas,
            stage_progress,
            plan,
            learnings,
            current_objective,
            channel_weights,
            plan_ready,
        })
    }
}

fn row_to_lead(row: LeadRow) -> anyhow::Result<Lead> {
    Ok(Lead {
        id: Uuid::parse_str(&row.id)?,
        goal_id: Uuid::parse_str(&row.goal_id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        campaign_id: row.campaign_id.as_deref().map(Uuid::parse_str).transpose()?,
        experiment_id: row
            .experiment_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?,
        source: row.source,
        source_reference: row.source_reference,
        chat_id: row.chat_id,
        account_id: row.account_id,
        stage: LeadStage::parse(&row.stage),
        score: row.score as i32,
        score_reasons: serde_json::from_str(&row.score_reasons).unwrap_or_default(),
        first_seen_at: parse_dt(&row.first_seen_at)?,
        last_activity_at: parse_dt(&row.last_activity_at)?,
        metadata: serde_json::from_str(&row.metadata).unwrap_or_default(),
        simulated: row.simulated != 0,
    })
}

fn row_to_note(row: NoteRow) -> anyhow::Result<ResearchNote> {
    Ok(ResearchNote {
        id: Uuid::parse_str(&row.id)?,
        goal_id: Uuid::parse_str(&row.goal_id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        campaign_id: row.campaign_id.as_deref().map(Uuid::parse_str).transpose()?,
        note_type: ResearchNoteType::parse(&row.note_type),
        content: row.content,
        source_url: row.source_url,
        source_title: row.source_title,
        confidence: row.confidence as f32,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
        metadata: serde_json::from_str(&row.metadata).unwrap_or_default(),
    })
}

fn row_to_draft(row: DraftRow) -> anyhow::Result<ContentDraft> {
    Ok(ContentDraft {
        id: Uuid::parse_str(&row.id)?,
        goal_id: Uuid::parse_str(&row.goal_id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        campaign_id: row.campaign_id.as_deref().map(Uuid::parse_str).transpose()?,
        experiment_id: row
            .experiment_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?,
        channel: row.channel,
        body: row.body,
        status: ContentDraftStatus::parse(&row.status),
        idempotency_key: row.idempotency_key,
        telegram_message_id: row.telegram_message_id,
        account_id: row.account_id,
        chat_id: row.chat_id,
        created_at: parse_dt(&row.created_at)?,
        updated_at: parse_dt(&row.updated_at)?,
        metadata: serde_json::from_str(&row.metadata).unwrap_or_default(),
        simulated: row.simulated != 0,
    })
}

fn row_to_conv(row: ConvRow) -> anyhow::Result<MarketingConversation> {
    Ok(MarketingConversation {
        id: Uuid::parse_str(&row.id)?,
        goal_id: Uuid::parse_str(&row.goal_id)?,
        run_id: Uuid::parse_str(&row.run_id)?,
        campaign_id: row.campaign_id.as_deref().map(Uuid::parse_str).transpose()?,
        lead_id: row.lead_id.as_deref().map(Uuid::parse_str).transpose()?,
        experiment_id: row
            .experiment_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?,
        chat_id: row.chat_id,
        account_id: row.account_id,
        intent: ConversationIntent::parse(&row.intent),
        messages: serde_json::from_str::<Vec<ConversationMessage>>(&row.messages)
            .unwrap_or_default(),
        last_action: row.last_action,
        next_action: row.next_action,
        updated_at: parse_dt(&row.updated_at)?,
        simulated: row.simulated != 0,
    })
}
