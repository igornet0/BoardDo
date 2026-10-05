//! Universal Content Engine — application layer on Goal Runtime primitives.
//!
//! Not a separate runtime: projects, brand, graph, versions, channels, and lifecycle
//! hang off Goal / Run / Artifact / Policy / Approval like marketing MVP.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

/// Supported content formats (extensible without runtime changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentFormatType {
    ShortPost,
    LongPost,
    Article,
    Thread,
    Advertisement,
    ProductDescription,
    LandingPage,
    Email,
    Newsletter,
    TelegramPost,
    SocialPost,
    VideoScript,
    ShortVideoScript,
    YoutubeScript,
    PodcastScript,
    ImagePrompt,
    Carousel,
    Story,
    CaseStudy,
    Tutorial,
    Review,
    Comparison,
    Announcement,
    PressRelease,
}

impl ContentFormatType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShortPost => "short_post",
            Self::LongPost => "long_post",
            Self::Article => "article",
            Self::Thread => "thread",
            Self::Advertisement => "advertisement",
            Self::ProductDescription => "product_description",
            Self::LandingPage => "landing_page",
            Self::Email => "email",
            Self::Newsletter => "newsletter",
            Self::TelegramPost => "telegram_post",
            Self::SocialPost => "social_post",
            Self::VideoScript => "video_script",
            Self::ShortVideoScript => "short_video_script",
            Self::YoutubeScript => "youtube_script",
            Self::PodcastScript => "podcast_script",
            Self::ImagePrompt => "image_prompt",
            Self::Carousel => "carousel",
            Self::Story => "story",
            Self::CaseStudy => "case_study",
            Self::Tutorial => "tutorial",
            Self::Review => "review",
            Self::Comparison => "comparison",
            Self::Announcement => "announcement",
            Self::PressRelease => "press_release",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "long_post" => Self::LongPost,
            "article" => Self::Article,
            "thread" => Self::Thread,
            "telegram_post" | "telegram" => Self::TelegramPost,
            "email" => Self::Email,
            "blog" | "blog_post" => Self::Article,
            "short_video_script" => Self::ShortVideoScript,
            "video_script" => Self::VideoScript,
            "x_thread" => Self::Thread,
            "social_post" => Self::SocialPost,
            _ => Self::ShortPost,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentChannelId {
    Telegram,
    Website,
    Blog,
    X,
    Reddit,
    Instagram,
    LinkedIn,
    Youtube,
    Email,
    Custom,
}

impl ContentChannelId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Website => "website",
            Self::Blog => "blog",
            Self::X => "x",
            Self::Reddit => "reddit",
            Self::Instagram => "instagram",
            Self::LinkedIn => "linkedin",
            Self::Youtube => "youtube",
            Self::Email => "email",
            Self::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "telegram" => Self::Telegram,
            "website" => Self::Website,
            "blog" => Self::Blog,
            "x" | "twitter" => Self::X,
            "reddit" => Self::Reddit,
            "instagram" => Self::Instagram,
            "linkedin" => Self::LinkedIn,
            "youtube" => Self::Youtube,
            "email" => Self::Email,
            _ => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentLifecycle {
    Idea,
    Brief,
    Draft,
    Review,
    Approved,
    Scheduled,
    Published,
    Analyzed,
    Archived,
}

impl ContentLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idea => "idea",
            Self::Brief => "brief",
            Self::Draft => "draft",
            Self::Review => "review",
            Self::Approved => "approved",
            Self::Scheduled => "scheduled",
            Self::Published => "published",
            Self::Analyzed => "analyzed",
            Self::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "brief" => Self::Brief,
            "review" => Self::Review,
            "approved" => Self::Approved,
            "scheduled" => Self::Scheduled,
            "published" => Self::Published,
            "analyzed" => Self::Analyzed,
            "archived" => Self::Archived,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentObjective {
    Awareness,
    Education,
    Engagement,
    Traffic,
    LeadGeneration,
    Conversion,
    Retention,
    Community,
}

impl ContentObjective {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Awareness => "awareness",
            Self::Education => "education",
            Self::Engagement => "engagement",
            Self::Traffic => "traffic",
            Self::LeadGeneration => "lead_generation",
            Self::Conversion => "conversion",
            Self::Retention => "retention",
            Self::Community => "community",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "education" => Self::Education,
            "engagement" => Self::Engagement,
            "traffic" => Self::Traffic,
            "lead_generation" | "leads" => Self::LeadGeneration,
            "conversion" => Self::Conversion,
            "retention" => Self::Retention,
            "community" => Self::Community,
            _ => Self::Awareness,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CtaType {
    LearnMore,
    Visit,
    Reply,
    Subscribe,
    Signup,
    Buy,
    Download,
    Contact,
    Share,
}

impl CtaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LearnMore => "learn_more",
            Self::Visit => "visit",
            Self::Reply => "reply",
            Self::Subscribe => "subscribe",
            Self::Signup => "signup",
            Self::Buy => "buy",
            Self::Download => "download",
            Self::Contact => "contact",
            Self::Share => "share",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrandVoice {
    #[serde(default)]
    pub tone: String,
    #[serde(default)]
    pub formality: String,
    #[serde(default)]
    pub directness: String,
    #[serde(default)]
    pub humor: String,
    #[serde(default)]
    pub technicality: String,
    #[serde(default)]
    pub emotionality: String,
    #[serde(default)]
    pub sentence_length: String,
    #[serde(default)]
    pub vocabulary: Vec<String>,
    #[serde(default)]
    pub style_notes: String,
}

impl Default for BrandVoice {
    fn default() -> Self {
        Self {
            tone: "direct".into(),
            formality: "low".into(),
            directness: "high".into(),
            humor: "moderate".into(),
            technicality: "high".into(),
            emotionality: "moderate".into(),
            sentence_length: "short".into(),
            vocabulary: vec![],
            style_notes: "No corporate fluff; concrete examples.".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrandProfile {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub products: Vec<String>,
    #[serde(default)]
    pub value_proposition: String,
    #[serde(default)]
    pub positioning: String,
    #[serde(default)]
    pub tone_of_voice: BrandVoice,
    #[serde(default)]
    pub vocabulary: Vec<String>,
    #[serde(default)]
    pub forbidden_terms: Vec<String>,
    #[serde(default)]
    pub differentiators: Vec<String>,
    #[serde(default)]
    pub competitors: Vec<String>,
    #[serde(default)]
    pub brand_rules: Map<String, Value>,
    #[serde(default)]
    pub examples: Vec<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudienceProfile {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub problems: Vec<String>,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub motivations: Vec<String>,
    #[serde(default)]
    pub objections: Vec<String>,
    #[serde(default)]
    pub knowledge_level: String,
    #[serde(default)]
    pub preferred_channels: Vec<String>,
    #[serde(default)]
    pub content_preferences: Map<String, Value>,
    #[serde(default)]
    pub audience_language: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentProject {
    pub id: Uuid,
    pub goal_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub brand_id: Option<Uuid>,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentPillar {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentStrategy {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_id: Uuid,
    #[serde(default)]
    pub objectives: Vec<String>,
    #[serde(default)]
    pub pillars: Vec<ContentPillar>,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub frequency: String,
    #[serde(default)]
    pub cta_strategy: String,
    #[serde(default)]
    pub distribution: Map<String, Value>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentIdea {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_id: Uuid,
    pub run_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub hook: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_id: Option<Uuid>,
    #[serde(default)]
    pub pain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pillar_id: Option<Uuid>,
    pub format: ContentFormatType,
    pub channel: ContentChannelId,
    pub objective: ContentObjective,
    #[serde(default)]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentBrief {
    pub id: Uuid,
    pub idea_id: Option<Uuid>,
    pub content_id: Option<Uuid>,
    pub goal: String,
    pub audience: String,
    pub problem: String,
    pub core_message: String,
    pub angle: String,
    pub hook: String,
    #[serde(default)]
    pub structure: Vec<String>,
    pub cta: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub sources: Vec<ContentSourceRef>,
    pub tone: String,
    pub channel: String,
    pub format: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentSourceRef {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub relevance: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentExplainability {
    pub goal: String,
    pub audience: String,
    #[serde(default)]
    pub pain: String,
    #[serde(default)]
    pub research_refs: Vec<String>,
    #[serde(default)]
    pub strategy: String,
    #[serde(default)]
    pub hypothesis: String,
    pub expected_objective: String,
    #[serde(default)]
    pub generated_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentQualityGates {
    #[serde(default)]
    pub brand_check: bool,
    #[serde(default)]
    pub audience_check: bool,
    #[serde(default)]
    pub channel_check: bool,
    #[serde(default)]
    pub fact_check: bool,
    #[serde(default)]
    pub policy_check: bool,
    #[serde(default)]
    pub spam_check: bool,
    #[serde(default)]
    pub approval_check: bool,
}

impl ContentQualityGates {
    pub fn ready_to_publish(&self) -> bool {
        self.brand_check
            && self.audience_check
            && self.channel_check
            && self.policy_check
            && self.spam_check
            && self.approval_check
            && self.fact_check
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentItem {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_id: Uuid,
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_content_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idea_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pillar_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    pub title: String,
    pub body: String,
    pub format: ContentFormatType,
    pub channel: ContentChannelId,
    pub objective: ContentObjective,
    pub lifecycle: ContentLifecycle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cta: Option<CtaType>,
    #[serde(default)]
    pub explainability: ContentExplainability,
    #[serde(default)]
    pub quality_gates: ContentQualityGates,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub legacy_draft_id: Option<Uuid>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentVersion {
    pub id: Uuid,
    pub content_id: Uuid,
    pub version: u32,
    pub stage: String,
    pub body: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CritiqueIssue {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(default)]
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentCritique {
    pub content_id: Uuid,
    pub issues: Vec<CritiqueIssue>,
    pub ready_to_publish: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimVerification {
    Verified,
    Uncertain,
    Unsupported,
    Contradicted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactClaim {
    pub claim: String,
    pub status: ClaimVerification,
    #[serde(default)]
    pub sources: Vec<ContentSourceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentCalendarEntry {
    pub id: Uuid,
    pub project_id: Uuid,
    pub content_id: Uuid,
    pub channel: ContentChannelId,
    pub format: ContentFormatType,
    pub scheduled_at: DateTime<Utc>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentPublication {
    pub id: Uuid,
    pub content_id: Uuid,
    pub channel: ContentChannelId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub published_at: DateTime<Utc>,
    pub status: String,
    #[serde(default)]
    pub simulated: bool,
    #[serde(default)]
    pub metrics: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentInsight {
    pub id: Uuid,
    pub project_id: Uuid,
    pub run_id: Uuid,
    pub observation: String,
    pub evidence: String,
    pub suggestion: String,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub sample_size: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentEngineSnapshot {
    pub project: Option<ContentProject>,
    pub brand: Option<BrandProfile>,
    pub audiences: Vec<AudienceProfile>,
    pub strategy: Option<ContentStrategy>,
    pub ideas: Vec<ContentIdea>,
    pub items: Vec<ContentItem>,
    pub calendar: Vec<ContentCalendarEntry>,
    pub publications: Vec<ContentPublication>,
    pub insights: Vec<ContentInsight>,
    pub graph_edges: Vec<ContentGraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentGraphEdge {
    pub parent_id: Uuid,
    pub child_id: Uuid,
    pub relation: String,
}

/// Map legacy marketing draft into content item fields.
pub fn item_from_legacy_draft(
    draft: &super::ContentDraft,
    project_id: Uuid,
    title: &str,
) -> ContentItem {
    let now = Utc::now();
    ContentItem {
        id: draft.id,
        project_id,
        goal_id: draft.goal_id,
        run_id: draft.run_id,
        parent_content_id: None,
        idea_id: None,
        brief_id: None,
        audience_id: None,
        pillar_id: None,
        experiment_id: draft.experiment_id,
        title: title.into(),
        body: draft.body.clone(),
        format: ContentFormatType::TelegramPost,
        channel: ContentChannelId::parse(&draft.channel),
        objective: ContentObjective::LeadGeneration,
        lifecycle: match draft.status {
            super::ContentDraftStatus::Published => ContentLifecycle::Published,
            super::ContentDraftStatus::Approved => ContentLifecycle::Approved,
            super::ContentDraftStatus::PendingApproval => ContentLifecycle::Review,
            super::ContentDraftStatus::Rejected => ContentLifecycle::Draft,
            super::ContentDraftStatus::Draft => ContentLifecycle::Draft,
        },
        cta: Some(CtaType::Reply),
        explainability: ContentExplainability {
            goal: String::new(),
            audience: String::new(),
            pain: String::new(),
            research_refs: vec![],
            strategy: String::new(),
            hypothesis: String::new(),
            expected_objective: ContentObjective::LeadGeneration.as_str().into(),
            generated_by: "agent.marketing.legacy".into(),
        },
        quality_gates: ContentQualityGates::default(),
        scheduled_at: None,
        legacy_draft_id: Some(draft.id),
        metadata: draft.metadata.clone(),
        created_at: draft.created_at,
        updated_at: draft.updated_at,
    }
}

pub fn content_capability_for_tool(type_id: &str) -> Option<&'static str> {
    use super::capability::caps;
    match type_id {
        "idea.generate" | "idea.expand" | "idea.combine" | "idea.validate" | "idea.research"
        | "strategy.create" | "strategy.update" | "brief.create" | "content.create"
        | "content.rewrite" | "content.expand" | "content.shorten" | "content.adapt"
        | "content.translate" | "content.critique" | "content.fact_check"
        | "content.create_draft" | "content.variant" | "analytics.read" | "analytics.analyze"
        | "insight.create" | "experiment.create" | "experiment.evaluate" => Some(caps::ARTIFACT_WRITE),
        "publish.prepare" | "asset.generate" | "asset.attach" => Some(caps::ARTIFACT_WRITE),
        "publish.send" | "publish.schedule" | "publisher.telegram.send" => Some(caps::TELEGRAM_WRITE),
        "publisher.telegram.prepare" => Some(caps::ARTIFACT_WRITE),
        _ => None,
    }
}

pub fn is_content_outbound_tool(type_id: &str) -> bool {
    matches!(
        type_id,
        "publish.send" | "publish.schedule" | "publisher.telegram.send"
    )
}
