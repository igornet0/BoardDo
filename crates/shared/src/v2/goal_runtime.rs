//! Runtime Goal → Agent loop types (distinct from canvas [`super::goal::Goal`] / WorkflowPlan).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::agent::AgentTool;
use super::capability::CapabilityBudget;
use super::goal::ApprovalDecision;
use crate::WorkflowSummary;

/// Safety envelope applied on every Act.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyBundle {
    #[serde(default = "default_max_actions")]
    pub max_actions_per_day: u32,
    #[serde(default)]
    pub budget_usd: f64,
    /// Soft daily spend cap (USD). Spend above this requires approval.
    #[serde(default)]
    pub max_daily_spend_usd: f64,
    /// Capability ids that require human approval before side-effects.
    #[serde(default)]
    pub require_approval: Vec<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
    #[serde(default)]
    pub kill_switch: bool,
}

fn default_max_actions() -> u32 {
    50
}

impl Default for PolicyBundle {
    fn default() -> Self {
        Self {
            max_actions_per_day: default_max_actions(),
            budget_usd: 0.0,
            max_daily_spend_usd: 0.0,
            require_approval: vec!["telegram.write".into()],
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            kill_switch: false,
        }
    }
}

/// What the user wants to achieve. Durable definition; runs are separate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalSpec {
    pub id: Uuid,
    pub title: String,
    pub text: String,
    /// Metric key, e.g. `leads_interested`.
    pub metric: String,
    pub target: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub constraints: Value,
    #[serde(default)]
    pub budget: CapabilityBudget,
    #[serde(default)]
    pub policy: PolicyBundle,
    /// Palette / template id, e.g. `agent.growth`.
    pub agent_type_id: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub tools: Vec<AgentTool>,
    /// Seconds between agent ticks while running.
    #[serde(default = "default_tick_interval")]
    pub tick_interval_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_connection_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_tick_interval() -> u64 {
    60
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalRunStatus {
    Draft,
    Running,
    Paused,
    WaitingApproval,
    Succeeded,
    Failed,
    Stopped,
}

impl GoalRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::WaitingApproval => "waiting_approval",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "paused" => Self::Paused,
            "waiting_approval" => Self::WaitingApproval,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "stopped" => Self::Stopped,
            _ => Self::Draft,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::WaitingApproval)
    }
}

/// Live instance of a [`GoalSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalRun {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub status: GoalRunStatus,
    pub current: f64,
    pub target: f64,
    /// Strategy channel → relative weight 0..1.
    #[serde(default)]
    pub strategy: Map<String, Value>,
    pub actions_today: u32,
    /// Calendar day (`YYYY-MM-DD`) the counter applies to.
    pub actions_day: String,
    pub tick_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tick_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_tick_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observe: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_think: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentDecision {
    Pending,
    Continue,
    Pivot,
    Stop,
}

impl ExperimentDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Continue => "continue",
            Self::Pivot => "pivot",
            Self::Stop => "stop",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "continue" => Self::Continue,
            "pivot" => Self::Pivot,
            "stop" => Self::Stop,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Experiment {
    pub id: Uuid,
    pub run_id: Uuid,
    pub number: u32,
    pub hypothesis: String,
    #[serde(default)]
    pub actions: Value,
    #[serde(default)]
    pub metrics: Value,
    pub decision: ExperimentDecision,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedToolCall {
    pub id: String,
    pub type_id: String,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickPlan {
    #[serde(default)]
    pub observe_notes: String,
    #[serde(default)]
    pub think: String,
    #[serde(default)]
    pub tool_calls: Vec<PlannedToolCall>,
    #[serde(default)]
    pub experiment_updates: Vec<ExperimentUpdate>,
    #[serde(default)]
    pub strategy_deltas: Map<String, Value>,
    #[serde(default)]
    pub new_hypotheses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperimentUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<ExperimentDecision>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMemoryEntry {
    pub id: Uuid,
    pub run_id: Uuid,
    pub key: String,
    pub value: Value,
    /// `short` or `long`.
    pub kind: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentAuditEntry {
    pub id: Uuid,
    pub run_id: Uuid,
    pub at: DateTime<Utc>,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentApproval {
    pub id: Uuid,
    pub run_id: Uuid,
    pub goal_id: Uuid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub capability: String,
    pub tool_type_id: String,
    #[serde(default)]
    pub payload: Value,
    pub status: ApprovalDecision,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GoalRuntimeEvent {
    TickStarted {
        run_id: Uuid,
        goal_id: Uuid,
        tick: u32,
    },
    Observe {
        run_id: Uuid,
        notes: String,
    },
    Think {
        run_id: Uuid,
        summary: String,
    },
    Plan {
        run_id: Uuid,
        tool_count: u32,
    },
    Act {
        run_id: Uuid,
        tool_type: String,
        ok: bool,
        message: String,
    },
    Evaluate {
        run_id: Uuid,
        current: f64,
        target: f64,
        status: GoalRunStatus,
    },
    ApprovalRequested {
        run_id: Uuid,
        approval_id: Uuid,
        title: String,
    },
    LeadRecorded {
        run_id: Uuid,
        current: f64,
    },
    StatusChanged {
        run_id: Uuid,
        status: GoalRunStatus,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateGoalRequest {
    pub title: String,
    pub text: String,
    #[serde(default = "default_metric")]
    pub metric: String,
    #[serde(default = "default_target")]
    pub target: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub constraints: Value,
    #[serde(default)]
    pub budget: CapabilityBudget,
    #[serde(default)]
    pub policy: PolicyBundle,
    #[serde(default = "default_agent_type")]
    pub agent_type_id: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub tools: Vec<AgentTool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_interval_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_connection_id: Option<Uuid>,
    /// If set, missing fields are filled from the named template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
}

fn default_metric() -> String {
    "leads_interested".into()
}
fn default_target() -> f64 {
    10.0
}
fn default_agent_type() -> String {
    "agent.growth".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateGoalRequest {
    pub title: String,
    pub text: String,
    pub metric: String,
    pub target: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub constraints: Value,
    #[serde(default)]
    pub budget: CapabilityBudget,
    #[serde(default)]
    pub policy: PolicyBundle,
    pub agent_type_id: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub tools: Vec<AgentTool>,
    #[serde(default)]
    pub tick_interval_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_connection_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartGoalRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_connection_id: Option<Uuid>,
}

/// Product-first campaign bootstrap (maps onto GoalSpec + marketing template).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateCampaignRequest {
    pub product_name: String,
    #[serde(default)]
    pub product_description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_url: Option<String>,
    #[serde(default = "default_campaign_metric")]
    pub goal_metric: String,
    #[serde(default = "default_campaign_target")]
    pub target: f64,
    #[serde(default)]
    pub budget_usd: f64,
    #[serde(default = "default_market")]
    pub market: String,
    #[serde(default = "default_deadline_days")]
    pub deadline_days: u32,
    /// `assisted` | `autonomous` | `full_autonomous` (also accepts level 1–4).
    #[serde(default = "default_autonomy_mode")]
    pub autonomy: String,
    #[serde(default)]
    pub demo: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_connection_id: Option<Uuid>,
}

fn default_campaign_metric() -> String {
    "leads_interested".into()
}
fn default_campaign_target() -> f64 {
    1000.0
}
fn default_market() -> String {
    "global".into()
}
fn default_deadline_days() -> u32 {
    30
}
fn default_autonomy_mode() -> String {
    "assisted".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignCreatedResponse {
    pub spec: GoalSpec,
    pub run: GoalRun,
    pub plan: super::marketing::CampaignPlan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordGoalEventRequest {
    /// e.g. `lead.interested`
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalDetail {
    pub spec: GoalSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<GoalRun>,
    /// Workflows this GoalRun created or linked for scenario autonomy.
    #[serde(default)]
    pub linked_workflows: Vec<WorkflowSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub id: String,
    pub title: String,
    pub description: String,
    pub agent_type_id: String,
    pub metric: String,
    pub target: f64,
    pub instructions: String,
    pub tools: Vec<AgentTool>,
    pub budget: CapabilityBudget,
    pub policy: PolicyBundle,
    #[serde(default)]
    pub strategy: Map<String, Value>,
}

pub fn default_growth_strategy() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("community".into(), Value::from(0.4));
    m.insert("content".into(), Value::from(0.3));
    m.insert("outreach".into(), Value::from(0.1));
    m.insert("search".into(), Value::from(0.2));
    m
}

/// Resolve alias tool ids to canonical implementations.
pub fn resolve_tool_alias(type_id: &str) -> &str {
    match type_id {
        "content.write" => "content.create",
        "telegram.publish" => "publisher.telegram.send",
        "python.create" | "create_tool" => "tool.create",
        "python.run" | "run_tool" => "tool.run",
        "python.validate" | "test_tool" => "tool.test",
        "memory.save" => "memory.write",
        "analytics.query" => "analytics.campaign_metrics",
        other => other,
    }
}

pub fn capability_for_tool(type_id: &str) -> &'static str {
    let type_id = resolve_tool_alias(type_id);
    if let Some(cap) = super::custom_tool::custom_tool_capability(type_id) {
        return cap;
    }
    if let Some(cap) = super::content::content_capability_for_tool(type_id) {
        return cap;
    }
    if let Some(cap) = super::marketing::marketing_capability_for_tool(type_id) {
        return cap;
    }
    match type_id {
        "web.search" => super::capability::caps::WEB_SEARCH,
        "web.fetch" | "web.open" | "web.extract" => super::capability::caps::WEB_FETCH,
        "http.request" => super::capability::caps::HTTP_REQUEST,
        "ai.chat" | "ai.generate" | "ai.image" | "ai.audio" | "ai.video" => {
            super::capability::caps::AI_GENERATE
        }
        "ai.analyze" | "ai.classify" => super::capability::caps::AI_ANALYZE,
        "telegram.send_message"
        | "telegram.send_photo"
        | "telegram.send_document"
        | "telegram.user.send_message"
        | "publisher.telegram.send"
        | "publisher.telegram.prepare" => super::capability::caps::TELEGRAM_WRITE,
        "github.get_latest_release" => super::capability::caps::GITHUB_READ,
        "debug.log" | "memory.write" | "memory.read" => super::capability::caps::ARTIFACT_WRITE,
        "analytics.record_lead" | "analytics.record_progress" | "analytics.campaign_metrics"
        | "analytics.read" => super::capability::caps::RESEARCH_WRITE,
        "workflow.list"
        | "workflow.get"
        | "workflow.create"
        | "workflow.update"
        | "workflow.validate"
        | "scenario.apply_ops"
        | "goal.propose_plan"
        | "goal.materialize_plan"
        | "campaign.bootstrap_plan" => super::capability::caps::WORKFLOW_EDIT,
        "workflow.run"
        | "workflow.activate"
        | "workflow.get_execution"
        | "workflow.list_executions" => super::capability::caps::WORKFLOW_INVOKE,
        "connection.list" | "connection.test" | "telegram.accounts.list" => {
            super::capability::caps::ARTIFACT_WRITE
        }
        _ => super::capability::caps::HTTP_REQUEST,
    }
}

/// Tools that inspect state and should not consume the daily action budget.
pub fn tool_counts_as_action(type_id: &str) -> bool {
    let type_id = resolve_tool_alias(type_id);
    !matches!(
        type_id,
        "workflow.list"
            | "workflow.get"
            | "workflow.validate"
            | "workflow.get_execution"
            | "workflow.list_executions"
            | "memory.read"
            | "connection.list"
            | "connection.test"
            | "telegram.accounts.list"
            | "goal.propose_plan"
            | "campaign.bootstrap_plan"
            | "research.fetch"
            | "analytics.campaign_metrics"
            | "analytics.read"
            | "analytics.query"
            | "lead.score"
            | "conversation.classify"
    )
}
