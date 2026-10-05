//! Built-in Goal + Agent templates (Growth / Research / Monitoring).

use super::agent::AgentTool;
use super::capability::{CapabilityBudget, caps};
use super::goal_runtime::{AgentTemplate, PolicyBundle};
use serde_json::json;

fn tool(type_id: &str, title: &str) -> AgentTool {
    AgentTool {
        type_id: type_id.into(),
        title: Some(title.into()),
        config: json!({}),
    }
}

pub fn growth() -> AgentTemplate {
    let budget = CapabilityBudget::default()
        .grant(caps::WEB_SEARCH)
        .grant(caps::WEB_FETCH)
        .grant(caps::AI_GENERATE)
        .grant(caps::AI_ANALYZE)
        .grant(caps::TELEGRAM_WRITE)
        .grant(caps::ARTIFACT_WRITE)
        .grant(caps::RESEARCH_WRITE)
        .grant(caps::WORKFLOW_EDIT)
        .grant(caps::WORKFLOW_INVOKE)
        .forbid(caps::ADS_LAUNCH)
        .forbid(caps::BILLING_WRITE);

    AgentTemplate {
        id: "growth".into(),
        title: "Growth Agent".into(),
        description: "Find interested users: research, test channels, adapt. No ad spend."
            .into(),
        agent_type_id: "agent.growth".into(),
        metric: "leads_interested".into(),
        target: 10.0,
        instructions: "You are BoardDo Growth Agent. Goal: get interested users without spending money. \
Never spam. Never send duplicate outreach. Prefer public research and content. \
Telegram outreach requires approval. Record what worked and pivot away from weak hypotheses."
            .into(),
        tools: vec![
            tool("web.search", "Web Search"),
            tool("web.open", "Read Page"),
            tool("web.extract", "Extract Data"),
            tool("web.fetch", "Web Fetch"),
            tool("ai.chat", "AI Chat"),
            tool("ai.analyze", "AI Analyze"),
            tool("telegram.user.send_message", "Telegram Send"),
            tool("memory.write", "Memory"),
            tool("memory.read", "Read Memory"),
            tool("analytics.record_lead", "Record Lead"),
            tool("debug.log", "Log"),
            tool("workflow.list", "List workflows"),
            tool("workflow.get", "Get workflow"),
            tool("workflow.create", "Create workflow"),
            tool("workflow.update", "Update workflow"),
            tool("workflow.validate", "Validate workflow"),
            tool("workflow.run", "Run workflow"),
            tool("workflow.get_execution", "Get execution"),
            tool("scenario.apply_ops", "Apply graph ops"),
            tool("connection.list", "List connections"),
        ],
        budget,
        policy: PolicyBundle {
            max_actions_per_day: 50,
            budget_usd: 0.0,
            max_daily_spend_usd: 0.0,
            require_approval: vec![caps::TELEGRAM_WRITE.into()],
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            kill_switch: false,
        },
        strategy: super::goal_runtime::default_growth_strategy(),
    }
}

pub fn research() -> AgentTemplate {
    let budget = CapabilityBudget::default()
        .grant(caps::WEB_SEARCH)
        .grant(caps::WEB_FETCH)
        .grant(caps::AI_ANALYZE)
        .grant(caps::AI_GENERATE)
        .grant(caps::ARTIFACT_WRITE)
        .grant(caps::RESEARCH_WRITE)
        .grant(caps::WORKFLOW_EDIT)
        .grant(caps::WORKFLOW_INVOKE)
        .forbid(caps::TELEGRAM_WRITE)
        .forbid(caps::ADS_LAUNCH)
        .forbid(caps::BILLING_WRITE);

    AgentTemplate {
        id: "research".into(),
        title: "Research Agent".into(),
        description: "Monitor a topic: search, fetch sources, summarize, store findings.".into(),
        agent_type_id: "agent.research".into(),
        metric: "sources_reviewed".into(),
        target: 20.0,
        instructions: "You are BoardDo Research Agent. Gather high-quality sources, extract claims, \
note contradictions, and keep a durable research memory. Do not send messages."
            .into(),
        tools: vec![
            tool("web.search", "Web Search"),
            tool("web.open", "Read Page"),
            tool("web.extract", "Extract Data"),
            tool("web.fetch", "Web Fetch"),
            tool("ai.analyze", "AI Analyze"),
            tool("ai.chat", "AI Chat"),
            tool("memory.write", "Memory"),
            tool("memory.read", "Read Memory"),
            tool("debug.log", "Log"),
            tool("workflow.create", "Create workflow"),
            tool("workflow.validate", "Validate workflow"),
            tool("workflow.run", "Run workflow"),
        ],
        budget,
        policy: PolicyBundle {
            max_actions_per_day: 80,
            budget_usd: 0.0,
            max_daily_spend_usd: 0.0,
            require_approval: Vec::new(),
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            kill_switch: false,
        },
        strategy: {
            let mut m = serde_json::Map::new();
            m.insert("search".into(), json!(0.5));
            m.insert("synthesis".into(), json!(0.5));
            m
        },
    }
}

pub fn monitoring() -> AgentTemplate {
    let budget = CapabilityBudget::default()
        .grant(caps::WEB_SEARCH)
        .grant(caps::WEB_FETCH)
        .grant(caps::GITHUB_READ)
        .grant(caps::HTTP_REQUEST)
        .grant(caps::AI_ANALYZE)
        .grant(caps::ARTIFACT_WRITE)
        .grant(caps::WORKFLOW_EDIT)
        .grant(caps::WORKFLOW_INVOKE)
        .forbid(caps::ADS_LAUNCH)
        .forbid(caps::BILLING_WRITE);

    AgentTemplate {
        id: "monitoring".into(),
        title: "Monitoring Agent".into(),
        description: "Watch competitors, GitHub releases, and news. Log deltas.".into(),
        agent_type_id: "agent.monitoring".into(),
        metric: "changes_detected".into(),
        target: 5.0,
        instructions: "You are BoardDo Monitoring Agent. Track the assigned subjects. \
Only record a change when something actually moved. Summarize impact."
            .into(),
        tools: vec![
            tool("web.search", "Web Search"),
            tool("web.open", "Read Page"),
            tool("web.extract", "Extract Data"),
            tool("web.fetch", "Web Fetch"),
            tool("github.get_latest_release", "GitHub Release"),
            tool("http.request", "HTTP"),
            tool("ai.analyze", "AI Analyze"),
            tool("memory.write", "Memory"),
            tool("debug.log", "Log"),
            tool("workflow.create", "Create workflow"),
            tool("workflow.validate", "Validate workflow"),
            tool("workflow.run", "Run workflow"),
        ],
        budget,
        policy: PolicyBundle {
            max_actions_per_day: 40,
            budget_usd: 0.0,
            max_daily_spend_usd: 0.0,
            require_approval: Vec::new(),
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            kill_switch: false,
        },
        strategy: {
            let mut m = serde_json::Map::new();
            m.insert("watch".into(), json!(0.7));
            m.insert("alert".into(), json!(0.3));
            m
        },
    }
}

pub fn scenario() -> AgentTemplate {
    let budget = CapabilityBudget::default()
        .grant(caps::WEB_SEARCH)
        .grant(caps::WEB_FETCH)
        .grant(caps::AI_GENERATE)
        .grant(caps::AI_ANALYZE)
        .grant(caps::HTTP_REQUEST)
        .grant(caps::TELEGRAM_WRITE)
        .grant(caps::ARTIFACT_WRITE)
        .grant(caps::RESEARCH_WRITE)
        .grant(caps::WORKFLOW_EDIT)
        .grant(caps::WORKFLOW_INVOKE)
        .forbid(caps::ADS_LAUNCH)
        .forbid(caps::BILLING_WRITE);

    AgentTemplate {
        id: "scenario".into(),
        title: "Scenario Agent".into(),
        description: "Turn a goal into a BoardDo scenario: plan, build, validate, test, iterate."
            .into(),
        agent_type_id: "agent.scenario".into(),
        metric: "scenarios_validated".into(),
        target: 1.0,
        instructions: "You are BoardDo Scenario Agent. Break the goal into a workflow, \
create or edit the graph, validate it, run a test, store findings, and iterate. \
Do not activate live side-effects (Telegram outreach, production HTTP) without approval. \
Prefer specializing an existing seed graph over inventing a huge canvas."
            .into(),
        tools: vec![
            tool("goal.propose_plan", "Propose plan"),
            tool("goal.materialize_plan", "Materialize plan"),
            tool("workflow.list", "List workflows"),
            tool("workflow.get", "Get workflow"),
            tool("workflow.create", "Create workflow"),
            tool("workflow.update", "Update workflow"),
            tool("workflow.validate", "Validate workflow"),
            tool("workflow.run", "Run workflow"),
            tool("workflow.activate", "Activate workflow"),
            tool("workflow.get_execution", "Get execution"),
            tool("workflow.list_executions", "List executions"),
            tool("scenario.apply_ops", "Apply graph ops"),
            tool("web.search", "Web Search"),
            tool("web.open", "Read Page"),
            tool("web.extract", "Extract Data"),
            tool("web.fetch", "Web Fetch"),
            tool("ai.chat", "AI Chat"),
            tool("ai.analyze", "AI Analyze"),
            tool("http.request", "HTTP"),
            tool("telegram.user.send_message", "Telegram Send"),
            tool("memory.write", "Memory"),
            tool("memory.read", "Read Memory"),
            tool("analytics.record_progress", "Record progress"),
            tool("connection.list", "List connections"),
            tool("connection.test", "Test connection"),
            tool("telegram.accounts.list", "List Telegram accounts"),
            tool("debug.log", "Log"),
        ],
        budget,
        policy: PolicyBundle {
            max_actions_per_day: 120,
            budget_usd: 0.0,
            max_daily_spend_usd: 0.0,
            require_approval: vec![caps::TELEGRAM_WRITE.into(), "workflow.activate".into()],
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            kill_switch: false,
        },
        strategy: {
            let mut m = serde_json::Map::new();
            m.insert("plan".into(), json!(0.3));
            m.insert("build".into(), json!(0.4));
            m.insert("test".into(), json!(0.3));
            m
        },
    }
}

pub fn marketing() -> AgentTemplate {
    let budget = CapabilityBudget::default()
        .grant(caps::WEB_SEARCH)
        .grant(caps::WEB_FETCH)
        .grant(caps::AI_GENERATE)
        .grant(caps::AI_ANALYZE)
        .grant(caps::TELEGRAM_WRITE)
        .grant(caps::ARTIFACT_WRITE)
        .grant(caps::RESEARCH_WRITE)
        .grant(caps::WORKFLOW_EDIT)
        .grant(caps::WORKFLOW_INVOKE)
        .forbid(caps::ADS_LAUNCH)
        .forbid(caps::BILLING_WRITE);

    AgentTemplate {
        id: "marketing".into(),
        title: "Marketing Agent".into(),
        description: "Zero-budget marketing loop: research → hypotheses → content → \
Telegram (approval) → leads → learn. Reuses growth primitives."
            .into(),
        agent_type_id: "agent.marketing".into(),
        metric: "leads_interested".into(),
        target: 10.0,
        instructions: "You are BoardDo Marketing Agent. Goal: promote the product without ad spend. \
Research audience and pains, create 3 hypotheses, draft Telegram content, wait for approval, \
publish, engage inbound messages, score leads, attribute to experiments, and update strategy. \
Never spend money. Never spam. Level-2 autonomy requires approval before Telegram publish."
            .into(),
        tools: vec![
            tool("web.search", "Web Search"),
            tool("web.open", "Read Page"),
            tool("web.extract", "Extract Data"),
            tool("web.fetch", "Web Fetch"),
            tool("research.search", "Research Search"),
            tool("research.fetch", "Research Fetch"),
            tool("research.note", "Research Note"),
            tool("research.competitors", "Competitors"),
            tool("research.audience", "Audience"),
            tool("research.pains", "Pains"),
            tool("strategy.create", "Content Strategy"),
            tool("idea.generate", "Generate Ideas"),
            tool("brief.create", "Content Brief"),
            tool("content.create", "Create Content"),
            tool("content.write", "Write Content"),
            tool("content.create_draft", "Create Draft (legacy)"),
            tool("content.adapt", "Adapt Channel"),
            tool("content.critique", "AI Critique"),
            tool("content.fact_check", "Fact Check"),
            tool("content.rewrite", "Rewrite"),
            tool("content.variant", "Draft Variant"),
            tool("publish.prepare", "Prepare Publish"),
            tool("publish.send", "Publish"),
            tool("publisher.telegram.prepare", "Prepare Telegram Post"),
            tool("publisher.telegram.send", "Send Telegram Post"),
            tool("telegram.publish", "Telegram Publish"),
            tool("insight.create", "Content Insight"),
            tool("analytics.read", "Content Analytics"),
            tool("analytics.query", "Analytics Query"),
            tool("telegram.user.send_message", "Telegram Send"),
            tool("telegram.accounts.list", "List Telegram Accounts"),
            tool("lead.create", "Create Lead"),
            tool("lead.update", "Update Lead"),
            tool("lead.score", "Score Lead"),
            tool("conversation.classify", "Classify Conversation"),
            tool("marketing.simulate_inbound", "Simulate Inbound (Demo)"),
            tool("analytics.campaign_metrics", "Campaign Metrics"),
            tool("analytics.record_lead", "Record Lead Metric"),
            tool("ai.chat", "AI Chat"),
            tool("ai.analyze", "AI Analyze"),
            tool("memory.write", "Memory"),
            tool("memory.save", "Save Memory"),
            tool("memory.read", "Read Memory"),
            tool("python.create", "Create Python Tool"),
            tool("python.validate", "Validate Python Tool"),
            tool("python.run", "Run Python Tool"),
            tool("tool.create", "Create Tool"),
            tool("tool.run", "Run Tool"),
            tool("tool.test", "Test Tool"),
            tool("debug.log", "Log"),
            tool("workflow.list", "List workflows"),
            tool("workflow.get", "Get workflow"),
            tool("workflow.create", "Create workflow"),
            tool("workflow.validate", "Validate workflow"),
            tool("workflow.run", "Run workflow"),
            tool("scenario.apply_ops", "Apply graph ops"),
            tool("connection.list", "List connections"),
            tool("campaign.bootstrap_plan", "Bootstrap Campaign Plan"),
        ],
        budget,
        policy: PolicyBundle {
            max_actions_per_day: 80,
            budget_usd: 0.0,
            max_daily_spend_usd: 0.0,
            require_approval: vec![caps::TELEGRAM_WRITE.into()],
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            kill_switch: false,
        },
        strategy: super::marketing::default_marketing_strategy(),
    }
}

pub fn tool_builder() -> AgentTemplate {
    let budget = CapabilityBudget::default()
        .grant(caps::TOOL_BUILD)
        .grant(caps::TOOL_EXECUTE)
        .grant(caps::AI_GENERATE)
        .grant(caps::AI_ANALYZE)
        .grant(caps::ARTIFACT_WRITE)
        .forbid(caps::TELEGRAM_WRITE)
        .forbid(caps::HTTP_REQUEST);

    AgentTemplate {
        id: "tool_builder".into(),
        title: "AI Tool Builder".into(),
        description: "Create, test, version, and run Python tools in sandbox.".into(),
        agent_type_id: "agent.tool_builder".into(),
        metric: "tools_active".into(),
        target: 1.0,
        instructions: "You are BoardDo Tool Builder. When the goal needs capability missing from the registry, \
search tools first (tool.find / tool.list). Only create_tool if nothing fits. Python tools must expose \
def run(input_data) returning JSON-serializable dict. Use locked-down permissions unless policy allows more. \
Always test_tool before relying on run_tool in scenarios.".into(),
        tools: vec![
            tool("tool.find", "Find tools"),
            tool("tool.list", "List tools"),
            tool("tool.get", "Get tool"),
            tool("tool.create", "Create tool"),
            tool("tool.update", "Update tool"),
            tool("tool.test", "Test tool"),
            tool("tool.run", "Run tool"),
            tool("tool.delete", "Delete tool"),
            tool("memory.write", "Memory"),
            tool("memory.read", "Read memory"),
            tool("debug.log", "Log"),
        ],
        budget,
        policy: PolicyBundle {
            max_actions_per_day: 60,
            budget_usd: 0.0,
            max_daily_spend_usd: 0.0,
            require_approval: vec![],
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            kill_switch: false,
        },
        strategy: {
            let mut m = serde_json::Map::new();
            m.insert("discover".into(), json!(0.2));
            m.insert("build".into(), json!(0.5));
            m.insert("validate".into(), json!(0.3));
            m
        },
    }
}

pub fn content_creator() -> AgentTemplate {
    let mut t = marketing();
    t.id = "content".into();
    t.title = "Content Engine".into();
    t.agent_type_id = "agent.content".into();
    t.description = "Universal content OS: brand → research → strategy → ideas → brief → create → adapt → publish → learn.".into();
    t.instructions = "You are BoardDo Content Engine. Run the full content lifecycle on Goal Runtime. \
Use Brand Profile, audience language from research, briefs before long-form, critique before publish. \
Adapt one source into many channel-specific pieces. All outbound actions via policy + approval.".into();
    t
}

pub fn all() -> Vec<AgentTemplate> {
    vec![
        growth(),
        research(),
        monitoring(),
        scenario(),
        marketing(),
        content_creator(),
        tool_builder(),
    ]
}

pub fn get(id: &str) -> Option<AgentTemplate> {
    all().into_iter().find(|t| t.id == id || t.agent_type_id == id)
}
