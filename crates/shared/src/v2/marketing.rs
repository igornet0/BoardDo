//! Marketing Goal loop primitives — Lead, research notes, drafts, conversations.
//!
//! Built on top of the shared Goal Runtime (GoalSpec / GoalRun / Experiment /
//! PolicyBundle / approvals). This is an application template, not a second runtime.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

/// 1 = Observe, 2 = Draft (default MVP), 3 = Execute (allowlisted), 4 = Autonomous (prepared).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutonomyLevel(pub u8);

impl Default for AutonomyLevel {
    fn default() -> Self {
        Self(2)
    }
}

impl AutonomyLevel {
    pub fn clamp(self) -> Self {
        Self(self.0.clamp(1, 4))
    }

    pub fn allows_publish_without_approval(self) -> bool {
        self.0 >= 3
    }

    pub fn allows_outbound(self) -> bool {
        self.0 >= 2
    }

    pub fn observe_only(self) -> bool {
        self.0 <= 1
    }

    /// Assisted=2, Autonomous=3, Full Autonomous=4.
    pub fn from_mode(mode: &str) -> Self {
        match mode.trim().to_lowercase().as_str() {
            "observe" | "1" => Self(1),
            "assisted" | "draft" | "2" => Self(2),
            "autonomous" | "execute" | "3" => Self(3),
            "full_autonomous" | "full" | "4" => Self(4),
            _ => Self(2),
        }
    }

    pub fn mode_label(self) -> &'static str {
        match self.0 {
            1 => "observe",
            3 => "autonomous",
            4 => "full_autonomous",
            _ => "assisted",
        }
    }

    /// Caps that always need approval even at level 4.
    pub fn full_autonomous_require_approval() -> Vec<String> {
        vec![
            "payments".into(),
            "account.create".into(),
            "data.delete".into(),
            super::capability::caps::ADS_LAUNCH.into(),
            super::capability::caps::BILLING_WRITE.into(),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketingStage {
    Research,
    Audience,
    Hypotheses,
    Content,
    Approval,
    Publish,
    Engage,
    Lead,
    Analytics,
    Learn,
}

impl MarketingStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Research => "research",
            Self::Audience => "audience",
            Self::Hypotheses => "hypotheses",
            Self::Content => "content",
            Self::Approval => "approval",
            Self::Publish => "publish",
            Self::Engage => "engage",
            Self::Lead => "lead",
            Self::Analytics => "analytics",
            Self::Learn => "learn",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "audience" => Self::Audience,
            "hypotheses" => Self::Hypotheses,
            "content" => Self::Content,
            "approval" => Self::Approval,
            "publish" => Self::Publish,
            "engage" => Self::Engage,
            "lead" => Self::Lead,
            "analytics" => Self::Analytics,
            "learn" => Self::Learn,
            _ => Self::Research,
        }
    }

    pub fn all() -> &'static [MarketingStage] {
        &[
            Self::Research,
            Self::Audience,
            Self::Hypotheses,
            Self::Content,
            Self::Approval,
            Self::Publish,
            Self::Engage,
            Self::Lead,
            Self::Analytics,
            Self::Learn,
        ]
    }

    pub fn next(self) -> Self {
        match self {
            Self::Research => Self::Audience,
            Self::Audience => Self::Hypotheses,
            Self::Hypotheses => Self::Content,
            Self::Content => Self::Approval,
            Self::Approval => Self::Publish,
            Self::Publish => Self::Engage,
            Self::Engage => Self::Lead,
            Self::Lead => Self::Analytics,
            Self::Analytics => Self::Learn,
            Self::Learn => Self::Research,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeadStage {
    Cold,
    Interested,
    Qualified,
    Converted,
    Lost,
}

impl LeadStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Interested => "interested",
            Self::Qualified => "qualified",
            Self::Converted => "converted",
            Self::Lost => "lost",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "interested" => Self::Interested,
            "qualified" => Self::Qualified,
            "converted" => Self::Converted,
            "lost" => Self::Lost,
            _ => Self::Cold,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lead {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub stage: LeadStage,
    pub score: i32,
    #[serde(default)]
    pub score_reasons: Vec<String>,
    pub first_seen_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    /// Simulated demo leads must never mix into production metrics.
    #[serde(default)]
    pub simulated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchNoteType {
    Competitor,
    Audience,
    Pain,
    Objection,
    Offer,
    Trend,
    MarketSignal,
    ContentPattern,
}

impl ResearchNoteType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Competitor => "competitor",
            Self::Audience => "audience",
            Self::Pain => "pain",
            Self::Objection => "objection",
            Self::Offer => "offer",
            Self::Trend => "trend",
            Self::MarketSignal => "market_signal",
            Self::ContentPattern => "content_pattern",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "competitor" => Self::Competitor,
            "audience" => Self::Audience,
            "pain" => Self::Pain,
            "objection" => Self::Objection,
            "offer" => Self::Offer,
            "trend" => Self::Trend,
            "market_signal" => Self::MarketSignal,
            "content_pattern" => Self::ContentPattern,
            _ => Self::MarketSignal,
        }
    }
}

/// Structured research signal (never raw HTML).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchNote {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub note_type: ResearchNoteType,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_title: Option<String>,
    #[serde(default)]
    pub confidence: f32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentDraftStatus {
    Draft,
    PendingApproval,
    Approved,
    Published,
    Rejected,
}

impl ContentDraftStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::PendingApproval => "pending_approval",
            Self::Approved => "approved",
            Self::Published => "published",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "pending_approval" => Self::PendingApproval,
            "approved" => Self::Approved,
            "published" => Self::Published,
            "rejected" => Self::Rejected,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentDraft {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    pub channel: String,
    pub body: String,
    pub status: ContentDraftStatus,
    /// Idempotency key: goal:campaign:experiment:content
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub simulated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationIntent {
    Question,
    Interested,
    Pricing,
    Objection,
    NotInterested,
    Spam,
    Unknown,
    Buy,
    Pain,
}

impl ConversationIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Interested => "interested",
            Self::Pricing => "pricing",
            Self::Objection => "objection",
            Self::NotInterested => "not_interested",
            Self::Spam => "spam",
            Self::Unknown => "unknown",
            Self::Buy => "buy",
            Self::Pain => "pain",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "question" => Self::Question,
            "interested" => Self::Interested,
            "pricing" => Self::Pricing,
            "objection" => Self::Objection,
            "not_interested" => Self::NotInterested,
            "spam" => Self::Spam,
            "buy" => Self::Buy,
            "pain" => Self::Pain,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub text: String,
    pub at: DateTime<Utc>,
    #[serde(default)]
    pub simulated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketingConversation {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    pub chat_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub intent: ConversationIntent,
    #[serde(default)]
    pub messages: Vec<ConversationMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub simulated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    pub observation: String,
    pub action: String,
    #[serde(default)]
    pub confidence: f32,
    pub created_at: DateTime<Utc>,
}

/// Durable in-campaign learning (append-only via memory).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Learning {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    pub observation: String,
    pub decision: String,
    pub reason: String,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignProduct {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// User-facing product brief (also stored under GoalSpec.constraints).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignBrief {
    pub product: CampaignProduct,
    #[serde(default = "default_goal_metric")]
    pub goal_metric: String,
    #[serde(default = "default_brief_target")]
    pub target: f64,
    #[serde(default)]
    pub budget_usd: f64,
    #[serde(default)]
    pub market: String,
    #[serde(default = "default_deadline_days_brief")]
    pub deadline_days: u32,
    /// assisted | autonomous | full_autonomous
    #[serde(default = "default_autonomy_str")]
    pub autonomy: String,
}

fn default_goal_metric() -> String {
    "leads_interested".into()
}
fn default_brief_target() -> f64 {
    1000.0
}
fn default_deadline_days_brief() -> u32 {
    30
}
fn default_autonomy_str() -> String {
    "assisted".into()
}

/// Pre-start AI Plan shown to the user before Start Campaign.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignPlan {
    pub summary: String,
    #[serde(default)]
    pub competitors: Vec<String>,
    #[serde(default)]
    pub segments: Vec<String>,
    #[serde(default)]
    pub pain_points: Vec<String>,
    #[serde(default)]
    pub content_opportunities: Vec<String>,
    #[serde(default)]
    pub acquisition_channels: Vec<String>,
    #[serde(default)]
    pub strategy_outline: String,
    #[serde(default)]
    pub content_tasks: Vec<String>,
    #[serde(default)]
    pub experiments: Vec<String>,
    #[serde(default)]
    pub lead_funnels: Vec<String>,
    #[serde(default)]
    pub sales_sequences: Vec<String>,
    #[serde(default)]
    pub estimated_workload: String,
    pub ready: bool,
    pub created_at: DateTime<Utc>,
}

impl CampaignPlan {
    pub fn bootstrap_from_brief(brief: &CampaignBrief) -> Self {
        let product = &brief.product.name;
        let market = if brief.market.is_empty() {
            "global"
        } else {
            brief.market.as_str()
        };
        Self {
            summary: format!(
                "I analyzed {product} for {market}. Ready to run a {}-day autonomous marketing loop toward {:.0} {}.",
                brief.deadline_days, brief.target, brief.goal_metric
            ),
            competitors: vec![
                format!("{product} alternative A (feature parity)"),
                format!("{product} alternative B (price leader)"),
                format!("Incumbent spreadsheet / manual workflow"),
                format!("Niche local competitor in {market}"),
            ],
            segments: vec![
                format!("Early adopters in {market} seeking automation"),
                "Operators drowning in manual lead processing".into(),
                "Small teams needing a single growth control plane".into(),
            ],
            pain_points: vec![
                "Too many tools glued together manually".into(),
                "No closed-loop feedback from posts to leads".into(),
                "Content produced without experiments".into(),
                "Unclear positioning vs incumbents".into(),
                "Slow follow-up on inbound interest".into(),
                "No durable campaign memory / learnings".into(),
                "Channel mix decided by gut feel".into(),
            ],
            content_opportunities: vec![
                "Problem-first posts on Telegram".into(),
                "Case-study style before/after threads".into(),
                "Short product demos / walkthroughs".into(),
                "Objection-handling FAQs".into(),
                "Market-trend commentary for {market}".replace("{market}", market),
                "Competitor comparison angles".into(),
                "Lead-magnet checklists".into(),
                "User-story micro-content".into(),
                "CTA experiments (reply INTERESTED)".into(),
                "Educational how-to series".into(),
                "Founder-voice notes".into(),
                "Weekly learnings recap".into(),
            ],
            acquisition_channels: vec![
                "Telegram".into(),
                "Web / landing".into(),
                "Organic search content".into(),
                "Community outreach".into(),
                "Referral / word of mouth".into(),
            ],
            strategy_outline: format!(
                "30-day loop for {product}: research → positioning → Telegram experiments → lead capture → analyze → pivot. Budget ${:.0}.",
                brief.budget_usd
            ),
            content_tasks: (1..=12)
                .map(|i| format!("Content task #{i}: draft Telegram variant for experiment angle"))
                .collect(),
            experiments: vec![
                "Problem vs product-description hook".into(),
                "Time-saving vs more-leads angle".into(),
                "Case study vs expert post".into(),
                "Short CTA vs long educational".into(),
                "Market-local language vs English".into(),
                "Offer-led vs pain-led".into(),
                "Social proof vs feature list".into(),
                "Morning vs evening publish window".into(),
            ],
            lead_funnels: vec![
                "Telegram reply → qualify → demo".into(),
                "Landing capture → nurture sequence".into(),
                "Lead magnet checklist → follow-up".into(),
            ],
            sales_sequences: vec![
                "Interested → pricing FAQ → soft offer".into(),
                "Objection → case study → revisit CTA".into(),
            ],
            estimated_workload: "Autonomous".into(),
            ready: true,
            created_at: Utc::now(),
        }
    }
}

impl Default for CampaignPlan {
    fn default() -> Self {
        Self {
            summary: String::new(),
            competitors: Vec::new(),
            segments: Vec::new(),
            pain_points: Vec::new(),
            content_opportunities: Vec::new(),
            acquisition_channels: Vec::new(),
            strategy_outline: String::new(),
            content_tasks: Vec::new(),
            experiments: Vec::new(),
            lead_funnels: Vec::new(),
            sales_sequences: Vec::new(),
            estimated_workload: "Autonomous".into(),
            ready: false,
            created_at: Utc::now(),
        }
    }
}

/// Campaign memory shape (serialized into agent_memory keys).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CampaignMemory {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<CampaignBrief>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<CampaignPlan>,
    #[serde(default)]
    pub audience_segments: Vec<String>,
    #[serde(default)]
    pub positioning: String,
    #[serde(default)]
    pub messages: Vec<String>,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub learnings: Vec<Learning>,
    #[serde(default)]
    pub metrics: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperimentMetrics {
    #[serde(default)]
    pub views: u32,
    #[serde(default)]
    pub reactions: u32,
    #[serde(default)]
    pub replies: u32,
    #[serde(default)]
    pub leads: u32,
    #[serde(default)]
    pub qualified_leads: u32,
    #[serde(default)]
    pub conversions: u32,
    #[serde(default)]
    pub posts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub channel: String,
}

impl Default for ExperimentMetrics {
    fn default() -> Self {
        Self {
            views: 0,
            reactions: 0,
            replies: 0,
            leads: 0,
            qualified_leads: 0,
            conversions: 0,
            posts: 0,
            published_at: None,
            channel: "telegram".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketingFunnel {
    pub research_notes: u32,
    pub audience_notes: u32,
    pub pain_notes: u32,
    pub hypotheses: u32,
    pub drafts: u32,
    pub pending_approval: u32,
    pub published: u32,
    pub conversations: u32,
    pub leads: u32,
    pub interested: u32,
    pub qualified: u32,
    pub conversions: u32,
}

impl Default for MarketingFunnel {
    fn default() -> Self {
        Self {
            research_notes: 0,
            audience_notes: 0,
            pain_notes: 0,
            hypotheses: 0,
            drafts: 0,
            pending_approval: 0,
            published: 0,
            conversations: 0,
            leads: 0,
            interested: 0,
            qualified: 0,
            conversions: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketingSnapshot {
    pub stage: MarketingStage,
    pub autonomy_level: AutonomyLevel,
    #[serde(default)]
    pub autonomy_mode: String,
    pub funnel: MarketingFunnel,
    #[serde(default)]
    pub leads: Vec<Lead>,
    #[serde(default)]
    pub research_notes: Vec<ResearchNote>,
    #[serde(default)]
    pub drafts: Vec<ContentDraft>,
    #[serde(default)]
    pub conversations: Vec<MarketingConversation>,
    #[serde(default)]
    pub strategy_deltas: Vec<StrategyDelta>,
    #[serde(default)]
    pub stage_progress: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<CampaignPlan>,
    #[serde(default)]
    pub learnings: Vec<Learning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_objective: Option<String>,
    #[serde(default)]
    pub channel_weights: Map<String, Value>,
    #[serde(default)]
    pub plan_ready: bool,
}

/// Deterministic lead score from conversation intents / text signals.
pub fn score_lead_signals(signals: &[ConversationIntent]) -> (i32, Vec<String>) {
    let mut score = 0i32;
    let mut reasons = Vec::new();
    for s in signals {
        let (delta, reason) = match s {
            ConversationIntent::Question => (20, "+20 product question"),
            ConversationIntent::Interested => (20, "+20 product interest"),
            ConversationIntent::Pricing => (25, "+25 pricing question"),
            ConversationIntent::Pain => (30, "+30 relevant pain"),
            ConversationIntent::Buy => (50, "+50 wants product / how to buy"),
            ConversationIntent::Objection => (10, "+10 objection (engaged)"),
            ConversationIntent::NotInterested => (-20, "-20 not interested"),
            ConversationIntent::Spam => (-30, "-30 spam"),
            ConversationIntent::Unknown => (0, "0 unknown"),
        };
        if delta != 0 || *s == ConversationIntent::Unknown {
            score += delta;
            reasons.push(reason.into());
        }
    }
    (score.clamp(0, 100), reasons)
}

pub fn classify_intent(text: &str) -> ConversationIntent {
    let t = text.to_lowercase();
    if ["spam", "crypto airdrop", "follow me"].iter().any(|m| t.contains(m)) {
        return ConversationIntent::Spam;
    }
    if ["not interested", "stop", "unsubscribe", "не интересно"].iter().any(|m| t.contains(m))
    {
        return ConversationIntent::NotInterested;
    }
    if ["how to buy", "purchase", "sign up", "get started", "хочу купить", "оформить"]
        .iter()
        .any(|m| t.contains(m))
    {
        return ConversationIntent::Buy;
    }
    if ["price", "pricing", "cost", "сколько", "цена"].iter().any(|m| t.contains(m)) {
        return ConversationIntent::Pricing;
    }
    if ["pain", "manual", "waste time", "tedious", "время", "вручную"].iter().any(|m| t.contains(m))
    {
        return ConversationIntent::Pain;
    }
    if ["interested", "demo", "try", "хочу", "интересно", "+1"].iter().any(|m| t.contains(m)) {
        return ConversationIntent::Interested;
    }
    if t.contains('?') || ["what", "how", "why", "как", "что"].iter().any(|m| t.contains(m)) {
        return ConversationIntent::Question;
    }
    ConversationIntent::Unknown
}

pub fn stage_from_score(score: i32, intent: ConversationIntent) -> LeadStage {
    if matches!(intent, ConversationIntent::Spam | ConversationIntent::NotInterested) {
        return LeadStage::Lost;
    }
    if score >= 70 || matches!(intent, ConversationIntent::Buy) {
        return LeadStage::Qualified;
    }
    if score >= 40 || matches!(intent, ConversationIntent::Interested | ConversationIntent::Pricing)
    {
        return LeadStage::Interested;
    }
    LeadStage::Cold
}

pub fn draft_idempotency_key(
    goal_id: Uuid,
    campaign_id: Option<Uuid>,
    experiment_id: Option<Uuid>,
    content_id: Uuid,
) -> String {
    format!(
        "{}:{}:{}:{}",
        goal_id,
        campaign_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "none".into()),
        experiment_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "none".into()),
        content_id
    )
}

pub fn default_marketing_strategy() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("research".into(), Value::from(0.25));
    m.insert("content".into(), Value::from(0.35));
    m.insert("telegram".into(), Value::from(0.25));
    m.insert("engage".into(), Value::from(0.15));
    m.insert("time_saving".into(), Value::from(0.34));
    m.insert("more_leads".into(), Value::from(0.33));
    m.insert("manual_work".into(), Value::from(0.33));
    m
}

pub fn product_from_constraints(constraints: &Value) -> String {
    if let Some(obj) = constraints.get("product") {
        if let Some(name) = obj.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            return name.to_string();
        }
        if let Some(s) = obj.as_str().filter(|s| !s.is_empty()) {
            return s.to_string();
        }
    }
    constraints
        .get("product_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("BoardDo")
        .to_string()
}

pub fn product_url_from_constraints(constraints: &Value) -> Option<String> {
    constraints
        .get("product")
        .and_then(|p| p.get("url"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            constraints
                .get("product_url")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

pub fn product_description_from_constraints(constraints: &Value) -> String {
    constraints
        .get("product")
        .and_then(|p| p.get("description"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| constraints.get("product_description").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

pub fn market_from_constraints(constraints: &Value) -> String {
    constraints
        .get("market")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("global")
        .to_string()
}

pub fn autonomy_from_constraints(constraints: &Value) -> AutonomyLevel {
    if let Some(mode) = constraints.get("autonomy").and_then(Value::as_str) {
        return AutonomyLevel::from_mode(mode);
    }
    let level = constraints
        .get("autonomy_level")
        .and_then(Value::as_u64)
        .or_else(|| {
            constraints
                .get("autonomy_level")
                .and_then(Value::as_i64)
                .map(|n| n as u64)
        })
        .unwrap_or(2) as u8;
    AutonomyLevel(level).clamp()
}

pub fn brief_from_constraints(constraints: &Value) -> CampaignBrief {
    CampaignBrief {
        product: CampaignProduct {
            name: product_from_constraints(constraints),
            description: product_description_from_constraints(constraints),
            url: product_url_from_constraints(constraints),
        },
        goal_metric: constraints
            .get("goal_metric")
            .and_then(Value::as_str)
            .unwrap_or("leads_interested")
            .to_string(),
        target: constraints
            .get("target")
            .and_then(Value::as_f64)
            .unwrap_or(1000.0),
        budget_usd: constraints
            .get("budget_usd")
            .and_then(Value::as_f64)
            .or_else(|| constraints.get("budget").and_then(Value::as_f64))
            .unwrap_or(0.0),
        market: market_from_constraints(constraints),
        deadline_days: constraints
            .get("deadline_days")
            .and_then(Value::as_u64)
            .unwrap_or(30) as u32,
        autonomy: autonomy_from_constraints(constraints).mode_label().into(),
    }
}

/// Tools forbidden at autonomy level 1 (observe-only).
pub fn is_outbound_tool(type_id: &str) -> bool {
    let type_id = super::goal_runtime::resolve_tool_alias(type_id);
    super::content::is_content_outbound_tool(type_id)
        || matches!(
            type_id,
            "telegram.send_message"
                | "telegram.send_photo"
                | "telegram.send_document"
                | "telegram.user.send_message"
                | "publisher.telegram.send"
                | "publisher.telegram.prepare"
                | "publish.send"
        )
}

pub fn marketing_capability_for_tool(type_id: &str) -> Option<&'static str> {
    let type_id = super::goal_runtime::resolve_tool_alias(type_id);
    match type_id {
        "research.search" | "research.fetch" | "research.note" | "research.competitors"
        | "research.audience" | "research.pains" => Some(super::capability::caps::RESEARCH_WRITE),
        "content.create_draft" | "content.rewrite" | "content.variant" | "content.create" => None,
        "lead.create" | "lead.update" | "lead.score" | "conversation.classify"
        | "marketing.simulate_inbound" | "analytics.campaign_metrics" => {
            Some(super::capability::caps::RESEARCH_WRITE)
        }
        _ => None,
    }
}
