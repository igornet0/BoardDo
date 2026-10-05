use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::v2::agent::{AgentSpec, AgentTool};
use crate::v2::artifact::{Artifact, ArtifactKind};
use crate::v2::capability::{CapabilityBudget, caps};
use crate::v2::context::RunContext;
use crate::v2::node_contract::{NestedWorkflowRef, NodeKind};
use crate::v2::ports::{IoContract, PortSpec};
use crate::v2::research::{Evidence, ResearchContext, ResearchDocument, SourceScore};
use crate::v2::web::{WebFetchRequest, WebPolicy, WebSearchRequest, tools};
use crate::v2::workflow_contract::{CompositePublish, WorkflowContract};
use crate::{Edge, Position, WorkflowStatus, type_ids};

#[test]
fn capability_deny_wins_and_narrows() {
    let parent = CapabilityBudget::default()
        .grant(caps::TELEGRAM_WRITE)
        .grant(caps::ADS_CREATE)
        .grant(caps::ADS_LAUNCH)
        .forbid(caps::BILLING_WRITE);

    let child = CapabilityBudget::default()
        .grant(caps::TELEGRAM_WRITE)
        .grant(caps::ADS_LAUNCH)
        .grant(caps::BILLING_WRITE); // attempt escalate

    let effective = parent.narrow(&child);
    assert!(effective.allows(caps::TELEGRAM_WRITE));
    assert!(effective.allows(caps::ADS_LAUNCH));
    assert!(!effective.allows(caps::ADS_CREATE)); // not in child allow
    assert!(!effective.allows(caps::BILLING_WRITE)); // parent deny
}

#[test]
fn run_context_artifacts_mirror_variables() {
    let mut ctx = RunContext::new(
        Uuid::now_v7(),
        Uuid::now_v7(),
        1,
        json!({}),
        CapabilityBudget::default().grant(caps::ARTIFACT_WRITE),
    );
    ctx.push_artifact(Artifact::text("analysis", "BTC looks bullish"));
    assert_eq!(ctx.variables["analysis"], "BTC looks bullish");
    assert_eq!(ctx.artifacts[0].kind, ArtifactKind::Text);
    assert!(ctx.can(caps::ARTIFACT_WRITE));
}

#[test]
fn composite_publish_roundtrip() {
    let inner_id = Uuid::now_v7();
    let wf = WorkflowContract {
        id: inner_id,
        name: "Telegram Content Pipeline".into(),
        description: "Research → article → image → approve → publish".into(),
        version: 1,
        status: WorkflowStatus::Active,
        goal: Some("Publish approved Telegram content from a topic".into()),
        io: IoContract {
            inputs: vec![PortSpec::required("topic", ArtifactKind::Text)],
            outputs: vec![PortSpec::required("post_id", ArtifactKind::Text)],
        },
        budget: CapabilityBudget::default()
            .grant(caps::TELEGRAM_WRITE)
            .grant(caps::AI_GENERATE)
            .forbid(caps::ADS_LAUNCH),
        nodes: vec![],
        edges: Vec::<Edge>::new(),
        published_as: Some(CompositePublish {
            type_id: "composite.telegram_content_pipeline".into(),
            title: "Telegram Content Pipeline".into(),
            description: Some("End-to-end content publish".into()),
            intent: Some("Turn a topic into an approved Telegram post".into()),
            io: IoContract {
                inputs: vec![PortSpec::required("topic", ArtifactKind::Text)],
                outputs: vec![PortSpec::required("post_id", ArtifactKind::Text)],
            },
            budget: CapabilityBudget::default().grant(caps::TELEGRAM_WRITE),
        }),
        meta: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let json = serde_json::to_value(&wf).unwrap();
    let back: WorkflowContract = serde_json::from_value(json).unwrap();
    assert_eq!(
        back.published_as.as_ref().unwrap().type_id,
        "composite.telegram_content_pipeline"
    );
    assert_eq!(back.io.inputs[0].name, "topic");

    let nested = NestedWorkflowRef::Stored {
        workflow_id: inner_id,
        version: Some(1),
    };
    assert!(matches!(nested, NestedWorkflowRef::Stored { .. }));
    assert_eq!(NodeKind::Composite, NodeKind::Composite);

    let _ = Position { x: 0.0, y: 0.0 };
    let _ = AgentSpec {
        id: Uuid::now_v7(),
        type_id: "agent.marketing".into(),
        title: "Marketing Agent".into(),
        description: None,
        instructions: "Create campaigns but never launch without approval".into(),
        io: IoContract::default(),
        budget: CapabilityBudget::default()
            .grant(caps::ADS_CREATE)
            .grant(caps::ADS_READ)
            .grant(caps::WEB_FETCH)
            .forbid(caps::ADS_LAUNCH),
        tools: vec![AgentTool {
            type_id: tools::FETCH.into(),
            title: Some("Fetch URL".into()),
            config: json!({}),
        }],
        memory: Default::default(),
        workflow: NestedWorkflowRef::Stored {
            workflow_id: inner_id,
            version: None,
        },
        require_approval_for_side_effects: true,
    };
}

#[test]
fn web_policy_blocks_private_and_loopback() {
    let policy = WebPolicy::default();
    assert!(policy.is_url_allowed("https://docs.rs/anyhow"));
    assert!(!policy.is_url_allowed("http://127.0.0.1/secret"));
    assert!(!policy.is_url_allowed("http://localhost:8080/"));
    assert!(!policy.is_url_allowed("http://10.0.0.1/admin"));
    assert!(!policy.is_url_allowed("http://192.168.1.1/"));
    assert!(!policy.is_url_allowed("http://172.16.5.4/"));
    assert!(!policy.is_url_allowed("http://[::1]/"));
    assert!(!policy.is_url_allowed("http://169.254.169.254/latest"));
    assert!(!policy.is_url_allowed("file:///etc/passwd"));
}

#[test]
fn web_policy_allowlist_and_budget_roundtrip() {
    let mut policy = WebPolicy::default();
    policy.allowed_domains = vec!["github.com".into(), "docs.rs".into()];
    assert!(policy.is_url_allowed("https://docs.github.com/en"));
    assert!(!policy.is_url_allowed("https://example.com/"));
    policy.blocked_domains = vec!["evil.github.com".into()];
    assert!(!policy.is_url_allowed("https://evil.github.com/payload"));
    assert!(policy.check_budget(0).is_ok());
    assert!(policy.check_budget(50).is_err());
    assert!(policy.check_method("POST").is_err());
    assert!(policy.check_method("GET").is_ok());

    let json = serde_json::to_value(&policy).unwrap();
    let back: WebPolicy = serde_json::from_value(json).unwrap();
    assert_eq!(back.allowed_domains, policy.allowed_domains);
    assert!(!back.allow_local_network);

    let req = WebSearchRequest {
        query: "best Rust embedded database 2026".into(),
        limit: Some(10),
        freshness: Some("30d".into()),
    };
    let search_json = serde_json::to_value(&req).unwrap();
    let search_back: WebSearchRequest = serde_json::from_value(search_json).unwrap();
    assert_eq!(search_back.query, req.query);

    let fetch = WebFetchRequest {
        url: "https://docs.rs/serde".into(),
        ..Default::default()
    };
    let fetch_json = serde_json::to_value(&fetch).unwrap();
    let fetch_back: WebFetchRequest = serde_json::from_value(fetch_json).unwrap();
    assert_eq!(fetch_back.method, "GET");
}

#[test]
fn source_score_weight_and_research_context() {
    let official = SourceScore {
        authority: 1.0,
        freshness: 1.0,
        relevance: 1.0,
        reliability: 1.0,
    };
    assert_eq!(official.weight(), 1.0);
    let github = SourceScore {
        authority: 0.9,
        freshness: 0.9,
        relevance: 1.0,
        reliability: 1.0,
    };
    assert!((github.weight() - 0.81).abs() < f32::EPSILON);
    let forum = SourceScore {
        authority: 0.3,
        freshness: 0.8,
        relevance: 1.0,
        reliability: 1.0,
    };
    assert!((forum.weight() - 0.24).abs() < f32::EPSILON);

    let mut ctx = ResearchContext::new("PostgreSQL vs SQLite for X");
    ctx.push_document(ResearchDocument::new(
        "https://www.sqlite.org/docs.html",
        "SQLite docs",
        "sqlite.org",
        "SQLite is a serverless database.",
    ));
    assert_eq!(ctx.documents.len(), 1);
    assert_eq!(ctx.sources.len(), 1);

    assert!(
        ctx.add_claim_with_evidence("SQLite is serverless", vec![], 0.9)
            .is_err()
    );
    let id = ctx
        .add_claim_with_evidence(
            "SQLite is serverless",
            vec![Evidence::new(
                "https://www.sqlite.org/docs.html",
                "SQLite is a serverless database.",
            )],
            0.94,
        )
        .unwrap();
    assert_eq!(ctx.claims[0].id, id);
    assert_eq!(ctx.claims[0].evidence.len(), 1);

    let scored = ResearchContext::score_source("https://docs.rs/tokio", 1.0, 1.0);
    assert_eq!(scored.authority, 1.0);
    let github_scored =
        ResearchContext::score_source("https://github.com/rust-lang/rust", 0.9, 1.0);
    assert_eq!(github_scored.authority, 0.9);

    let json = serde_json::to_value(&ctx).unwrap();
    let back: ResearchContext = serde_json::from_value(json).unwrap();
    assert_eq!(back.query, ctx.query);
    assert_eq!(back.claims.len(), 1);
}

#[test]
fn web_caps_and_type_ids_align() {
    assert_eq!(caps::WEB_SEARCH, tools::SEARCH);
    assert_eq!(caps::WEB_OPEN, tools::OPEN);
    assert_eq!(caps::WEB_FETCH, tools::FETCH);
    assert_eq!(caps::WEB_EXTRACT, tools::EXTRACT);
    assert_eq!(caps::WEB_SCREENSHOT, tools::SCREENSHOT);
    assert_eq!(type_ids::WEB_SEARCH, tools::SEARCH);
    assert_eq!(type_ids::WEB_FETCH, tools::FETCH);
    assert_eq!(
        crate::type_ids::category_for(type_ids::WEB_OPEN),
        crate::NodeCategory::Action
    );

    let mut run = RunContext::new(
        Uuid::now_v7(),
        Uuid::now_v7(),
        1,
        json!({}),
        CapabilityBudget::default()
            .grant(caps::WEB_FETCH)
            .grant(caps::RESEARCH_WRITE),
    );
    run.research = Some(ResearchContext::new("Find best VPS under $20"));
    assert!(run.can(caps::WEB_FETCH));
    assert!(run.research.is_some());
    assert_eq!(ArtifactKind::Document, ArtifactKind::Document);
}

#[test]
fn goal_run_status_and_policy_defaults() {
    use crate::v2::goal_runtime::{GoalRunStatus, PolicyBundle, capability_for_tool};
    assert!(GoalRunStatus::Running.is_active());
    assert!(!GoalRunStatus::Stopped.is_active());
    let p = PolicyBundle::default();
    assert_eq!(p.max_actions_per_day, 50);
    assert!(p.require_approval.contains(&"telegram.write".into()));
    assert_eq!(capability_for_tool("web.search"), caps::WEB_SEARCH);
    assert_eq!(
        capability_for_tool("telegram.user.send_message"),
        caps::TELEGRAM_WRITE
    );
}

#[test]
fn agent_templates_cover_growth_research_monitoring() {
    let all = crate::v2::templates::all();
    assert_eq!(all.len(), 7);
    let growth = crate::v2::templates::get("growth").unwrap();
    assert_eq!(growth.agent_type_id, "agent.growth");
    assert!(!growth.budget.allows(caps::ADS_LAUNCH));
    assert!(growth.budget.allows(caps::WEB_SEARCH));
    assert!(growth.budget.allows(caps::WORKFLOW_EDIT));
    assert!(crate::v2::templates::get("agent.research").is_some());
    let scenario = crate::v2::templates::get("scenario").unwrap();
    assert_eq!(scenario.agent_type_id, "agent.scenario");
    assert!(scenario.budget.allows(caps::WORKFLOW_INVOKE));
    let marketing = crate::v2::templates::get("marketing").unwrap();
    assert_eq!(marketing.agent_type_id, "agent.marketing");
    assert!(!marketing.budget.allows(caps::ADS_LAUNCH));
    assert!(marketing.budget.allows(caps::TELEGRAM_WRITE));
}
