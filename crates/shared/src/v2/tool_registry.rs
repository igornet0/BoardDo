//! Tool registry — the single description of every tool an agent or a
//! workflow node can invoke: inputs and outputs, required capability, cost
//! class, side effects, idempotency, result verification and failure modes.
//!
//! [`capability_for_tool`](super::capability_for_tool) and
//! [`tool_counts_as_action`](super::tool_counts_as_action) read from here, the
//! policy denies tools missing from the catalog (or without an executor), and
//! trigger ingress refuses graphs that reference them.

use serde::Serialize;
use serde_json::Value;

use super::capability::caps;
use super::node_contract::NodeKind;

/// What a successful call changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    /// Reads only.
    None,
    /// Writes BoardDo's own state (memory, workflows, leads, drafts).
    Internal,
    /// Acts on the outside world (sends messages, calls third-party APIs).
    External,
}

/// What a call costs beyond CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    Free,
    /// Paid model tokens (recorded in the execution ledger).
    Model,
    /// Third-party metered API.
    External,
}

/// Where the executor lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    /// Workflow node handler in the engine registry.
    Node,
    /// Agent-runtime gateway function (not usable as a graph node).
    Gateway,
}

/// How to check that a result is usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    None,
    /// Result must not be `null`.
    NonNull,
    /// Result must be an object containing every listed key.
    Keys(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureMode {
    Timeout,
    RateLimited,
    Auth,
    InvalidInput,
    NotFound,
    Upstream,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ToolDescriptor {
    pub type_id: &'static str,
    pub aliases: &'static [&'static str],
    pub capability: &'static str,
    pub kind: NodeKind,
    pub source: ToolSource,
    pub side_effect: SideEffect,
    /// Cannot be undone (deleting, running shell commands). Needs approval.
    pub irreversible: bool,
    /// Repeating the call with the same input is harmless.
    pub idempotent: bool,
    pub cost: CostClass,
    /// Consumes the daily action budget.
    pub counts_as_action: bool,
    /// Config keys that must be present; `"a|b"` means either one.
    pub required_inputs: &'static [&'static str],
    pub optional_inputs: &'static [&'static str],
    pub outputs: &'static [&'static str],
    pub verification: Verification,
    pub failure_modes: &'static [FailureMode],
    /// An executor exists; `false` means the id is reserved but not runnable.
    pub available: bool,
}

const fn tool(
    type_id: &'static str,
    capability: &'static str,
    kind: NodeKind,
    source: ToolSource,
) -> ToolDescriptor {
    ToolDescriptor {
        type_id,
        aliases: &[],
        capability,
        kind,
        source,
        side_effect: SideEffect::Internal,
        irreversible: false,
        idempotent: true,
        cost: CostClass::Free,
        counts_as_action: true,
        required_inputs: &[],
        optional_inputs: &[],
        outputs: &[],
        verification: Verification::NonNull,
        failure_modes: &[],
        available: true,
    }
}

const fn node(type_id: &'static str, capability: &'static str, kind: NodeKind) -> ToolDescriptor {
    tool(type_id, capability, kind, ToolSource::Node)
}

const fn gw(type_id: &'static str, capability: &'static str) -> ToolDescriptor {
    tool(type_id, capability, NodeKind::Action, ToolSource::Gateway)
}

impl ToolDescriptor {
    const fn aliases(mut self, a: &'static [&'static str]) -> Self {
        self.aliases = a;
        self
    }
    /// External side effect; repeating it repeats the effect.
    const fn external(mut self) -> Self {
        self.side_effect = SideEffect::External;
        self
    }
    /// No side effects, does not count as an action.
    const fn read_only(mut self) -> Self {
        self.side_effect = SideEffect::None;
        self.counts_as_action = false;
        self
    }
    /// Inspects state without consuming the action budget.
    const fn no_action(mut self) -> Self {
        self.counts_as_action = false;
        self
    }
    const fn irreversible(mut self) -> Self {
        self.irreversible = true;
        self.idempotent = false;
        self
    }
    const fn not_idempotent(mut self) -> Self {
        self.idempotent = false;
        self
    }
    const fn cost(mut self, c: CostClass) -> Self {
        self.cost = c;
        self
    }
    const fn model(self) -> Self {
        self.cost(CostClass::Model)
    }
    const fn req(mut self, r: &'static [&'static str]) -> Self {
        self.required_inputs = r;
        self
    }
    const fn opt(mut self, o: &'static [&'static str]) -> Self {
        self.optional_inputs = o;
        self
    }
    const fn outputs(mut self, o: &'static [&'static str]) -> Self {
        self.outputs = o;
        self
    }
    const fn verify(mut self, v: Verification) -> Self {
        self.verification = v;
        self
    }
    const fn fails(mut self, f: &'static [FailureMode]) -> Self {
        self.failure_modes = f;
        self
    }
    const fn unavailable(mut self) -> Self {
        self.available = false;
        self
    }
}

use FailureMode::*;
use NodeKind as K;

const NET: &[FailureMode] = &[Timeout, RateLimited, Upstream];
const NET_AUTH: &[FailureMode] = &[Timeout, RateLimited, Auth, Upstream, InvalidInput];
const MODEL: &[FailureMode] = &[Timeout, RateLimited, Auth, Upstream];
const LOOKUP: &[FailureMode] = &[NotFound, InvalidInput];
const INPUT: &[FailureMode] = &[InvalidInput];

static CATALOG: &[ToolDescriptor] = &[
    // ── triggers / data / control (engine nodes) ─────────────────────────
    node("trigger.manual", caps::HTTP_REQUEST, K::Trigger).read_only().verify(Verification::None),
    node("trigger.webhook", caps::HTTP_REQUEST, K::Trigger).read_only().verify(Verification::None),
    node("trigger.schedule", caps::HTTP_REQUEST, K::Trigger).read_only().verify(Verification::None),
    node("trigger.telegram.user.message_received", caps::HTTP_REQUEST, K::Trigger)
        .read_only()
        .verify(Verification::None),
    node("data.set", caps::HTTP_REQUEST, K::Data).read_only().verify(Verification::None),
    node("data.transform", caps::HTTP_REQUEST, K::Data).read_only().verify(Verification::None).fails(INPUT),
    node("logic.condition", caps::HTTP_REQUEST, K::Decision)
        .read_only()
        .req(&["expression"])
        .fails(INPUT),
    node("logic.delay", caps::HTTP_REQUEST, K::Control).read_only().verify(Verification::None),
    node("debug.log", caps::ARTIFACT_WRITE, K::Debug).verify(Verification::None),
    // ── http / web ───────────────────────────────────────────────────────
    node("http.request", caps::HTTP_REQUEST, K::Action)
        .external()
        .not_idempotent()
        .req(&["url|connection_id"])
        .opt(&["method", "headers", "body", "path"])
        .outputs(&["status", "body"])
        .verify(Verification::Keys(&["status"]))
        .fails(NET_AUTH),
    node("web.search", caps::WEB_SEARCH, K::Action)
        .cost(CostClass::External)
        .req(&["query"])
        .outputs(&["results"])
        .fails(NET),
    node("web.fetch", caps::WEB_FETCH, K::Action).req(&["url"]).outputs(&["content"]).fails(NET),
    node("web.open", caps::WEB_FETCH, K::Action).req(&["url"]).fails(NET),
    node("web.extract", caps::WEB_FETCH, K::Action)
        .model()
        .req(&["instruction", "url"])
        .fails(MODEL),
    // ── ai ───────────────────────────────────────────────────────────────
    node("ai.chat", caps::AI_GENERATE, K::Ai).model().not_idempotent().fails(MODEL),
    node("ai.classify", caps::AI_ANALYZE, K::Ai).model().fails(MODEL),
    node("ai.analyze", caps::AI_ANALYZE, K::Ai).model().fails(MODEL),
    node("ai.image", caps::AI_GENERATE, K::Ai).model().not_idempotent().fails(MODEL),
    node("ai.audio", caps::AI_GENERATE, K::Ai).model().not_idempotent().fails(MODEL),
    node("ai.video", caps::AI_GENERATE, K::Ai).model().not_idempotent().fails(MODEL),
    // Reserved in NodeContracts / planner vocabulary; no executor yet.
    node("ai.generate", caps::AI_GENERATE, K::Ai).model().unavailable(),
    node("ai.extract", caps::AI_ANALYZE, K::Ai).model().unavailable(),
    node("ai.decide", caps::AI_ANALYZE, K::Ai).model().unavailable(),
    node("ai.plan", caps::AI_ANALYZE, K::Ai).model().unavailable(),
    node("ai.code", caps::AI_GENERATE, K::Ai).model().unavailable(),
    node("human.approval", caps::HTTP_REQUEST, K::Approval).unavailable(),
    node("workflow.composite", caps::WORKFLOW_INVOKE, K::Composite).unavailable(),
    node("agent.run", caps::WORKFLOW_INVOKE, K::Agent).unavailable(),
    // ── github / telegram bot ────────────────────────────────────────────
    node("github.get_latest_release", caps::GITHUB_READ, K::Action)
        .read_only()
        .req(&["repo|owner"])
        .fails(NET_AUTH),
    node("telegram.send_message", caps::TELEGRAM_WRITE, K::Action)
        .external()
        .not_idempotent()
        .fails(NET_AUTH),
    node("telegram.send_photo", caps::TELEGRAM_WRITE, K::Action)
        .external()
        .not_idempotent()
        .fails(NET_AUTH),
    node("telegram.send_document", caps::TELEGRAM_WRITE, K::Action)
        .external()
        .not_idempotent()
        .fails(NET_AUTH),
    // ── telegram user account ────────────────────────────────────────────
    node("telegram.user.send_message", caps::TELEGRAM_WRITE, K::Action)
        .external()
        .not_idempotent()
        .fails(NET_AUTH),
    node("telegram.user.forward_message", caps::TELEGRAM_WRITE, K::Action)
        .external()
        .not_idempotent()
        .fails(NET_AUTH),
    node("telegram.user.edit_message", caps::TELEGRAM_WRITE, K::Action)
        .external()
        .fails(NET_AUTH),
    node("telegram.user.get_chat_members", caps::TELEGRAM_READ, K::Action)
        .read_only()
        .fails(NET_AUTH),
    node("telegram.user.delete_messages", caps::TELEGRAM_WRITE, K::Action)
        .external()
        .irreversible()
        .fails(NET_AUTH),
    // ── projecthub extension ─────────────────────────────────────────────
    node("projecthub.projects", caps::GITHUB_READ, K::Action).read_only().fails(NET),
    node("projecthub.project", caps::GITHUB_READ, K::Action).read_only().fails(NET),
    node("projecthub.brdd", caps::GITHUB_READ, K::Action).read_only().fails(NET),
    node("projecthub.code", caps::GITHUB_READ, K::Action).read_only().fails(NET),
    node("projecthub.git", caps::GITHUB_READ, K::Action).read_only().fails(NET),
    node("projecthub.tasks", caps::GITHUB_READ, K::Action).read_only().fails(NET),
    node("projecthub.task.create", caps::GITHUB_WRITE, K::Action)
        .external()
        .not_idempotent()
        .fails(NET),
    node("projecthub.task.update", caps::GITHUB_WRITE, K::Action).external().fails(NET),
    node("projecthub.github", caps::GITHUB_WRITE, K::Action)
        .external()
        .not_idempotent()
        .fails(NET),
    node("projecthub.command", caps::GITHUB_WRITE, K::Action)
        .external()
        .irreversible()
        .req(&["command"])
        .fails(NET),
    node("alertwim.alert", caps::HTTP_REQUEST, K::Action)
        .external()
        .not_idempotent()
        .req(&["message"])
        .fails(NET),
    // ── memory / analytics (gateway) ─────────────────────────────────────
    gw("memory.write", caps::ARTIFACT_WRITE).aliases(&["memory.save"]),
    gw("memory.read", caps::ARTIFACT_WRITE).read_only().verify(Verification::None),
    gw("analytics.record_lead", caps::RESEARCH_WRITE).not_idempotent(),
    gw("analytics.record_progress", caps::RESEARCH_WRITE).not_idempotent(),
    // ── workflows / plans ────────────────────────────────────────────────
    gw("workflow.list", caps::WORKFLOW_EDIT).read_only(),
    gw("workflow.get", caps::WORKFLOW_EDIT).read_only().fails(LOOKUP),
    gw("workflow.create", caps::WORKFLOW_EDIT).not_idempotent().fails(INPUT),
    gw("workflow.update", caps::WORKFLOW_EDIT).fails(LOOKUP),
    gw("workflow.validate", caps::WORKFLOW_EDIT).read_only().fails(LOOKUP),
    gw("workflow.run", caps::WORKFLOW_INVOKE).not_idempotent().fails(LOOKUP),
    gw("workflow.activate", caps::WORKFLOW_INVOKE).fails(LOOKUP),
    gw("workflow.get_execution", caps::WORKFLOW_INVOKE).read_only().fails(LOOKUP),
    gw("workflow.list_executions", caps::WORKFLOW_INVOKE).read_only().fails(LOOKUP),
    gw("scenario.apply_ops", caps::WORKFLOW_EDIT).req(&["ops"]).fails(INPUT),
    gw("goal.propose_plan", caps::WORKFLOW_EDIT).no_action().model().fails(MODEL),
    gw("goal.materialize_plan", caps::WORKFLOW_EDIT).not_idempotent(),
    gw("campaign.bootstrap_plan", caps::WORKFLOW_EDIT).no_action(),
    // ── connections ──────────────────────────────────────────────────────
    gw("connection.list", caps::ARTIFACT_WRITE).read_only(),
    gw("connection.test", caps::ARTIFACT_WRITE).read_only().fails(NET_AUTH),
    gw("telegram.accounts.list", caps::ARTIFACT_WRITE).read_only(),
    // ── research ─────────────────────────────────────────────────────────
    gw("research.search", caps::RESEARCH_WRITE).cost(CostClass::External).fails(NET),
    gw("research.fetch", caps::RESEARCH_WRITE).no_action().req(&["url"]).fails(NET),
    gw("research.note", caps::RESEARCH_WRITE).not_idempotent().req(&["content"]),
    gw("research.competitors", caps::RESEARCH_WRITE).model().fails(MODEL),
    gw("research.audience", caps::RESEARCH_WRITE).model().fails(MODEL),
    gw("research.pains", caps::RESEARCH_WRITE).model().fails(MODEL),
    // ── content / ideas / strategy ───────────────────────────────────────
    gw("idea.generate", caps::ARTIFACT_WRITE).model().not_idempotent().fails(MODEL),
    gw("idea.expand", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("idea.combine", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("idea.validate", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("idea.research", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("strategy.create", caps::ARTIFACT_WRITE).model().not_idempotent().fails(MODEL),
    gw("strategy.update", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("brief.create", caps::ARTIFACT_WRITE).model().not_idempotent().fails(MODEL),
    gw("content.create", caps::ARTIFACT_WRITE)
        .aliases(&["content.write"])
        .model()
        .not_idempotent()
        .fails(MODEL),
    gw("content.rewrite", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("content.expand", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("content.shorten", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("content.adapt", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("content.translate", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("content.critique", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("content.fact_check", caps::ARTIFACT_WRITE).model().fails(MODEL),
    gw("content.create_draft", caps::ARTIFACT_WRITE).not_idempotent(),
    gw("content.variant", caps::ARTIFACT_WRITE).model().not_idempotent().fails(MODEL),
    gw("analytics.read", caps::ARTIFACT_WRITE).read_only(),
    gw("insight.create", caps::ARTIFACT_WRITE).model().not_idempotent().fails(MODEL),
    gw("analytics.analyze", caps::ARTIFACT_WRITE).unavailable(),
    gw("experiment.create", caps::ARTIFACT_WRITE).unavailable(),
    gw("experiment.evaluate", caps::ARTIFACT_WRITE).unavailable(),
    gw("asset.generate", caps::ARTIFACT_WRITE)
        .model()
        .not_idempotent()
        .req(&["prompt|text"])
        .fails(MODEL),
    gw("asset.attach", caps::ARTIFACT_WRITE).req(&["uri"]).fails(INPUT),
    // ── publishing ───────────────────────────────────────────────────────
    gw("publish.prepare", caps::ARTIFACT_WRITE).req(&["content_id"]).fails(LOOKUP),
    gw("publish.send", caps::TELEGRAM_WRITE)
        .external()
        .not_idempotent()
        .req(&["content_id"])
        .fails(NET_AUTH),
    gw("publish.schedule", caps::TELEGRAM_WRITE).not_idempotent().fails(LOOKUP),
    gw("publisher.telegram.prepare", caps::ARTIFACT_WRITE).req(&["content_id"]).fails(LOOKUP),
    gw("publisher.telegram.send", caps::TELEGRAM_WRITE)
        .aliases(&["telegram.publish"])
        .external()
        .not_idempotent()
        .req(&["content_id"])
        .fails(NET_AUTH),
    // ── leads / CRM ──────────────────────────────────────────────────────
    gw("lead.create", caps::RESEARCH_WRITE).not_idempotent().fails(INPUT),
    gw("lead.update", caps::RESEARCH_WRITE).fails(LOOKUP),
    gw("lead.score", caps::RESEARCH_WRITE).no_action().fails(LOOKUP),
    gw("conversation.classify", caps::RESEARCH_WRITE).no_action(),
    gw("marketing.simulate_inbound", caps::RESEARCH_WRITE).not_idempotent(),
    gw("analytics.campaign_metrics", caps::RESEARCH_WRITE)
        .aliases(&["analytics.query"])
        .read_only(),
    // ── custom python tools ──────────────────────────────────────────────
    gw("tool.create", caps::TOOL_BUILD)
        .aliases(&["python.create", "create_tool"])
        .not_idempotent()
        .req(&["code", "name"])
        .fails(INPUT),
    gw("tool.generate", caps::TOOL_BUILD)
        .aliases(&["generate_tool"])
        .model()
        .not_idempotent()
        .fails(MODEL),
    gw("tool.update", caps::TOOL_BUILD).aliases(&["update_tool"]).fails(LOOKUP),
    gw("tool.test", caps::TOOL_BUILD)
        .aliases(&["python.validate", "test_tool"])
        .fails(LOOKUP),
    gw("tool.delete", caps::TOOL_BUILD)
        .aliases(&["delete_tool"])
        .fails(LOOKUP),
    gw("tool.find", caps::TOOL_BUILD).aliases(&["find_tools"]).read_only(),
    // `tool.run` is both a graph node and a gateway tool; one entry.
    tool("tool.run", caps::TOOL_EXECUTE, K::Action, ToolSource::Node)
        .aliases(&["python.run", "run_tool"])
        .not_idempotent()
        .fails(&[Timeout, InvalidInput, NotFound]),
    gw("tool.list", caps::TOOL_EXECUTE).aliases(&["list_tools"]).read_only(),
    gw("tool.get", caps::TOOL_EXECUTE).aliases(&["get_tool"]).read_only().fails(LOOKUP),
];

pub fn all_tools() -> &'static [ToolDescriptor] {
    CATALOG
}

/// Descriptor for a canonical id or any alias.
pub fn lookup_tool(type_id: &str) -> Option<&'static ToolDescriptor> {
    let canonical = super::goal_runtime::resolve_tool_alias(type_id);
    CATALOG
        .iter()
        .find(|d| d.type_id == canonical)
        .or_else(|| CATALOG.iter().find(|d| d.aliases.contains(&type_id)))
}

fn present(config: &Value, key: &str) -> bool {
    match config.get(key) {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(_) => true,
    }
}

/// Required inputs absent from `config` (each `"a|b"` is satisfied by either).
pub fn missing_required_inputs(d: &ToolDescriptor, config: &Value) -> Vec<String> {
    d.required_inputs
        .iter()
        .filter(|req| !req.split('|').any(|k| present(config, k)))
        .map(|req| req.replace('|', " or "))
        .collect()
}

/// Checks a tool result against the descriptor's [`Verification`].
pub fn verify_result(d: &ToolDescriptor, result: &Value) -> Result<(), String> {
    match d.verification {
        Verification::None => Ok(()),
        Verification::NonNull if result.is_null() => {
            Err(format!("`{}` returned no result", d.type_id))
        }
        Verification::NonNull => Ok(()),
        Verification::Keys(keys) => {
            let Some(obj) = result.as_object() else {
                return Err(format!("`{}` returned a non-object result", d.type_id));
            };
            let missing: Vec<&str> = keys.iter().copied().filter(|k| !obj.contains_key(*k)).collect();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(format!("`{}` result lacks {}", d.type_id, missing.join(", ")))
            }
        }
    }
}

/// Why a result was rejected, and whether repeating the call may help.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationFailure {
    pub tool: String,
    pub reason: String,
    pub mode: FailureMode,
    /// Transient failure of an idempotent tool: a retry cannot double an effect.
    pub retryable: bool,
}

impl std::fmt::Display for VerificationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "verification failed: {}", self.reason)
    }
}

impl VerificationFailure {
    pub fn new(d: &ToolDescriptor, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        let mode = classify_failure(&reason);
        let transient = matches!(
            mode,
            FailureMode::Timeout | FailureMode::RateLimited | FailureMode::Upstream
        );
        Self {
            tool: d.type_id.to_string(),
            retryable: transient && d.idempotent && !d.irreversible,
            reason,
            mode,
        }
    }
}

/// `code` as a standalone number inside `msg` ("HTTP 429", "status=404").
fn has_code(msg: &str, code: &str) -> bool {
    msg.match_indices(code).any(|(i, _)| {
        let before = msg[..i].chars().next_back();
        let after = msg[i + code.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_digit()) && !after.is_some_and(|c| c.is_ascii_digit())
    })
}

/// Maps an error message to a [`FailureMode`]; unknown errors are `Upstream`.
pub fn classify_failure(msg: &str) -> FailureMode {
    let m = msg.to_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if any(&["timeout", "timed out", "deadline exceeded"]) || has_code(&m, "408") {
        FailureMode::Timeout
    } else if any(&["rate limit", "rate_limit", "too many requests", "flood"]) || has_code(&m, "429") {
        FailureMode::RateLimited
    } else if any(&["unauthor", "forbidden", "permission denied", "auth", "access denied"])
        || has_code(&m, "401")
        || has_code(&m, "403")
    {
        FailureMode::Auth
    } else if any(&["not found", "not_found", "no such"]) || has_code(&m, "404") || has_code(&m, "410") {
        FailureMode::NotFound
    } else if any(&["invalid", "missing", "required", "bad request", "malformed", "parse"])
        || has_code(&m, "400")
        || has_code(&m, "422")
    {
        FailureMode::InvalidInput
    } else {
        FailureMode::Upstream
    }
}

/// A failure the tool reported inside a successful return: `"ok": false` or
/// `"delivered": false` at the top level (nodes configured with
/// `continue_on_error` / `fail_on_error: false` answer this way).
pub fn reported_failure(result: &Value) -> Option<String> {
    let obj = result.as_object()?;
    let flag = ["ok", "delivered"]
        .into_iter()
        .find(|k| obj.get(*k) == Some(&Value::Bool(false)))?;
    let detail = ["error", "message"]
        .into_iter()
        .filter_map(|k| obj.get(k).and_then(Value::as_str))
        .find(|e| !e.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("reported {flag}=false"));
    Some(detail)
}

/// Result is usable as a success: passes [`verify_result`] and carries no
/// [`reported_failure`]. Tools with [`Verification::None`] are not inspected.
pub fn verify_success(d: &ToolDescriptor, result: &Value) -> Result<(), VerificationFailure> {
    verify_result(d, result).map_err(|reason| VerificationFailure::new(d, reason))?;
    if d.verification == Verification::None {
        return Ok(());
    }
    match reported_failure(result) {
        Some(detail) => Err(VerificationFailure::new(
            d,
            format!("`{}` reported failure: {detail}", d.type_id),
        )),
        None => Ok(()),
    }
}

/// Must take effect at most once per input: undoable effects, and external
/// actions that would repeat on a second call (a resend is a second message).
pub fn requires_single_execution(d: &ToolDescriptor) -> bool {
    d.irreversible || (d.side_effect == SideEffect::External && !d.idempotent)
}

/// The upstream refused the call outright, so it changed nothing and may be
/// attempted again. Timeouts and unknown errors leave the outcome open.
pub fn failure_had_no_effect(mode: FailureMode) -> bool {
    matches!(
        mode,
        FailureMode::RateLimited
            | FailureMode::Auth
            | FailureMode::InvalidInput
            | FailureMode::NotFound
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn single_execution_covers_sends_and_irreversible_only() {
        for id in [
            "telegram.send_message",
            "telegram.user.send_message",
            "publisher.telegram.send",
            "telegram.user.delete_messages",
            "projecthub.command",
        ] {
            assert!(requires_single_execution(lookup_tool(id).unwrap()), "{id}");
        }
        for id in ["web.search", "memory.read", "tool.run", "lead.create"] {
            assert!(!requires_single_execution(lookup_tool(id).unwrap()), "{id}");
        }
    }

    #[test]
    fn only_refusals_are_known_to_have_no_effect() {
        assert!(failure_had_no_effect(classify_failure("HTTP 429")));
        assert!(failure_had_no_effect(classify_failure("chat not found")));
        assert!(!failure_had_no_effect(classify_failure("request timed out")));
        assert!(!failure_had_no_effect(classify_failure("connection reset")));
    }

    #[test]
    fn ids_and_aliases_are_unique() {
        let mut seen = HashSet::new();
        for d in CATALOG {
            assert!(seen.insert(d.type_id), "duplicate tool `{}`", d.type_id);
        }
        for d in CATALOG {
            for a in d.aliases {
                assert!(seen.insert(a), "alias `{a}` collides");
                assert_eq!(lookup_tool(a).unwrap().type_id, d.type_id);
            }
        }
    }

    #[test]
    fn resolve_alias_targets_are_catalogued() {
        for alias in [
            "content.write",
            "telegram.publish",
            "python.create",
            "create_tool",
            "python.run",
            "run_tool",
            "python.validate",
            "test_tool",
            "memory.save",
            "analytics.query",
        ] {
            let d = lookup_tool(alias).unwrap_or_else(|| panic!("alias `{alias}` unresolved"));
            assert_eq!(
                d.type_id,
                super::super::goal_runtime::resolve_tool_alias(alias),
                "{alias}"
            );
        }
    }

    #[test]
    fn domain_capability_tables_agree_with_catalog() {
        for d in CATALOG {
            for cap in [
                super::super::custom_tool::custom_tool_capability(d.type_id),
                super::super::content::content_capability_for_tool(d.type_id),
                super::super::marketing::marketing_capability_for_tool(d.type_id),
            ]
            .into_iter()
            .flatten()
            {
                // Custom-tool check runs first in the old order, then content,
                // then marketing; the first hit wins.
                let first = super::super::custom_tool::custom_tool_capability(d.type_id)
                    .or_else(|| super::super::content::content_capability_for_tool(d.type_id))
                    .or_else(|| super::super::marketing::marketing_capability_for_tool(d.type_id))
                    .unwrap();
                assert_eq!(d.capability, first, "`{}` (domain says {cap})", d.type_id);
            }
        }
    }

    #[test]
    fn legacy_capability_table_preserved() {
        let cap = super::super::goal_runtime::capability_for_tool;
        for (id, want) in [
            ("web.search", caps::WEB_SEARCH),
            ("web.fetch", caps::WEB_FETCH),
            ("web.open", caps::WEB_FETCH),
            ("web.extract", caps::WEB_FETCH),
            ("http.request", caps::HTTP_REQUEST),
            ("ai.chat", caps::AI_GENERATE),
            ("ai.generate", caps::AI_GENERATE),
            ("ai.image", caps::AI_GENERATE),
            ("ai.audio", caps::AI_GENERATE),
            ("ai.video", caps::AI_GENERATE),
            ("ai.analyze", caps::AI_ANALYZE),
            ("ai.classify", caps::AI_ANALYZE),
            ("telegram.send_message", caps::TELEGRAM_WRITE),
            ("telegram.send_photo", caps::TELEGRAM_WRITE),
            ("telegram.send_document", caps::TELEGRAM_WRITE),
            ("telegram.user.send_message", caps::TELEGRAM_WRITE),
            ("publisher.telegram.send", caps::TELEGRAM_WRITE),
            ("telegram.publish", caps::TELEGRAM_WRITE),
            ("publisher.telegram.prepare", caps::ARTIFACT_WRITE),
            ("github.get_latest_release", caps::GITHUB_READ),
            ("debug.log", caps::ARTIFACT_WRITE),
            ("memory.write", caps::ARTIFACT_WRITE),
            ("memory.save", caps::ARTIFACT_WRITE),
            ("memory.read", caps::ARTIFACT_WRITE),
            ("analytics.record_lead", caps::RESEARCH_WRITE),
            ("analytics.record_progress", caps::RESEARCH_WRITE),
            ("analytics.campaign_metrics", caps::RESEARCH_WRITE),
            ("analytics.query", caps::RESEARCH_WRITE),
            ("analytics.read", caps::ARTIFACT_WRITE),
            ("workflow.list", caps::WORKFLOW_EDIT),
            ("workflow.get", caps::WORKFLOW_EDIT),
            ("workflow.create", caps::WORKFLOW_EDIT),
            ("workflow.update", caps::WORKFLOW_EDIT),
            ("workflow.validate", caps::WORKFLOW_EDIT),
            ("scenario.apply_ops", caps::WORKFLOW_EDIT),
            ("goal.propose_plan", caps::WORKFLOW_EDIT),
            ("goal.materialize_plan", caps::WORKFLOW_EDIT),
            ("campaign.bootstrap_plan", caps::WORKFLOW_EDIT),
            ("workflow.run", caps::WORKFLOW_INVOKE),
            ("workflow.activate", caps::WORKFLOW_INVOKE),
            ("workflow.get_execution", caps::WORKFLOW_INVOKE),
            ("workflow.list_executions", caps::WORKFLOW_INVOKE),
            ("connection.list", caps::ARTIFACT_WRITE),
            ("connection.test", caps::ARTIFACT_WRITE),
            ("telegram.accounts.list", caps::ARTIFACT_WRITE),
            ("tool.create", caps::TOOL_BUILD),
            ("python.create", caps::TOOL_BUILD),
            ("tool.run", caps::TOOL_EXECUTE),
            ("python.run", caps::TOOL_EXECUTE),
            ("tool.list", caps::TOOL_EXECUTE),
            ("content.create", caps::ARTIFACT_WRITE),
            ("content.write", caps::ARTIFACT_WRITE),
            ("publish.send", caps::TELEGRAM_WRITE),
            ("lead.score", caps::RESEARCH_WRITE),
            ("research.search", caps::RESEARCH_WRITE),
            ("logic.condition", caps::HTTP_REQUEST),
            ("trigger.manual", caps::HTTP_REQUEST),
            ("totally.unknown", caps::HTTP_REQUEST),
        ] {
            assert_eq!(cap(id), want, "{id}");
        }
        // Previously fell through to http.request; now scoped properly.
        assert_eq!(cap("telegram.user.delete_messages"), caps::TELEGRAM_WRITE);
        assert_eq!(cap("telegram.user.get_chat_members"), caps::TELEGRAM_READ);
        assert_eq!(cap("projecthub.command"), caps::GITHUB_WRITE);
        assert_eq!(cap("projecthub.code"), caps::GITHUB_READ);
    }

    #[test]
    fn legacy_action_counting_preserved() {
        let counts = super::super::goal_runtime::tool_counts_as_action;
        for id in [
            "workflow.list",
            "workflow.get",
            "workflow.validate",
            "workflow.get_execution",
            "workflow.list_executions",
            "memory.read",
            "connection.list",
            "connection.test",
            "telegram.accounts.list",
            "goal.propose_plan",
            "campaign.bootstrap_plan",
            "research.fetch",
            "analytics.campaign_metrics",
            "analytics.read",
            "analytics.query",
            "lead.score",
            "conversation.classify",
        ] {
            assert!(!counts(id), "{id} must not count");
        }
        for id in [
            "memory.write",
            "workflow.create",
            "workflow.run",
            "web.search",
            "telegram.send_message",
            "publish.send",
            "tool.run",
            "lead.create",
            "totally.unknown",
        ] {
            assert!(counts(id), "{id} must count");
        }
    }

    #[test]
    fn templates_only_use_runnable_tools() {
        for t in super::super::templates::all() {
            for tool in &t.tools {
                let d = lookup_tool(&tool.type_id)
                    .unwrap_or_else(|| panic!("template tool `{}` not catalogued", tool.type_id));
                assert!(d.available, "template tool `{}` has no executor", tool.type_id);
            }
        }
    }

    #[test]
    fn sends_are_not_idempotent_and_deletes_irreversible() {
        for id in [
            "telegram.send_message",
            "telegram.send_photo",
            "telegram.send_document",
            "telegram.user.send_message",
            "telegram.user.forward_message",
            "publish.send",
            "publisher.telegram.send",
        ] {
            let d = lookup_tool(id).unwrap();
            assert_eq!(d.side_effect, SideEffect::External, "{id}");
            assert!(!d.idempotent, "{id}");
        }
        for id in ["telegram.user.delete_messages", "projecthub.command"] {
            let d = lookup_tool(id).unwrap();
            assert!(d.irreversible && !d.idempotent, "{id}");
        }
        assert!(CATALOG.iter().filter(|d| d.irreversible).all(|d| !d.idempotent));
    }

    #[test]
    fn required_inputs_alternatives_and_blanks() {
        let http = lookup_tool("http.request").unwrap();
        assert_eq!(missing_required_inputs(http, &json!({})), vec!["url or connection_id"]);
        assert!(missing_required_inputs(http, &json!({"connection_id": "c"})).is_empty());
        assert!(missing_required_inputs(http, &json!({"url": "https://x"})).is_empty());
        assert_eq!(missing_required_inputs(http, &json!({"url": "  ", "connection_id": null})).len(), 1);

        let create = lookup_tool("python.create").unwrap();
        assert_eq!(
            missing_required_inputs(create, &json!({"name": "n"})),
            vec!["code".to_string()]
        );
        assert_eq!(missing_required_inputs(create, &Value::Null).len(), 2);
        let ops = lookup_tool("scenario.apply_ops").unwrap();
        assert!(missing_required_inputs(ops, &json!({"ops": []})).is_empty());
    }

    #[test]
    fn verification_modes() {
        let http = lookup_tool("http.request").unwrap();
        assert!(verify_result(http, &json!({"status": 200})).is_ok());
        assert!(verify_result(http, &json!({"body": ""})).unwrap_err().contains("status"));
        assert!(verify_result(http, &json!("x")).is_err());
        let chat = lookup_tool("ai.chat").unwrap();
        assert!(verify_result(chat, &Value::Null).is_err());
        assert!(verify_result(chat, &json!("hi")).is_ok());
        let log = lookup_tool("debug.log").unwrap();
        assert!(verify_result(log, &Value::Null).is_ok());
    }

    #[test]
    fn catalog_serializes() {
        let v = serde_json::to_value(all_tools()).unwrap();
        let first = &v[0];
        assert_eq!(first["type_id"], "trigger.manual");
        assert_eq!(first["kind"], "trigger");
        assert_eq!(first["source"], "node");
        let http = v
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["type_id"] == "http.request")
            .unwrap();
        assert_eq!(http["verification"], json!({"keys": ["status"]}));
        assert_eq!(http["side_effect"], "external");
    }

    #[test]
    fn failure_classification() {
        assert_eq!(classify_failure("request timed out after 30s"), FailureMode::Timeout);
        assert_eq!(classify_failure("rate limited (HTTP 429) for x"), FailureMode::RateLimited);
        assert_eq!(classify_failure("HTTP 403: {}"), FailureMode::Auth);
        assert_eq!(classify_failure("HTTP 404: nope"), FailureMode::NotFound);
        assert_eq!(classify_failure("missing chat_id"), FailureMode::InvalidInput);
        assert_eq!(classify_failure("connection reset"), FailureMode::Upstream);
        // Digits inside a longer number are not status codes.
        assert_eq!(classify_failure("order 14290 failed"), FailureMode::Upstream);
    }

    #[test]
    fn reported_failures_are_not_success() {
        let alert = lookup_tool("alertwim.alert").unwrap();
        let soft = json!({"delivered": false, "error": "window closed"});
        let err = verify_success(alert, &soft).unwrap_err();
        assert!(err.reason.contains("window closed"), "{err}");
        assert!(verify_success(alert, &json!({"delivered": true})).is_ok());
        assert_eq!(reported_failure(&json!({"ok": false})).as_deref(), Some("reported ok=false"));
        assert_eq!(
            reported_failure(&json!({"ok": false, "message": "401 from api"})).as_deref(),
            Some("401 from api")
        );
        // Describing another run's error is data, not a failure.
        assert_eq!(reported_failure(&json!({"status": "failed", "error": "x"})), None);
        // Tools without verification are never inspected.
        let log = lookup_tool("debug.log").unwrap();
        assert!(verify_success(log, &json!({"ok": false})).is_ok());
    }

    #[test]
    fn retry_only_transient_idempotent() {
        let http = lookup_tool("http.request").unwrap();
        assert!(!http.idempotent);
        assert!(!VerificationFailure::new(http, "timeout").retryable);
        let search = lookup_tool("web.search").unwrap();
        assert!(search.idempotent);
        assert!(VerificationFailure::new(search, "HTTP 503").retryable);
        assert!(!VerificationFailure::new(search, "HTTP 401").retryable);
        let shape = verify_success(http, &json!({"body": ""})).unwrap_err();
        assert!(shape.to_string().starts_with("verification failed:"), "{shape}");
    }
}
