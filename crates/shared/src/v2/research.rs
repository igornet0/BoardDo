//! Research context — documents, facts, claims, and evidence for internet agents.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::web::parse_http_url;

/// A retrieved page (or API payload) stored as research memory, not raw HTML for the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchDocument {
    pub id: Uuid,
    pub url: String,
    pub title: String,
    pub source: String,
    pub retrieved_at: DateTime<Utc>,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl ResearchDocument {
    pub fn new(
        url: impl Into<String>,
        title: impl Into<String>,
        source: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            url: url.into(),
            title: title.into(),
            source: source.into(),
            retrieved_at: Utc::now(),
            content: content.into(),
            metadata: None,
        }
    }
}

/// Atomic extracted statement tied to a document / URL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: Uuid,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default)]
    pub confidence: f32,
}

impl Fact {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            text: text.into(),
            document_id: None,
            source_url: None,
            confidence: 0.0,
        }
    }
}

/// Quoted support for a claim, always pointing at a retrieved source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: Uuid,
    pub url: String,
    pub quote: String,
    pub retrieved_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_id: Option<Uuid>,
}

impl Evidence {
    pub fn new(url: impl Into<String>, quote: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            url: url.into(),
            quote: quote.into(),
            retrieved_at: Utc::now(),
            document_id: None,
        }
    }
}

/// A conclusion the agent wants to stand behind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: Uuid,
    pub text: String,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub confidence: f32,
}

/// Source quality dimensions. [`Self::weight`] is the product of all four.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SourceScore {
    pub authority: f32,
    pub freshness: f32,
    pub relevance: f32,
    pub reliability: f32,
}

impl SourceScore {
    pub fn weight(self) -> f32 {
        self.authority * self.freshness * self.relevance * self.reliability
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: Uuid,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub score: SourceScore,
}

impl Source {
    pub fn new(url: impl Into<String>, score: SourceScore) -> Self {
        Self {
            id: Uuid::now_v7(),
            url: url.into(),
            title: None,
            score,
        }
    }
}

/// Accumulated state of one research investigation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchContext {
    pub query: String,
    #[serde(default)]
    pub documents: Vec<ResearchDocument>,
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

impl ResearchContext {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            documents: Vec::new(),
            facts: Vec::new(),
            claims: Vec::new(),
            sources: Vec::new(),
            confidence: None,
        }
    }

    pub fn push_document(&mut self, document: ResearchDocument) {
        let url = document.url.clone();
        let title = document.title.clone();
        if !self.sources.iter().any(|s| s.url == url) {
            let mut source = Source::new(&url, Self::score_source(&url, 1.0, 1.0));
            if !title.is_empty() {
                source.title = Some(title);
            }
            self.sources.push(source);
        }
        self.documents.push(document);
    }

    /// Claims must carry at least one evidence item (quote + url).
    pub fn add_claim_with_evidence(
        &mut self,
        text: impl Into<String>,
        evidence: Vec<Evidence>,
        confidence: f32,
    ) -> Result<Uuid, ResearchError> {
        if evidence.is_empty() {
            return Err(ResearchError::ClaimNeedsEvidence);
        }
        if evidence
            .iter()
            .any(|e| e.url.is_empty() || e.quote.is_empty())
        {
            return Err(ResearchError::IncompleteEvidence);
        }
        let id = Uuid::now_v7();
        self.claims.push(Claim {
            id,
            text: text.into(),
            evidence,
            confidence,
        });
        Ok(id)
    }

    /// Heuristic source scoring from URL host. Freshness / relevance are caller-supplied.
    pub fn score_source(url: &str, freshness: f32, relevance: f32) -> SourceScore {
        let host = parse_http_url(url)
            .map(|p| p.host)
            .unwrap_or_else(|_| url.to_ascii_lowercase());
        let (authority, reliability) = authority_for_host(&host);
        SourceScore {
            authority,
            freshness: freshness.clamp(0.0, 1.0),
            relevance: relevance.clamp(0.0, 1.0),
            reliability,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearchError {
    ClaimNeedsEvidence,
    IncompleteEvidence,
}

impl std::fmt::Display for ResearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClaimNeedsEvidence => {
                write!(f, "claim requires at least one evidence item")
            }
            Self::IncompleteEvidence => {
                write!(f, "evidence requires a url and a quote")
            }
        }
    }
}

impl std::error::Error for ResearchError {}

fn authority_for_host(host: &str) -> (f32, f32) {
    if is_official_docs_host(host) {
        return (1.0, 1.0);
    }
    if host == "github.com"
        || host.ends_with(".github.com")
        || host == "gitlab.com"
        || host.ends_with(".gitlab.com")
    {
        return (0.9, 0.9);
    }
    if host.contains("blog")
        || host == "medium.com"
        || host.ends_with(".medium.com")
        || host.ends_with(".substack.com")
    {
        return (0.6, 0.7);
    }
    if host == "stackoverflow.com"
        || host.ends_with(".stackexchange.com")
        || host == "reddit.com"
        || host.ends_with(".reddit.com")
        || host == "news.ycombinator.com"
        || host.contains("forum")
    {
        return (0.3, 0.5);
    }
    (0.5, 0.5)
}

fn is_official_docs_host(host: &str) -> bool {
    host == "docs.rs"
        || host.ends_with(".docs.rs")
        || host == "rust-lang.org"
        || host.ends_with(".rust-lang.org")
        || host == "doc.rust-lang.org"
        || host.ends_with(".readthedocs.io")
        || host == "developer.mozilla.org"
        || host.starts_with("docs.")
        || host.contains(".docs.")
}
