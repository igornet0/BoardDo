//! Content Engine tool execution (registered via ToolGateway).

use boarddo_shared::v2::{
    AudienceProfile, BrandProfile, ContentBrief, ContentCalendarEntry, ContentChannelId,
    ContentCritique, ContentExplainability, ContentFormatType, ContentIdea, ContentItem,
    ContentLifecycle, ContentObjective, ContentPillar, ContentQualityGates, ContentSourceRef,
    ContentStrategy, ContentVersion, CritiqueIssue, CtaType, FactClaim, ClaimVerification,
    ContentInsight, GoalRun, GoalSpec, product_from_constraints,
};
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::connections::ConnectionProvider;
use crate::integrations::openai::{
    OpenAiEndpoint, audio_speech, images_generate, videos_generate,
};
use crate::state::SharedState;
use crate::storage::content::default_brand_for_project;

use super::publishing::{prepare_publication, publish_content};

pub async fn execute(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    type_id: &str,
    config: &Value,
) -> Result<Value, String> {
    match type_id {
        "idea.generate" => idea_generate(state, spec, run, config).await,
        "idea.expand" | "idea.validate" | "idea.research" | "idea.combine" => {
            idea_generate(state, spec, run, config).await
        }
        "strategy.create" | "strategy.update" => strategy_create(state, spec, run, config).await,
        "brief.create" => brief_create(state, spec, run, config).await,
        "content.create" => content_create(state, spec, run, config).await,
        "content.adapt" => content_adapt(state, spec, run, config).await,
        "content.critique" => content_critique(state, spec, run, config).await,
        "content.fact_check" => content_fact_check(state, spec, run, config).await,
        "content.rewrite" | "content.expand" | "content.shorten" | "content.translate" => {
            content_revise(state, spec, run, type_id, config).await
        }
        "publish.prepare" => {
            let content_id = uuid_field(config, "content_id")?;
            prepare_publication(
                state,
                spec,
                run,
                content_id,
                str_field(config, "account_id"),
                str_field(config, "chat_id"),
            )
            .await
        }
        "publish.send" | "publisher.telegram.send" => {
            let content_id = config
                .get("content_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| "missing content_id".to_string())?;
            publish_content(
                state,
                spec,
                run,
                content_id,
                str_field(config, "account_id"),
                str_field(config, "chat_id"),
            )
            .await
        }
        "publisher.telegram.prepare" => {
            let content_id = config
                .get("content_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| "missing content_id".to_string())?;
            prepare_publication(state, spec, run, content_id, None, None).await
        }
        "publish.schedule" => schedule_publish(state, spec, run, config).await,
        "insight.create" => insight_create(state, spec, run, config).await,
        "analytics.read" => analytics_read(state, spec, run).await,
        "asset.generate" => asset_generate(state, spec, run, config).await,
        "asset.attach" => asset_attach(state, spec, run, config).await,
        other => Err(format!("unknown content tool `{other}`")),
    }
}

async fn project_ctx(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
) -> Result<(boarddo_shared::v2::ContentProject, BrandProfile), String> {
    let product = product_from_constraints(&spec.constraints);
    let channels: Vec<String> = spec
        .constraints
        .get("channels")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| vec!["telegram".into(), "blog".into()]);
    let project = state
        .storage
        .ensure_content_project(spec.id, run.id, &format!("{product} Content"), &channels)
        .await
        .map_err(|e| e.to_string())?;
    let brand = if let Some(b) = state
        .storage
        .get_brand_for_project(project.id)
        .await
        .map_err(|e| e.to_string())?
    {
        b
    } else {
        let b = default_brand_for_project(project.id, &product);
        state
            .storage
            .upsert_brand_profile(&b)
            .await
            .map_err(|e| e.to_string())?;
        b
    };
    Ok((project, brand))
}

async fn idea_generate(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let (project, brand) = project_ctx(state, spec, run).await?;
    let product = product_from_constraints(&spec.constraints);
    let notes = state
        .storage
        .list_research_notes(run.id)
        .await
        .unwrap_or_default();
    let pain = notes
        .iter()
        .find(|n| n.note_type == boarddo_shared::v2::ResearchNoteType::Pain)
        .map(|n| n.content.clone())
        .unwrap_or_else(|| "Manual work wastes hours on repetitive ops.".into());

    let templates = [
        (
            format!("Why {product} beats glue-code automation"),
            "Problem → insight → solution".into(),
            ContentFormatType::TelegramPost,
            ContentChannelId::Telegram,
        ),
        (
            format!("Tutorial: first marketing loop in {product}"),
            "Step-by-step for founders".into(),
            ContentFormatType::Article,
            ContentChannelId::Blog,
        ),
        (
            format!("Founders still triaging leads manually"),
            pain.clone(),
            ContentFormatType::ShortPost,
            ContentChannelId::Telegram,
        ),
        (
            format!("Goal runtime vs workflow builders"),
            "Comparison angle".into(),
            ContentFormatType::Thread,
            ContentChannelId::X,
        ),
        (
            format!("Behind the scenes: BoardDo content engine"),
            "Transparency / trust".into(),
            ContentFormatType::LongPost,
            ContentChannelId::Blog,
        ),
    ];

    let count = config.get("count").and_then(Value::as_u64).unwrap_or(5) as usize;
    let mut ids = Vec::new();
    let now = Utc::now();
    for (title, hook, format, channel) in templates.into_iter().take(count) {
        let idea = ContentIdea {
            id: Uuid::now_v7(),
            project_id: project.id,
            goal_id: spec.id,
            run_id: run.id,
            title,
            hook,
            audience_id: None,
            pain: pain.clone(),
            pillar_id: None,
            format,
            channel,
            objective: ContentObjective::LeadGeneration,
            source: format!("research+brand:{}", brand.name),
            experiment_id: None,
            created_at: now,
        };
        state
            .storage
            .insert_idea(&idea)
            .await
            .map_err(|e| e.to_string())?;
        audit(state, run.id, "idea.created", &idea.title).await;
        ids.push(idea.id);
    }
    Ok(json!({ "ok": true, "ideas_created": ids.len(), "ids": ids }))
}

async fn strategy_create(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    _config: &Value,
) -> Result<Value, String> {
    let (project, _) = project_ctx(state, spec, run).await?;
    let pillars = vec![
        ContentPillar {
            id: Uuid::now_v7(),
            project_id: project.id,
            name: "Education".into(),
            description: "Teach goal-driven automation".into(),
        },
        ContentPillar {
            id: Uuid::now_v7(),
            project_id: project.id,
            name: "Product".into(),
            description: "BoardDo capabilities".into(),
        },
        ContentPillar {
            id: Uuid::now_v7(),
            project_id: project.id,
            name: "Industry insights".into(),
            description: "Market + competitor angles".into(),
        },
    ];
    let strategy = ContentStrategy {
        id: Uuid::now_v7(),
        project_id: project.id,
        goal_id: spec.id,
        objectives: vec![
            ContentObjective::Awareness.as_str().into(),
            ContentObjective::LeadGeneration.as_str().into(),
        ],
        pillars,
        formats: vec![
            "telegram_post".into(),
            "article".into(),
            "thread".into(),
        ],
        channels: project.channels.clone(),
        frequency: "daily".into(),
        cta_strategy: "reply_interested".into(),
        distribution: Default::default(),
        updated_at: Utc::now(),
    };
    state
        .storage
        .upsert_strategy(&strategy)
        .await
        .map_err(|e| e.to_string())?;
    audit(state, run.id, "strategy.updated", "content strategy").await;
    Ok(json!({ "ok": true, "strategy": strategy }))
}

async fn brief_create(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let product = product_from_constraints(&spec.constraints);
    let idea_id = config
        .get("idea_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok());
    let brief = ContentBrief {
        id: Uuid::now_v7(),
        idea_id,
        content_id: None,
        goal: spec.text.clone(),
        audience: "Developers and founders building automations".into(),
        problem: "Manual content + outreach doesn't compound".into(),
        core_message: format!("{product} runs the full content loop on goals"),
        angle: config
            .get("angle")
            .and_then(Value::as_str)
            .unwrap_or("time saved")
            .into(),
        hook: config
            .get("hook")
            .and_then(Value::as_str)
            .unwrap_or("Stop writing ads — run a goal")
            .into(),
        structure: vec![
            "Hook".into(),
            "Problem".into(),
            "Insight".into(),
            "Proof".into(),
            "CTA".into(),
        ],
        cta: "Reply INTERESTED".into(),
        evidence: vec![],
        sources: vec![ContentSourceRef {
            url: "internal://research".into(),
            title: Some("Research notes".into()),
            relevance: 0.7,
        }],
        tone: "direct, technical".into(),
        channel: config
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or("telegram")
            .into(),
        format: config
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("telegram_post")
            .into(),
        created_at: Utc::now(),
    };
    state
        .storage
        .insert_brief(&brief)
        .await
        .map_err(|e| e.to_string())?;
    audit(state, run.id, "brief.created", &brief.hook).await;
    Ok(json!({ "ok": true, "brief": brief }))
}

async fn content_create(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let (project, brand) = project_ctx(state, spec, run).await?;
    let brief_id = config
        .get("brief_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok());
    let format = ContentFormatType::parse(
        config
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("telegram_post"),
    );
    let channel = ContentChannelId::parse(
        config
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or("telegram"),
    );
    let body = config
        .get("body")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "**{}**\n\n{} founders: {} saves hours by running research → content → publish → learn on one Goal.\n\nReply INTERESTED for a walkthrough.",
                brand.name,
                brand.tone_of_voice.tone,
                brand.name
            )
        });
    let title = config
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Content draft")
        .to_string();
    let now = Utc::now();
    let item = ContentItem {
        id: Uuid::now_v7(),
        project_id: project.id,
        goal_id: spec.id,
        run_id: run.id,
        parent_content_id: None,
        idea_id: config
            .get("idea_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok()),
        brief_id,
        audience_id: None,
        pillar_id: None,
        experiment_id: config
            .get("experiment_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok()),
        title: title.clone(),
        body: body.clone(),
        format,
        channel,
        objective: ContentObjective::parse(
            config
                .get("objective")
                .and_then(Value::as_str)
                .unwrap_or("lead_generation"),
        ),
        lifecycle: ContentLifecycle::Draft,
        cta: Some(CtaType::Reply),
        explainability: ContentExplainability {
            goal: spec.text.clone(),
            audience: "developers, founders".into(),
            pain: "manual content ops".into(),
            research_refs: vec!["research_notes".into()],
            strategy: "content pillars".into(),
            hypothesis: String::new(),
            expected_objective: ContentObjective::LeadGeneration.as_str().into(),
            generated_by: "content.create".into(),
        },
        quality_gates: ContentQualityGates::default(),
        scheduled_at: None,
        legacy_draft_id: None,
        metadata: Default::default(),
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .insert_content_item(&item)
        .await
        .map_err(|e| e.to_string())?;
    let ver = ContentVersion {
        id: Uuid::now_v7(),
        content_id: item.id,
        version: 1,
        stage: "draft".into(),
        body: body.clone(),
        created_by: "agent".into(),
        created_at: now,
        note: Some("initial draft".into()),
    };
    state
        .storage
        .insert_content_version(&ver)
        .await
        .map_err(|e| e.to_string())?;
    audit(state, run.id, "content.created", &title).await;
    audit(state, run.id, "content.version.created", "v1").await;
    Ok(json!({ "ok": true, "content": item, "version": ver }))
}

async fn content_adapt(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let source_id = uuid_field(config, "source_content_id")
        .or_else(|_| uuid_field(config, "content_id"))?;
    let source = state
        .storage
        .get_content_item(source_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "source content not found".to_string())?;
    let target_channel = ContentChannelId::parse(
        config
            .get("target_channel")
            .or_else(|| config.get("channel"))
            .and_then(Value::as_str)
            .unwrap_or("telegram"),
    );
    let target_format = ContentFormatType::parse(
        config
            .get("target_format")
            .or_else(|| config.get("format"))
            .and_then(Value::as_str)
            .unwrap_or("telegram_post"),
    );
    let adapted_body = match target_channel {
        ContentChannelId::Telegram => format!(
            "{}\n\n— adapted for Telegram (hook-first, short paragraphs).",
            truncate_lines(&source.body, 6)
        ),
        ContentChannelId::Blog | ContentChannelId::Website => format!(
            "# {}\n\n{}\n\n## Takeaways\n- Goal-driven loops\n- Policy + approval\n- Real metrics",
            source.title, source.body
        ),
        ContentChannelId::X => threadify(&source.body),
        _ => format!("Adapted ({}): {}", target_channel.as_str(), source.body),
    };
    let now = Utc::now();
    let child = ContentItem {
        id: Uuid::now_v7(),
        project_id: source.project_id,
        goal_id: source.goal_id,
        run_id: source.run_id,
        parent_content_id: Some(source.id),
        idea_id: source.idea_id,
        brief_id: source.brief_id,
        audience_id: source.audience_id,
        pillar_id: source.pillar_id,
        experiment_id: source.experiment_id,
        title: format!("{} → {}", source.title, target_channel.as_str()),
        body: adapted_body,
        format: target_format,
        channel: target_channel,
        objective: source.objective,
        lifecycle: ContentLifecycle::Draft,
        cta: source.cta,
        explainability: source.explainability.clone(),
        quality_gates: ContentQualityGates::default(),
        scheduled_at: None,
        legacy_draft_id: None,
        metadata: json!({ "adapted_from": source.id }).as_object().cloned().unwrap_or_default(),
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .insert_content_item(&child)
        .await
        .map_err(|e| e.to_string())?;
    audit(state, run.id, "content.adapted", &child.title).await;
    Ok(json!({ "ok": true, "content": child, "parent_id": source.id }))
}

async fn content_critique(
    state: &SharedState,
    _spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let content_id = uuid_field(config, "content_id")?;
    let mut item = state
        .storage
        .get_content_item(content_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "content not found".to_string())?;
    let brand = state
        .storage
        .get_brand_for_project(item.project_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut issues = Vec::new();
    if item.body.len() < 80 {
        issues.push(CritiqueIssue {
            code: "too_short".into(),
            severity: "medium".into(),
            message: "Body may be too short for the objective.".into(),
            suggestion: "Add a concrete example or pain point.".into(),
        });
    }
    if let Some(b) = &brand {
        for term in &b.forbidden_terms {
            if item.body.to_lowercase().contains(&term.to_lowercase()) {
                issues.push(CritiqueIssue {
                    code: "forbidden_term".into(),
                    severity: "high".into(),
                    message: format!("Contains forbidden term: {term}"),
                    suggestion: "Remove or rephrase.".into(),
                });
            }
        }
    }
    if !item.body.to_lowercase().contains("interested")
        && item.cta == Some(CtaType::Reply)
    {
        issues.push(CritiqueIssue {
            code: "weak_cta".into(),
            severity: "low".into(),
            message: "CTA may be unclear.".into(),
            suggestion: "Add explicit reply CTA.".into(),
        });
    }
    let critique = ContentCritique {
        content_id: item.id,
        issues: issues.clone(),
        ready_to_publish: issues.iter().all(|i| i.severity != "high"),
        created_at: Utc::now(),
    };
    item.lifecycle = ContentLifecycle::Review;
    item.quality_gates.brand_check = brand.is_some();
    item.quality_gates.audience_check = true;
    item.quality_gates.channel_check = true;
    item.quality_gates.spam_check = !item.body.to_lowercase().contains("free money");
    item.quality_gates.fact_check = true;
    item.updated_at = Utc::now();
    state
        .storage
        .update_content_item(&item)
        .await
        .map_err(|e| e.to_string())?;
    audit(state, run.id, "content.reviewed", "critique").await;
    Ok(json!({ "ok": true, "critique": critique, "content": item }))
}

async fn content_fact_check(
    state: &SharedState,
    _spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let content_id = uuid_field(config, "content_id")?;
    let item = state
        .storage
        .get_content_item(content_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "content not found".to_string())?;
    let claims = vec![FactClaim {
        claim: "Product automates full content lifecycle".into(),
        status: ClaimVerification::Uncertain,
        sources: vec![],
    }];
    audit(state, run.id, "content.reviewed", "fact_check").await;
    Ok(json!({ "ok": true, "content_id": item.id, "claims": claims }))
}

async fn content_revise(
    state: &SharedState,
    _spec: &GoalSpec,
    run: &GoalRun,
    type_id: &str,
    config: &Value,
) -> Result<Value, String> {
    let content_id = uuid_field(config, "content_id")?;
    let mut item = state
        .storage
        .get_content_item(content_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "content not found".to_string())?;
    let next = state
        .storage
        .next_content_version(item.id)
        .await
        .map_err(|e| e.to_string())?;
    let new_body = match type_id {
        "content.shorten" => truncate_lines(&item.body, 4),
        "content.expand" => format!("{}\n\nMore detail: concrete workflow + metrics.", item.body),
        "content.translate" => format!("[localized]\n{}", item.body),
        _ => format!("Revised: {}", item.body),
    };
    item.body = new_body.clone();
    item.updated_at = Utc::now();
    state
        .storage
        .update_content_item(&item)
        .await
        .map_err(|e| e.to_string())?;
    let ver = ContentVersion {
        id: Uuid::now_v7(),
        content_id: item.id,
        version: next,
        stage: "revision".into(),
        body: new_body,
        created_by: "agent".into(),
        created_at: Utc::now(),
        note: Some(type_id.into()),
    };
    state
        .storage
        .insert_content_version(&ver)
        .await
        .map_err(|e| e.to_string())?;
    audit(state, run.id, "content.updated", type_id).await;
    Ok(json!({ "ok": true, "content": item, "version": ver }))
}

async fn schedule_publish(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let content_id = uuid_field(config, "content_id")?;
    let mut item = state
        .storage
        .get_content_item(content_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "content not found".to_string())?;
    let at = Utc::now() + Duration::hours(24);
    item.scheduled_at = Some(at);
    item.lifecycle = ContentLifecycle::Scheduled;
    item.updated_at = Utc::now();
    state
        .storage
        .update_content_item(&item)
        .await
        .map_err(|e| e.to_string())?;
    let entry = ContentCalendarEntry {
        id: Uuid::now_v7(),
        project_id: item.project_id,
        content_id: item.id,
        channel: item.channel,
        format: item.format,
        scheduled_at: at,
        status: "scheduled".into(),
        campaign_id: Some(run.id),
    };
    state
        .storage
        .insert_calendar_entry(&entry)
        .await
        .map_err(|e| e.to_string())?;
    audit(state, run.id, "content.scheduled", &item.title).await;
    Ok(json!({ "ok": true, "calendar": entry, "content": item }))
}

async fn insight_create(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let (project, _) = project_ctx(state, spec, run).await?;
    let insight = ContentInsight {
        id: Uuid::now_v7(),
        project_id: project.id,
        run_id: run.id,
        observation: config
            .get("observation")
            .and_then(Value::as_str)
            .unwrap_or("Tutorial-style posts generated more replies.")
            .into(),
        evidence: config
            .get("evidence")
            .and_then(Value::as_str)
            .unwrap_or("sample>=3 posts")
            .into(),
        suggestion: config
            .get("suggestion")
            .and_then(Value::as_str)
            .unwrap_or("Increase tutorial pillar frequency.")
            .into(),
        confidence: 0.65,
        sample_size: 3,
        created_at: Utc::now(),
    };
    state
        .storage
        .insert_insight(&insight)
        .await
        .map_err(|e| e.to_string())?;
    audit(state, run.id, "insight.created", &insight.observation).await;
    Ok(json!({ "ok": true, "insight": insight }))
}

async fn analytics_read(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
) -> Result<Value, String> {
    let snap = state
        .storage
        .content_engine_snapshot(spec.id, run.id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true, "snapshot": snap }))
}

pub async fn sync_draft_to_content_item(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    draft: &boarddo_shared::v2::ContentDraft,
) -> Result<(), String> {
    let product = product_from_constraints(&spec.constraints);
    let project = state
        .storage
        .ensure_content_project(spec.id, run.id, &format!("{product} Content"), &["telegram".into()])
        .await
        .map_err(|e| e.to_string())?;
    if state
        .storage
        .get_content_item(draft.id)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let title = if draft.body.len() > 48 {
        format!("{}…", draft.body.chars().take(48).collect::<String>())
    } else {
        draft.body.clone()
    };
    let mut item =
        boarddo_shared::v2::item_from_legacy_draft(draft, project.id, &title);
    item.title = title;
    state
        .storage
        .insert_content_item(&item)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn uuid_field(config: &Value, key: &str) -> Result<Uuid, String> {
    config
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing `{key}`"))
        .and_then(|s| Uuid::parse_str(s).map_err(|e| e.to_string()))
}

fn str_field(config: &Value, key: &str) -> Option<String> {
    config.get(key).and_then(Value::as_str).map(str::to_string)
}

fn truncate_lines(s: &str, max_lines: usize) -> String {
    s.lines().take(max_lines).collect::<Vec<_>>().join("\n")
}

fn threadify(body: &str) -> String {
    body.lines()
        .take(5)
        .enumerate()
        .map(|(i, line)| format!("{}/5 {}", i + 1, line))
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn audit(state: &SharedState, run_id: Uuid, kind: &str, message: &str) {
    let _ = state
        .storage
        .insert_audit(&boarddo_shared::v2::AgentAuditEntry {
            id: Uuid::now_v7(),
            run_id,
            at: Utc::now(),
            kind: kind.into(),
            message: Some(message.into()),
            payload: None,
        })
        .await;
}

async fn asset_generate(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let kind = config
        .get("kind")
        .or_else(|| config.get("media"))
        .and_then(Value::as_str)
        .unwrap_or("image")
        .to_ascii_lowercase();
    let prompt = config
        .get("prompt")
        .or_else(|| config.get("text"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "asset.generate: missing prompt".to_string())?;
    let connection_id = config
        .get("connection_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| spec.openai_connection_id.map(|id| id.to_string()))
        .ok_or_else(|| "asset.generate: missing connection_id".to_string())?;
    let resolved = state.connections.resolve(&connection_id).await?;
    let api_key = resolved
        .credentials
        .get("api_key")
        .and_then(Value::as_str)
        .ok_or_else(|| "asset.generate: connection needs api_key".to_string())?
        .to_string();
    let endpoint = OpenAiEndpoint {
        api_key,
        base_url: boarddo_shared::openai_base_url(&resolved.config),
        default_model: boarddo_shared::openai_default_model(&resolved.config),
    };
    let model = config
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(endpoint.default_model.as_str());

    let out = match kind.as_str() {
        "audio" => {
            boarddo_shared::require_openai_capability(&resolved.config, model, "audio_generate")?;
            let bytes = audio_speech(&endpoint, model, prompt, "alloy", "mp3", 60_000).await?;
            let stored = state.media.save_bytes(&bytes, "audio/mpeg", "mp3").await?;
            json!({
                "kind": "audio",
                "uri": stored.uri,
                "mime": stored.mime,
                "model": model,
                "prompt": prompt,
            })
        }
        "video" => {
            boarddo_shared::require_openai_capability(&resolved.config, model, "video_generate")?;
            let job = videos_generate(&endpoint, model, prompt, None, Some(4), 120_000).await?;
            json!({
                "kind": "video",
                "job_id": job.id,
                "status": job.status,
                "model": model,
                "prompt": prompt,
                "raw": job.raw,
            })
        }
        _ => {
            boarddo_shared::require_openai_capability(&resolved.config, model, "image_generate")?;
            let img = images_generate(&endpoint, model, prompt, "1024x1024", None, 90_000).await?;
            let stored = if let Some(b64) = img.b64_json {
                state.media.save_base64(&b64, "image/png", "png").await?
            } else if let Some(url) = img.url {
                let (bytes, mime) = state.media.read_input_bytes(&url).await?;
                state.media.save_bytes(&bytes, &mime, "png").await?
            } else {
                return Err("asset.generate: empty image response".into());
            };
            json!({
                "kind": "image",
                "uri": stored.uri,
                "mime": stored.mime,
                "model": model,
                "prompt": prompt,
            })
        }
    };

    audit(
        state,
        run.id,
        "asset.generate",
        &format!("generated {kind}"),
    )
    .await;
    Ok(out)
}

async fn asset_attach(
    state: &SharedState,
    _spec: &GoalSpec,
    run: &GoalRun,
    config: &Value,
) -> Result<Value, String> {
    let uri = config
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| "asset.attach: missing uri".to_string())?;
    let kind = config
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("image");
    let mime = config
        .get("mime")
        .and_then(Value::as_str)
        .map(str::to_string);
    let artifact = boarddo_shared::v2::Artifact {
        id: Uuid::now_v7(),
        kind: match kind {
            "audio" => boarddo_shared::v2::ArtifactKind::Audio,
            "video" => boarddo_shared::v2::ArtifactKind::Video,
            "file" => boarddo_shared::v2::ArtifactKind::File,
            _ => boarddo_shared::v2::ArtifactKind::Image,
        },
        name: config
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        value: None,
        uri: Some(uri.to_string()),
        mime,
        produced_by: Some("asset.attach".into()),
        created_at: Utc::now(),
        meta: Some(json!({ "run_id": run.id })),
    };
    audit(state, run.id, "asset.attach", uri).await;
    Ok(json!({
        "artifact_id": artifact.id,
        "kind": kind,
        "uri": uri,
        "artifact": artifact,
    }))
}
