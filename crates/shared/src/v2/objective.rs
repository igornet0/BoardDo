//! Objectives — user goals broken into a tree of items that SmartDo executes
//! through generated (or user-picked) scenarios.
//!
//! Distinct from [`super::goal_runtime::GoalSpec`] (metric-driven marketing
//! agent loop): an objective is a checklist the AI can draft, decompose and
//! run item by item, each item backed by a real workflow on the canvas.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Max entries kept in [`Objective::log`].
pub const OBJECTIVE_LOG_LIMIT: usize = 200;
/// Max depth of the item tree (objective → item → sub-item → …).
pub const OBJECTIVE_MAX_DEPTH: usize = 3;
/// Upper bound for [`Objective::auto_refine`].
pub const OBJECTIVE_MAX_REFINE: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveStatus {
    #[default]
    Draft,
    Ready,
    Running,
    /// Stopped by the user or waiting on manual items.
    Paused,
    Done,
    Failed,
}

impl ObjectiveStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "ready" => Self::Ready,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveItemStatus {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

impl ObjectiveItemStatus {
    /// Counts as finished for dependency and progress purposes.
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Done | Self::Skipped)
    }
}

/// How an item is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveItemKind {
    /// Generated scenario: trigger → ai.chat.
    #[default]
    Ai,
    /// Generated scenario: trigger → web.search → ai.chat.
    Research,
    /// User-selected existing scenario (`workflow_id`).
    Workflow,
    /// Done by a human; the run waits for it.
    Manual,
    /// A ProjectHub task in the goal's linked project; the run tracks it
    /// until it is done there (by a developer, an agent or a closed issue).
    Task,
}

impl ObjectiveItemKind {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "research" | "web" | "search" => Self::Research,
            "workflow" | "scenario" => Self::Workflow,
            "manual" | "human" => Self::Manual,
            "task" | "projecthub" | "dev" | "code" => Self::Task,
            _ => Self::Ai,
        }
    }
}

/// Named property of an objective: budget, deadline, audience, format, …
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectiveCharacteristic {
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectiveItem {
    /// Stable short id inside the objective (`i1`, `i1.2`, …).
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: ObjectiveItemKind,
    #[serde(default)]
    pub status: ObjectiveItemStatus,
    /// Item ids that must be settled first (leaf or parent ids).
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Extra instruction for the AI step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Search query for `research` items (defaults to the title).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Scenario that executes this item (generated on first run or user-picked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<Uuid>,
    /// Fingerprint of the generated scenario; a mismatch means the user edited
    /// it on the canvas and BoardDo must not overwrite it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub attempts: u32,
    /// Task id in the external tracker (ProjectHub) for `task` items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_task_id: Option<String>,
    /// Last status seen there (`todo` | `in_progress` | `done` | `cancelled`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_status: Option<String>,
    /// GitHub issue linked to that task, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

impl ObjectiveItem {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            parent_id: None,
            title: title.into(),
            description: String::new(),
            kind: ObjectiveItemKind::Ai,
            status: ObjectiveItemStatus::Pending,
            depends_on: Vec::new(),
            prompt: None,
            query: None,
            workflow_id: None,
            scenario_hash: None,
            execution_id: None,
            result: None,
            error: None,
            attempts: 0,
            external_task_id: None,
            external_status: None,
            external_url: None,
            started_at: None,
            finished_at: None,
        }
    }

    /// Clear execution state so the item runs again (scenario link is kept).
    pub fn reset(&mut self) {
        self.status = ObjectiveItemStatus::Pending;
        self.execution_id = None;
        self.result = None;
        self.error = None;
        self.started_at = None;
        self.finished_at = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectiveLogEntry {
    pub at: DateTime<Utc>,
    /// `info` | `success` | `warning` | `error`.
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct ObjectiveProgress {
    /// Executable (leaf) items.
    pub total: u32,
    pub done: u32,
    pub failed: u32,
    pub running: u32,
    pub pending: u32,
    /// 0..100 over leaf items (skipped count as done).
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Objective {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: ObjectiveStatus,
    #[serde(default)]
    pub characteristics: Vec<ObjectiveCharacteristic>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub items: Vec<ObjectiveItem>,
    /// AI connection (OpenAI-compatible or Claude) for drafting and execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    /// ProjectHub project the goal works on (code context, `task` items).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_hub_project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_hub_project_name: Option<String>,
    /// Final evaluation written after the last item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub achieved: Option<bool>,
    /// Rounds the run may add gap-closing items on its own after a "not achieved" evaluation.
    #[serde(default)]
    pub auto_refine: u32,
    /// Refinement rounds already spent in the current run.
    #[serde(default)]
    pub refine_rounds: u32,
    #[serde(default)]
    pub log: Vec<ObjectiveLogEntry>,
    #[serde(default)]
    pub progress: ObjectiveProgress,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

impl Objective {
    pub fn new(title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            title: title.into(),
            description: String::new(),
            status: ObjectiveStatus::Draft,
            characteristics: Vec::new(),
            success_criteria: Vec::new(),
            items: Vec::new(),
            connection_id: None,
            model: None,
            deadline: None,
            project_hub_project_id: None,
            project_hub_project_name: None,
            summary: None,
            achieved: None,
            auto_refine: 0,
            refine_rounds: 0,
            log: Vec::new(),
            progress: ObjectiveProgress::default(),
            created_at: now,
            updated_at: now,
            started_at: None,
            finished_at: None,
        }
    }

    pub fn item(&self, id: &str) -> Option<&ObjectiveItem> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_mut(&mut self, id: &str) -> Option<&mut ObjectiveItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    pub fn children(&self, id: &str) -> impl Iterator<Item = &ObjectiveItem> {
        self.items
            .iter()
            .filter(move |i| i.parent_id.as_deref() == Some(id))
    }

    pub fn is_leaf(&self, id: &str) -> bool {
        self.children(id).next().is_none()
    }

    pub fn depth(&self, id: &str) -> usize {
        let mut depth = 0;
        let mut cur = self.item(id).and_then(|i| i.parent_id.clone());
        while let Some(pid) = cur {
            depth += 1;
            if depth > self.items.len() {
                break;
            }
            cur = self.item(&pid).and_then(|i| i.parent_id.clone());
        }
        depth
    }

    /// Leaf: own status. Parent: every descendant leaf settled.
    pub fn is_settled(&self, id: &str) -> bool {
        let Some(item) = self.item(id) else {
            // Unknown dependency ids never block a run.
            return true;
        };
        if self.is_leaf(id) {
            return item.status.is_settled();
        }
        self.children(id).all(|c| self.is_settled(&c.id))
    }

    /// Dependencies of an item including those inherited from its ancestors.
    fn effective_deps(&self, id: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let mut cur = self.item(id);
        let mut guard = 0;
        while let Some(item) = cur {
            deps.extend(item.depends_on.iter().cloned());
            guard += 1;
            if guard > self.items.len() {
                break;
            }
            cur = item.parent_id.as_deref().and_then(|p| self.item(p));
        }
        deps
    }

    /// Leaf items in tree order whose dependencies are settled and that still need a run.
    pub fn ready_items(&self) -> Vec<String> {
        self.ordered_ids()
            .into_iter()
            .filter(|id| self.is_leaf(id))
            .filter(|id| {
                self.item(id)
                    .is_some_and(|i| i.status == ObjectiveItemStatus::Pending)
            })
            .filter(|id| {
                let ancestors = self.ancestors(id);
                self.effective_deps(id)
                    .iter()
                    .filter(|d| *d != id && !ancestors.contains(d))
                    .all(|d| self.is_settled(d))
            })
            .collect()
    }

    fn ancestors(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = self.item(id).and_then(|i| i.parent_id.clone());
        while let Some(pid) = cur {
            if out.contains(&pid) || out.len() > self.items.len() {
                break;
            }
            cur = self.item(&pid).and_then(|i| i.parent_id.clone());
            out.push(pid);
        }
        out
    }

    /// Depth-first order following `items` order among siblings.
    pub fn ordered_ids(&self) -> Vec<String> {
        fn walk(o: &Objective, parent: Option<&str>, out: &mut Vec<String>, depth: usize) {
            if depth > OBJECTIVE_MAX_DEPTH + 2 {
                return;
            }
            for item in o.items.iter().filter(|i| i.parent_id.as_deref() == parent) {
                if out.contains(&item.id) {
                    continue;
                }
                out.push(item.id.clone());
                walk(o, Some(&item.id), out, depth + 1);
            }
        }
        let mut out = Vec::new();
        walk(self, None, &mut out, 0);
        // Orphans (parent id missing) still show up at the end.
        for item in &self.items {
            if !out.contains(&item.id) {
                out.push(item.id.clone());
            }
        }
        out
    }

    /// Parents mirror their children; progress counts leaves.
    pub fn recompute(&mut self) {
        let ids = self.ordered_ids();
        // Bottom-up so nested parents see updated children.
        for id in ids.iter().rev() {
            if self.is_leaf(id) {
                continue;
            }
            let statuses: Vec<ObjectiveItemStatus> = self.children(id).map(|c| c.status).collect();
            let next = if statuses.iter().all(|s| s.is_settled()) {
                ObjectiveItemStatus::Done
            } else if statuses.contains(&ObjectiveItemStatus::Running) {
                ObjectiveItemStatus::Running
            } else if statuses.contains(&ObjectiveItemStatus::Failed) {
                ObjectiveItemStatus::Failed
            } else if statuses.iter().any(|s| s.is_settled()) {
                ObjectiveItemStatus::Running
            } else {
                ObjectiveItemStatus::Pending
            };
            if let Some(item) = self.item_mut(id) {
                item.status = next;
            }
        }

        let mut p = ObjectiveProgress::default();
        for id in &ids {
            if !self.is_leaf(id) {
                continue;
            }
            let Some(item) = self.item(id) else { continue };
            p.total += 1;
            match item.status {
                ObjectiveItemStatus::Done | ObjectiveItemStatus::Skipped => p.done += 1,
                ObjectiveItemStatus::Failed => p.failed += 1,
                ObjectiveItemStatus::Running => p.running += 1,
                ObjectiveItemStatus::Pending => p.pending += 1,
            }
        }
        p.percent = if p.total == 0 {
            0.0
        } else {
            ((p.done as f64 / p.total as f64) * 1000.0).round() / 10.0
        };
        self.progress = p;
    }

    pub fn push_log(&mut self, level: &str, item_id: Option<&str>, message: impl Into<String>) {
        self.log.push(ObjectiveLogEntry {
            at: Utc::now(),
            level: level.into(),
            item_id: item_id.map(str::to_string),
            message: message.into(),
        });
        if self.log.len() > OBJECTIVE_LOG_LIMIT {
            let drop = self.log.len() - OBJECTIVE_LOG_LIMIT;
            self.log.drain(..drop);
        }
    }

    /// Next free top-level (`i7`) or child (`i3.4`) id.
    pub fn next_item_id(&self, parent: Option<&str>) -> String {
        let prefix = match parent {
            Some(p) => format!("{p}."),
            None => "i".to_string(),
        };
        let mut n = 1;
        loop {
            let candidate = format!("{prefix}{n}");
            if self.item(&candidate).is_none() {
                return candidate;
            }
            n += 1;
        }
    }

    /// Drop dangling references and duplicate ids after edits / AI drafts.
    pub fn normalize(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.items.retain(|i| !i.id.trim().is_empty() && seen.insert(i.id.clone()));
        let ids: std::collections::HashSet<String> = self.items.iter().map(|i| i.id.clone()).collect();
        for item in &mut self.items {
            if item.parent_id.as_ref().is_some_and(|p| !ids.contains(p) || p == &item.id) {
                item.parent_id = None;
            }
            let own = item.id.clone();
            item.depends_on.retain(|d| ids.contains(d) && d != &own);
            item.title = item.title.trim().to_string();
        }
        self.break_dependency_cycles();
        self.characteristics.retain(|c| !c.key.trim().is_empty());
        self.success_criteria.retain(|c| !c.trim().is_empty());
        self.recompute();
    }

    /// The item and everything nested inside it.
    fn subtree(&self, id: &str) -> Vec<String> {
        let mut out = vec![id.to_string()];
        let mut i = 0;
        while i < out.len() && out.len() <= self.items.len() + 1 {
            let kids: Vec<String> = self.children(&out[i]).map(|c| c.id.clone()).collect();
            out.extend(kids);
            i += 1;
        }
        out
    }

    /// Does `a` (transitively) wait for `b`? Waiting on a parent means waiting
    /// on everything inside it; items inherit their ancestors' dependencies.
    pub fn waits_on(&self, a: &str, b: &str) -> bool {
        let mut stack = vec![a.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            for dep in self.effective_deps(&cur) {
                let inside = self.subtree(&dep);
                if inside.iter().any(|x| x == b) {
                    return true;
                }
                stack.extend(inside);
            }
        }
        false
    }

    /// Drop the dependency edges that close a cycle (they would block forever).
    fn break_dependency_cycles(&mut self) {
        for id in self.ordered_ids() {
            let deps = self.item(&id).map(|i| i.depends_on.clone()).unwrap_or_default();
            for dep in deps {
                let cyclic = self.ancestors(&id).contains(&dep)
                    || self.subtree(&dep).contains(&id)
                    || self.waits_on(&dep, &id);
                if cyclic && let Some(item) = self.item_mut(&id) {
                    item.depends_on.retain(|d| d != &dep);
                }
            }
        }
    }
}

// ─── API payloads ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CreateObjectiveRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub characteristics: Vec<ObjectiveCharacteristic>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub items: Vec<ObjectiveItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub auto_refine: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_hub_project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_hub_project_name: Option<String>,
}

/// Full replacement of the editable fields (execution state of items is kept
/// when an item id survives and its definition did not change).
pub type UpdateObjectiveRequest = CreateObjectiveRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveAssistMode {
    /// Draft description, characteristics, criteria and top-level items.
    #[default]
    Fill,
    /// Split the objective (or `item_id`) into sub-items.
    Decompose,
    /// After a "not achieved" evaluation: append items that close the gaps.
    Refine,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ObjectiveAssistRequest {
    #[serde(default)]
    pub mode: ObjectiveAssistMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    /// Free-form wish from the user ("make it cheaper", "3 steps max").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    /// Save the draft straight away (otherwise the UI previews it).
    #[serde(default)]
    pub apply: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ObjectiveItemDraft {
    #[serde(default)]
    pub key: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    /// Keys (or titles) of sibling drafts this one waits for.
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub children: Vec<ObjectiveItemDraft>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ObjectiveDraft {
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub characteristics: Vec<ObjectiveCharacteristic>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub items: Vec<ObjectiveItemDraft>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectiveAssistResponse {
    pub draft: ObjectiveDraft,
    /// Objective with the draft merged in (saved only when `apply`).
    pub objective: Objective,
    pub applied: bool,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RunObjectiveRequest {
    /// Restart failed items (and everything when `reset`).
    #[serde(default)]
    pub retry_failed: bool,
    #[serde(default)]
    pub reset: bool,
    /// Run only this item (and nothing else).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UpdateObjectiveItemRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ObjectiveItemStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<Uuid>,
    /// Remove the scenario link (a new one is generated on next run).
    #[serde(default)]
    pub unlink_workflow: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Objective {
        let mut o = Objective::new("Launch landing page");
        let mut a = ObjectiveItem::new("i1", "Research competitors");
        a.kind = ObjectiveItemKind::Research;
        let b = ObjectiveItem::new("i2", "Write copy");
        let mut b1 = ObjectiveItem::new("i2.1", "Headline");
        b1.parent_id = Some("i2".into());
        let mut b2 = ObjectiveItem::new("i2.2", "Body");
        b2.parent_id = Some("i2".into());
        b2.depends_on = vec!["i2.1".into()];
        let mut c = ObjectiveItem::new("i3", "Review");
        c.depends_on = vec!["i2".into()];
        o.items = vec![a, b, b1, b2, c];
        o.normalize();
        o
    }

    #[test]
    fn ready_items_respect_tree_and_deps() {
        let mut o = tree();
        assert_eq!(o.ready_items(), vec!["i1", "i2.1"]);
        o.item_mut("i2.1").unwrap().status = ObjectiveItemStatus::Done;
        o.recompute();
        assert_eq!(o.ready_items(), vec!["i1", "i2.2"]);
        assert_eq!(o.item("i2").unwrap().status, ObjectiveItemStatus::Running);
        o.item_mut("i2.2").unwrap().status = ObjectiveItemStatus::Done;
        o.recompute();
        assert_eq!(o.item("i2").unwrap().status, ObjectiveItemStatus::Done);
        assert_eq!(o.ready_items(), vec!["i1", "i3"]);
    }

    #[test]
    fn progress_counts_leaves() {
        let mut o = tree();
        o.item_mut("i1").unwrap().status = ObjectiveItemStatus::Done;
        o.item_mut("i2.1").unwrap().status = ObjectiveItemStatus::Failed;
        o.recompute();
        assert_eq!(o.progress.total, 4);
        assert_eq!(o.progress.done, 1);
        assert_eq!(o.progress.failed, 1);
        assert_eq!(o.progress.percent, 25.0);
    }

    #[test]
    fn normalize_drops_dangling_refs() {
        let mut o = Objective::new("x");
        let mut a = ObjectiveItem::new("i1", " A ");
        a.parent_id = Some("nope".into());
        a.depends_on = vec!["i1".into(), "ghost".into()];
        o.items = vec![a.clone(), a];
        o.normalize();
        assert_eq!(o.items.len(), 1);
        assert_eq!(o.items[0].title, "A");
        assert!(o.items[0].parent_id.is_none());
        assert!(o.items[0].depends_on.is_empty());
    }

    #[test]
    fn normalize_breaks_dependency_cycles() {
        let mut o = Objective::new("x");
        let mut a = ObjectiveItem::new("i1", "A");
        let mut b = ObjectiveItem::new("i2", "B");
        a.depends_on = vec!["i2".into()];
        b.depends_on = vec!["i1".into()];
        let mut c = ObjectiveItem::new("i3", "C");
        let mut c1 = ObjectiveItem::new("i3.1", "C1");
        c1.parent_id = Some("i3".into());
        c1.depends_on = vec!["i3".into()]; // waits on own parent
        c.depends_on = vec![];
        o.items = vec![a, b, c, c1];
        o.normalize();
        let deps: usize = o.items.iter().map(|i| i.depends_on.len()).sum();
        assert_eq!(deps, 1, "{:?}", o.items.iter().map(|i| (&i.id, &i.depends_on)).collect::<Vec<_>>());
        assert!(!o.ready_items().is_empty());
        assert!(o.ready_items().contains(&"i3.1".to_string()));
    }

    #[test]
    fn next_ids() {
        let o = tree();
        assert_eq!(o.next_item_id(None), "i4");
        assert_eq!(o.next_item_id(Some("i2")), "i2.3");
        assert_eq!(o.depth("i2.2"), 1);
    }
}
