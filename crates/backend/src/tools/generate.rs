//! Tools written by AI from a plain-language description: the model drafts the
//! code, schemas and test cases; the builder runs them in the sandbox and sends
//! failures (traceback, logs, mismatching values) back to the model to repair.

use async_trait::async_trait;
use boarddo_shared::v2::{CreateToolRequest, ToolOrigin, ToolPermissions, ToolRuntimeConfig, ToolTestCase};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::builder::{Repair, TestFailure, ToolRepairer};
use super::service::{ToolError, ToolService};
use crate::objectives::llm::{self, ObjectiveLlm};
use crate::state::SharedState;

/// Reasoning models spend much of the budget before answering.
const MAX_TOKENS: u64 = 16_000;
const RETRY_MAX_TOKENS: u64 = 32_000;
const TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GenerateToolRequest {
    /// What the tool should do (any language).
    pub description: String,
    /// Preferred `snake_case` name; generated when empty.
    #[serde(default)]
    pub name: Option<String>,
    /// Must-pass examples: input and (part of) the expected output.
    #[serde(default)]
    pub examples: Vec<ToolTestCase>,
    #[serde(default)]
    pub permissions: ToolPermissions,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub runtime: Option<ToolRuntimeConfig>,
    #[serde(default)]
    pub connection_id: Option<Uuid>,
    #[serde(default)]
    pub model: Option<String>,
}

/// The model behind generation and repair (mockable in tests).
#[async_trait]
pub trait ToolAuthor: Send + Sync {
    async fn complete(&self, system: &str, user: &str) -> Result<String, String>;
    fn label(&self) -> String;
}

#[async_trait]
impl ToolAuthor for ObjectiveLlm {
    async fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        match self.chat(system, user, 0.2, MAX_TOKENS, TIMEOUT_MS).await {
            // The whole budget went to reasoning: one more try with a larger one.
            Err(e) if e.contains("max_tokens") => self.chat(system, user, 0.2, RETRY_MAX_TOKENS, TIMEOUT_MS).await,
            other => other,
        }
    }
    fn label(&self) -> String {
        self.model.clone()
    }
}

pub async fn generate(state: &SharedState, req: GenerateToolRequest) -> Result<Value, ToolError> {
    let llm = llm::resolve_connection_for(state, req.connection_id, req.model.as_deref(), boarddo_shared::v2::TaskKind::Reasoning)
        .await
        .map_err(ToolError::Invalid)?;
    let svc = ToolService::new(state.storage.clone());
    generate_with(&svc, &llm, req).await
}

pub async fn generate_with(svc: &ToolService, author: &dyn ToolAuthor, req: GenerateToolRequest) -> Result<Value, ToolError> {
    let description = req.description.trim();
    if description.len() < 8 {
        return Err(ToolError::Invalid("describe what the tool should do (a sentence or more)".into()));
    }
    let python = python_version();
    let reply = author
        .complete(&system_prompt(&python, &req), &draft_prompt(description, &req))
        .await
        .map_err(|e| ToolError::Internal(format!("AI request failed: {e}")))?;
    let draft = parse_draft(&reply).map_err(|e| ToolError::Internal(format!("AI reply could not be used: {e}")))?;

    let base = req
        .name
        .as_deref()
        .map(sanitize_name)
        .filter(|n| n.len() >= 2)
        .or_else(|| Some(sanitize_name(&draft.name)).filter(|n| n.len() >= 2))
        .unwrap_or_else(|| "generated_tool".into());
    let name = unique_name(svc, &base).await?;

    // The user's examples are authoritative; the model's tests come after them.
    let mut tests: Vec<ToolTestCase> = req
        .examples
        .iter()
        .cloned()
        .map(|mut t| {
            t.source = Some("user".into());
            t
        })
        .collect();
    tests.extend(draft.tests.iter().cloned());
    let create = CreateToolRequest {
        name,
        description: draft.description.clone().filter(|d| !d.trim().is_empty()).unwrap_or_else(|| description.to_string()),
        language: "python".into(),
        input_schema: draft.input_schema.clone(),
        output_schema: draft.output_schema.clone(),
        requirements: req.requirements.clone(),
        code: draft.code.clone(),
        permissions: req.permissions.clone(),
        test_input: if tests.is_empty() { Some(json!({})) } else { None },
        tests,
        runtime: req.runtime.clone(),
        purpose: description.to_string(),
        tags: vec!["generated".into()],
    };
    let repairer = AiRepairer { author, description: description.to_string(), python };
    let mut created = svc
        .create_tool_with(create, "ai", ToolOrigin::Agent, Some(&repairer))
        .await?;
    created["generated"] = json!({ "model": author.label(), "tests": created["version"]["test_cases"].as_array().map(|a| a.len()) });
    Ok(created)
}

struct AiRepairer<'a> {
    author: &'a dyn ToolAuthor,
    description: String,
    python: String,
}

#[async_trait]
impl ToolRepairer for AiRepairer<'_> {
    async fn repair(
        &self,
        req: &CreateToolRequest,
        code: &str,
        cases: &[ToolTestCase],
        failure: &TestFailure,
        attempt: u32,
    ) -> Result<Option<Repair>, String> {
        let r = &failure.result;
        let tests: Vec<Value> = cases
            .iter()
            .enumerate()
            .map(|(i, c)| json!({ "index": i, "source": c.source, "input": c.input, "expected": c.expected }))
            .collect();
        let failing_ai_test = failure
            .case
            .and_then(|i| cases.get(i))
            .is_some_and(|c| c.source.as_deref() == Some("ai") && c.expected.is_some());
        let tail = |s: &str, n: usize| s.chars().rev().take(n).collect::<String>().chars().rev().collect::<String>();
        let user = format!(
            "Task: {desc}\n\nThe tool failed ({stage}, repair attempt {attempt}).\n\
             Reason: {reason}\nError type: {kind}\nMessage: {msg}\n\
             Input: {input}\nOutput: {output}\nTraceback:\n{tb}\nLogs (stderr):\n{logs}\n\
             Output schema: {schema}\nTest cases (index, source): {tests}\n\n\
             Current code:\n```python\n{code}\n```\n\n\
             Return the complete corrected module in one ```python block. Keep the `run(input_data)` \
             contract; fix the cause rather than special-casing the test input.{drop}",
            desc = self.description,
            stage = failure.stage,
            reason = failure.reason,
            kind = r.error_type.as_deref().unwrap_or("-"),
            msg = r.message.as_deref().unwrap_or("-"),
            input = tail(&failure.input.to_string(), 2000),
            output = tail(&r.output.as_ref().map(Value::to_string).unwrap_or_default(), 2000),
            tb = tail(r.traceback.as_deref().unwrap_or("-"), 3000),
            logs = tail(&r.logs, 1500),
            schema = req.output_schema,
            tests = tail(&serde_json::to_string(&tests).unwrap_or_default(), 4000),
            drop = if failing_ai_test {
                format!(
                    "\nThe failing case #{} was generated (source \"ai\"); its expected value may itself be wrong. \
                     Recompute it from the task. If the code is right and the expectation is wrong, reply instead with only \
                     ```json\n{{\"drop_test\": {}, \"reason\": \"why the expectation is wrong\"}}\n``` \
                     (never for source \"user\" tests).",
                    failure.case.unwrap_or(0) + 1,
                    failure.case.unwrap_or(0)
                )
            } else {
                String::new()
            },
        );
        let system = system_prompt(&self.python, &GenerateToolRequest {
            permissions: req.permissions.clone(),
            requirements: req.requirements.clone(),
            ..Default::default()
        });
        let reply = self.author.complete(&system, &user).await?;
        if let Some(index) = fenced(&reply, "json")
            .and_then(|j| serde_json::from_str::<Value>(&j).ok())
            .and_then(|v| Some((v.get("drop_test")?.as_u64()? as usize, v["reason"].as_str().unwrap_or("").to_string())))
        {
            return Ok(Some(Repair::DropTest { index: index.0, reason: index.1 }));
        }
        Ok(fenced(&reply, "python").or_else(|| fenced(&reply, "py")).map(Repair::Code))
    }
}

fn system_prompt(python: &str, req: &GenerateToolRequest) -> String {
    let p = &req.permissions;
    let allowed = |b: bool| if b { "allowed" } else { "NOT allowed" };
    format!(
        "You write tools for the BoardDo automation platform. A tool is one Python module run in a sandbox \
         with {python}.\n\
         Contract: define a top-level `run(input_data)` that receives the JSON-decoded input (usually a dict) \
         and returns JSON-serializable data (prefer a dict; no datetime/set/Decimal/NaN in the result — convert them).\n\
         Rules:\n\
         - Standard library only{pkgs}; anything else is not installed and fails to import.\n\
         - Forbidden: eval, exec, compile, __import__, globals/locals, dunder attributes other than __init__/__name__, \
           importlib, ctypes, pickle, inspect, gc, builtins, sys.modules, threads/multiprocessing.\n\
         - Network: {net}. Shell/subprocess: {shell}. Reading files outside the working directory: {read}. Writing outside it: {write}.\n\
         - Validate the input and raise ValueError with a clear message for missing or invalid fields.\n\
         - Deterministic: no randomness or current time unless the task needs it.\n\
         - Compatible with the stated Python version (e.g. no `match`, no `list[int]` annotations on 3.8).\n\
         Write the description and messages in the language of the task.",
        pkgs = {
            let mut extra: Vec<String> = super::runtime::current()
                .map(|i| i.packages.into_iter().filter(|(_, ok)| *ok).map(|(k, _)| k).collect())
                .unwrap_or_default();
            extra.extend(req.requirements.iter().cloned());
            extra.sort();
            extra.dedup();
            if extra.is_empty() { String::new() } else { format!(" plus installed: {}", extra.join(", ")) }
        },
        net = allowed(p.network),
        shell = allowed(p.shell),
        read = allowed(p.filesystem_read),
        write = allowed(p.filesystem_write),
    )
}

fn draft_prompt(description: &str, req: &GenerateToolRequest) -> String {
    let examples = if req.examples.is_empty() {
        String::new()
    } else {
        format!(
            "\nThe tool MUST pass these examples (input → expected output fields):\n{}\n",
            serde_json::to_string_pretty(&req.examples).unwrap_or_default()
        )
    };
    format!(
        "Task: {description}\n{examples}\n\
         Reply with exactly two fenced blocks, nothing else:\n\
         ```json\n{{\"name\": \"snake_case_name\", \"description\": \"one sentence\", \
         \"input_schema\": {{JSON Schema of input_data}}, \"output_schema\": {{JSON Schema with type/required/properties}}, \
         \"tests\": [{{\"input\": {{...}}, \"expected\": {{...}}}}]}}\n```\n\
         ```python\n<the module>\n```\n\
         tests: 2–4 cases (typical + edge). `expected` lists only output fields whose exact values you are certain of; \
         compute them carefully. Do not include cases that should raise errors."
    )
}

#[derive(Debug, Clone)]
struct Draft {
    name: String,
    description: Option<String>,
    input_schema: Value,
    output_schema: Value,
    tests: Vec<ToolTestCase>,
    code: String,
}

fn parse_draft(reply: &str) -> Result<Draft, String> {
    let code = fenced(reply, "python")
        .or_else(|| fenced(reply, "py"))
        .ok_or("no ```python block")?;
    let spec: Value = fenced(reply, "json")
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_else(|| json!({}));
    let tests = spec["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let input = t.get("input")?.clone();
            Some(ToolTestCase {
                input,
                expected_schema: Value::Null,
                expected: t.get("expected").cloned().filter(|e| !e.is_null()),
                source: Some("ai".into()),
            })
        })
        .take(6)
        .collect();
    Ok(Draft {
        name: spec["name"].as_str().unwrap_or_default().to_string(),
        description: spec["description"].as_str().map(str::to_string),
        input_schema: spec.get("input_schema").cloned().unwrap_or(Value::Null),
        output_schema: spec.get("output_schema").cloned().filter(Value::is_object).unwrap_or(Value::Null),
        tests,
        code,
    })
}

/// Content of the first ```lang fenced block.
fn fenced(text: &str, lang: &str) -> Option<String> {
    let open = format!("```{lang}");
    let mut rest = text;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        // `lang` must end the fence's info string (```py must not match ```python).
        if after.starts_with(|c: char| c.is_alphanumeric()) {
            rest = after;
            continue;
        }
        let body = after.split_once('\n').map(|(_, b)| b).unwrap_or("");
        let end = body.find("```").unwrap_or(body.len());
        let block = body[..end].trim_end().to_string();
        return (!block.trim().is_empty()).then_some(block);
    }
    None
}

pub fn sanitize_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    let out = if out.starts_with(|c: char| c.is_ascii_digit()) { format!("tool_{out}") } else { out };
    out.chars().take(56).collect()
}

async fn unique_name(svc: &ToolService, base: &str) -> Result<String, ToolError> {
    for i in 1..100 {
        let name = if i == 1 { base.to_string() } else { format!("{base}_{i}") };
        if matches!(svc.resolve_tool(&name).await, Err(ToolError::NotFound(_))) {
            return Ok(name);
        }
    }
    Err(ToolError::Conflict(format!("no free name for `{base}`")))
}

/// "Python 3.14.7 (standard library only)" / "… with numpy, pandas" — what the model may rely on.
fn python_version() -> String {
    super::runtime::current().map(|i| i.label()).unwrap_or_else(|| "Python 3".into())
}
