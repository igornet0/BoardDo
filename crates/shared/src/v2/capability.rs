//! Capability / permission model — what nodes and agents may do.

use serde::{Deserialize, Serialize};

/// A concrete permission string, e.g. `telegram.write`, `ads.launch`.
///
/// Keep the vocabulary stable; agents and composite nodes declare budgets
/// against this namespace. SmartDo enforces at execution time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capability(pub String);

impl Capability {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Capability {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for Capability {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Well-known capability identifiers (extend over time; do not rename).
pub mod caps {
    pub const TELEGRAM_READ: &str = "telegram.read";
    pub const TELEGRAM_WRITE: &str = "telegram.write";
    pub const HTTP_REQUEST: &str = "http.request";
    pub const WEB_SEARCH: &str = "web.search";
    pub const WEB_OPEN: &str = "web.open";
    pub const WEB_FETCH: &str = "web.fetch";
    pub const WEB_EXTRACT: &str = "web.extract";
    pub const WEB_SCREENSHOT: &str = "web.screenshot";
    pub const RESEARCH_WRITE: &str = "research.write";
    pub const WEBSITE_CREATE: &str = "website.create";
    pub const WEBSITE_DEPLOY: &str = "website.deploy";
    pub const GITHUB_READ: &str = "github.read";
    pub const GITHUB_WRITE: &str = "github.write";
    pub const ADS_READ: &str = "ads.read";
    pub const ADS_CREATE: &str = "ads.create";
    pub const ADS_LAUNCH: &str = "ads.launch";
    pub const BILLING_WRITE: &str = "billing.write";
    pub const DATABASE_READ: &str = "database.read";
    pub const DATABASE_WRITE: &str = "database.write";
    pub const DATABASE_DELETE: &str = "database.delete";
    pub const AI_GENERATE: &str = "ai.generate";
    pub const AI_ANALYZE: &str = "ai.analyze";
    pub const ARTIFACT_WRITE: &str = "artifact.write";
    pub const WORKFLOW_EDIT: &str = "workflow.edit";
    pub const WORKFLOW_INVOKE: &str = "workflow.invoke";
    pub const TOOL_BUILD: &str = "tool.build";
    pub const TOOL_EXECUTE: &str = "tool.execute";
}

/// Allow / deny set attached to a node, agent, or run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityBudget {
    /// Explicitly granted capabilities.
    #[serde(default)]
    pub allow: Vec<Capability>,
    /// Explicitly denied (wins over allow and inheritance).
    #[serde(default)]
    pub deny: Vec<Capability>,
}

impl CapabilityBudget {
    pub fn allows(&self, cap: &str) -> bool {
        if self.deny.iter().any(|c| c.as_str() == cap) {
            return false;
        }
        self.allow.iter().any(|c| c.as_str() == cap)
    }

    pub fn grant(mut self, cap: impl Into<Capability>) -> Self {
        self.allow.push(cap.into());
        self
    }

    pub fn forbid(mut self, cap: impl Into<Capability>) -> Self {
        self.deny.push(cap.into());
        self
    }

    /// Child budget cannot exceed parent: intersect allows, union denies.
    pub fn narrow(&self, child: &CapabilityBudget) -> CapabilityBudget {
        let allow = child
            .allow
            .iter()
            .filter(|c| self.allows(c.as_str()))
            .cloned()
            .collect();
        let mut deny = self.deny.clone();
        for d in &child.deny {
            if !deny.iter().any(|x| x == d) {
                deny.push(d.clone());
            }
        }
        CapabilityBudget { allow, deny }
    }
}
