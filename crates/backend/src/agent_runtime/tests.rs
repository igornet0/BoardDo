use std::path::PathBuf;
use std::sync::Arc;

use boarddo_shared::v2::CreateGoalRequest;
use boarddo_shared::ExecutionStatus;
use boarddo_telegram::{TelegramEngine, TelegramSessionStore};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;

use crate::connections::StorageConnectionProvider;
use crate::secrets::{EncryptedSqliteSecretStore, SecretStore};
use crate::state::{AppState, SharedState};
use crate::storage::Storage;

async fn test_state() -> SharedState {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let storage = Storage::from_pool(pool.clone());
    storage.migrate().await.unwrap();
    let secrets = Arc::new(EncryptedSqliteSecretStore::new(pool.clone(), [7u8; 32]));
    secrets.migrate().await.unwrap();
    let secrets_dyn: Arc<dyn SecretStore> = secrets;
    let connections = Arc::new(StorageConnectionProvider::new(
        storage.clone(),
        secrets_dyn.clone(),
    ));
    let telegram_store = Arc::new(TelegramSessionStore::new(pool));
    telegram_store.migrate().await.unwrap();
    let data_dir = PathBuf::from(format!(
        "{}/boarddo-agent-e2e-{}",
        std::env::temp_dir().display(),
        uuid::Uuid::now_v7()
    ));
    let (telegram, _) = TelegramEngine::with_mock(telegram_store, data_dir.clone());
    Arc::new(AppState::new(
        storage,
        secrets_dyn,
        connections,
        Arc::new(telegram),
        data_dir,
    ))
}

#[tokio::test]
async fn scenario_goal_creates_validates_and_runs_workflow() {
    let state = test_state().await;
    let spec = super::spec_from_request(CreateGoalRequest {
        title: "Ship a test scenario".into(),
        text: "Build a log workflow and prove it runs".into(),
        metric: "scenarios_validated".into(),
        target: 1.0,
        deadline: None,
        constraints: json!({}),
        budget: Default::default(),
        policy: Default::default(),
        agent_type_id: "agent.scenario".into(),
        instructions: String::new(),
        tools: vec![],
        tick_interval_secs: Some(5),
        openai_connection_id: None,
        template_id: Some("scenario".into()),
    });
    state.storage.insert_goal_spec(&spec).await.unwrap();
    let mut spec = spec;
    let run = super::start_run(&state, &mut spec, None).await.unwrap();

    super::tick::run_tick(&state, spec.clone(), run.clone())
        .await
        .unwrap();

    let ids = state
        .storage
        .list_goal_run_workflow_ids(run.id)
        .await
        .unwrap();
    assert!(
        !ids.is_empty(),
        "scenario tick should materialize a workflow"
    );
    let wf = state.storage.get_workflow(ids[0]).await.unwrap().unwrap();
    assert!(
        state.engine.validate(&wf.definition).is_ok(),
        "created workflow should validate"
    );

    let execs = state
        .storage
        .list_executions_for_workflow(wf.id, 5)
        .await
        .unwrap();
    assert!(
        execs.iter().any(|e| e.status == ExecutionStatus::Completed),
        "test run should complete: {execs:?}"
    );

    let mem = state.storage.get_memory(run.id, "bootstrap").await.unwrap();
    assert!(mem.is_some(), "bootstrap memory should be written");

    let updated = state.storage.get_goal_run(run.id).await.unwrap().unwrap();
    assert!(
        updated.current >= 1.0,
        "scenarios_validated should advance, current={}",
        updated.current
    );
}

#[tokio::test]
async fn marketing_goal_loop_smoke() {
    use boarddo_shared::v2::{
        ApprovalDecision, ContentDraftStatus, LeadStage, MarketingStage, PlannedToolCall,
    };

    let state = test_state().await;
    let spec = super::spec_from_request(CreateGoalRequest {
        title: "Promote Product X".into(),
        text: "Promote BoardDo. Zero ad budget. Find audience, hypotheses, content, Telegram, leads, learn.".into(),
        metric: "leads_interested".into(),
        target: 10.0,
        deadline: None,
        constraints: json!({
            "product": "BoardDo",
            "budget": 0,
            "target_leads": 10,
            "channel": "telegram",
            "autonomy_level": 2,
            "demo": true
        }),
        budget: Default::default(),
        policy: Default::default(),
        agent_type_id: "agent.marketing".into(),
        instructions: String::new(),
        tools: vec![],
        tick_interval_secs: Some(5),
        openai_connection_id: None,
        template_id: Some("marketing".into()),
    });
    assert_eq!(spec.agent_type_id, "agent.marketing");
    assert!(!spec.tools.is_empty());
    state.storage.insert_goal_spec(&spec).await.unwrap();
    let mut spec = spec;
    let run = super::start_run(&state, &mut spec, None).await.unwrap();

    // Drive the loop: research → audience → hypotheses → content → approval(+approvals) → …
    for _ in 0..24 {
        let current = state.storage.get_goal_run(run.id).await.unwrap().unwrap();
        if current.status == boarddo_shared::v2::GoalRunStatus::WaitingApproval {
            let pending = state
                .storage
                .list_approvals(Some(run.id), true)
                .await
                .unwrap();
            for a in pending {
                let call = PlannedToolCall {
                    id: a
                        .payload
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("approved")
                        .to_string(),
                    type_id: a.tool_type_id.clone(),
                    config: a.payload.get("config").cloned().unwrap_or(json!({})),
                };
                super::execute_approved_tool(&state, &spec, &current, &call)
                    .await
                    .expect("approved publish should succeed in demo mode");
                let mut a = a;
                a.status = ApprovalDecision::Approved;
                a.resolved_at = Some(chrono::Utc::now());
                state.storage.update_approval(&a).await.unwrap();
                let mut resumed = current.clone();
                resumed.status = boarddo_shared::v2::GoalRunStatus::Running;
                resumed.next_tick_at = Some(chrono::Utc::now());
                state.storage.update_goal_run(&resumed).await.unwrap();
            }
            continue;
        }
        if !current.status.is_active() {
            break;
        }
        super::tick::run_tick(&state, spec.clone(), current)
            .await
            .unwrap();

        let stage = state
            .storage
            .get_memory(run.id, "marketing.stage")
            .await
            .unwrap()
            .and_then(|m| m.value.as_str().map(|s| s.to_string()));
        let delta = state
            .storage
            .get_memory(run.id, "strategy_delta")
            .await
            .unwrap();
        if stage.as_deref() == Some(MarketingStage::Research.as_str()) && delta.is_some() {
            // Completed a full loop and returned to research with a strategy delta.
            break;
        }
    }

    let notes = state.storage.list_research_notes(run.id).await.unwrap();
    assert!(
        !notes.is_empty(),
        "research notes should be persisted"
    );
    assert!(
        notes.iter().any(|n| n.note_type == boarddo_shared::v2::ResearchNoteType::Audience),
        "audience notes expected"
    );
    assert!(
        notes.iter().any(|n| n.note_type == boarddo_shared::v2::ResearchNoteType::Pain),
        "pain notes expected"
    );

    let experiments = state.storage.list_experiments(run.id).await.unwrap();
    assert!(
        experiments.len() >= 3,
        "expected >=3 hypotheses, got {}",
        experiments.len()
    );

    let drafts = state.storage.list_content_drafts(run.id).await.unwrap();
    assert!(
        drafts.len() >= 3,
        "expected >=3 drafts, got {}",
        drafts.len()
    );
    assert!(
        drafts
            .iter()
            .any(|d| d.status == ContentDraftStatus::Published),
        "at least one draft should be published (demo/SIMULATED ok)"
    );

    let leads = state.storage.list_leads(run.id, true).await.unwrap();
    assert!(!leads.is_empty(), "lead should be created from simulated inbound");
    let lead = &leads[0];
    assert!(lead.score > 0, "lead should be scored");
    assert!(
        matches!(
            lead.stage,
            LeadStage::Interested | LeadStage::Qualified | LeadStage::Cold
        ),
        "lead stage={:?}",
        lead.stage
    );
    assert!(
        lead.experiment_id.is_some(),
        "lead should attribute to experiment"
    );
    assert!(lead.simulated, "demo inbound must be marked SIMULATED");

    let convs = state.storage.list_conversations(run.id).await.unwrap();
    assert!(!convs.is_empty(), "conversation should be linked");
    assert_eq!(convs[0].lead_id, Some(lead.id));

    let delta = state
        .storage
        .get_memory(run.id, "strategy_delta")
        .await
        .unwrap();
    assert!(delta.is_some(), "strategy_delta must be generated");

    let snap = state
        .storage
        .marketing_snapshot(spec.id, run.id, &spec.constraints)
        .await
        .unwrap();
    assert!(snap.funnel.research_notes > 0);
    assert!(snap.funnel.hypotheses >= 3);
    assert!(snap.funnel.drafts >= 3);
    assert!(snap.funnel.published >= 1);
    assert!(snap.funnel.leads >= 1 || snap.leads.iter().any(|l| l.simulated));

    // Next tick must be able to read strategy_delta (already stored under memory key).
    let _ = super::marketing::marketing_plan(
        &state,
        &spec,
        &state.storage.get_goal_run(run.id).await.unwrap().unwrap(),
        &experiments,
    )
    .await;
}

#[tokio::test]
async fn content_engine_project_adapt_and_publish() {
    let state = test_state().await;
    let spec = super::spec_from_request(CreateGoalRequest {
        title: "BoardDo Content".into(),
        text: "Promote BoardDo to developers".into(),
        metric: "leads_interested".into(),
        target: 5.0,
        deadline: None,
        constraints: json!({
            "product": "BoardDo",
            "channels": ["telegram", "blog"],
            "autonomy_level": 2,
            "demo": true
        }),
        budget: Default::default(),
        policy: Default::default(),
        agent_type_id: "agent.content".into(),
        instructions: String::new(),
        tools: vec![],
        tick_interval_secs: Some(5),
        openai_connection_id: None,
        template_id: Some("content".into()),
    });
    state.storage.insert_goal_spec(&spec).await.unwrap();
    let mut spec = spec;
    let run = super::start_run(&state, &mut spec, None).await.unwrap();

    let gateway = super::tools::ToolGateway::new();
    gateway
        .execute(
            &state,
            &spec,
            &run,
            &boarddo_shared::v2::PlannedToolCall {
                id: "s".into(),
                type_id: "strategy.create".into(),
                config: json!({}),
            },
        )
        .await
        .unwrap();
    gateway
        .execute(
            &state,
            &spec,
            &run,
            &boarddo_shared::v2::PlannedToolCall {
                id: "i".into(),
                type_id: "idea.generate".into(),
                config: json!({ "count": 3 }),
            },
        )
        .await
        .unwrap();
    let article = gateway
        .execute(
            &state,
            &spec,
            &run,
            &boarddo_shared::v2::PlannedToolCall {
                id: "c".into(),
                type_id: "content.create".into(),
                config: json!({
                    "format": "article",
                    "channel": "blog",
                    "title": "Goal runtime guide"
                }),
            },
        )
        .await
        .unwrap();
    let article_id = article
        .get("content")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_str())
        .unwrap();
    let adapted = gateway
        .execute(
            &state,
            &spec,
            &run,
            &boarddo_shared::v2::PlannedToolCall {
                id: "a".into(),
                type_id: "content.adapt".into(),
                config: json!({
                    "source_content_id": article_id,
                    "target_channel": "telegram",
                    "target_format": "telegram_post"
                }),
            },
        )
        .await
        .unwrap();
    let tg_id = adapted
        .get("content")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_str())
        .unwrap();
    gateway
        .execute(
            &state,
            &spec,
            &run,
            &boarddo_shared::v2::PlannedToolCall {
                id: "cr".into(),
                type_id: "content.critique".into(),
                config: json!({ "content_id": tg_id }),
            },
        )
        .await
        .unwrap();
    gateway
        .execute(
            &state,
            &spec,
            &run,
            &boarddo_shared::v2::PlannedToolCall {
                id: "p".into(),
                type_id: "publish.send".into(),
                config: json!({ "content_id": tg_id }),
            },
        )
        .await
        .unwrap();

    let snap = state
        .storage
        .content_engine_snapshot(spec.id, run.id)
        .await
        .unwrap();
    assert!(snap.project.is_some());
    assert!(snap.strategy.is_some());
    assert!(snap.ideas.len() >= 3);
    assert!(snap.items.len() >= 2);
    assert!(snap.graph_edges.len() >= 1);
    assert!(snap.publications.len() >= 1);
}

#[test]
fn registry_includes_fetch_and_analyze() {
    let reg = crate::engine::registry::NodeRegistry::with_defaults();
    assert!(reg.contains("web.fetch"));
    assert!(reg.contains("ai.analyze"));
}
