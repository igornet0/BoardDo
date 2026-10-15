//! Build → test → repair loop for Python tools.
//!
//! A version is activated only when its source passes the static check and
//! every test case runs, matches the output schema and the expected values.
//! Failures go to a [`ToolRepairer`] (AI) — or to small deterministic fixes —
//! for up to [`MAX_REPAIR_ATTEMPTS`] rounds.

use async_trait::async_trait;
use boarddo_shared::v2::{
    CreateToolRequest, CustomToolVersion, MAX_REPAIR_ATTEMPTS, ToolRepairRecord, ToolRuntimeConfig,
    ToolTestCase, ToolVersionStatus,
};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::sandbox::{self, SandboxResult};
use super::security;

pub const STATISTICS_ANALYZER_CODE: &str = r#"import statistics

def run(input_data):
    numbers = input_data.get("numbers")
    if numbers is None and isinstance(input_data.get("input"), dict):
        numbers = input_data["input"].get("numbers")
    if not numbers:
        raise ValueError("numbers required")
    return {
        "mean": statistics.mean(numbers),
        "median": statistics.median(numbers),
        "stdev": statistics.stdev(numbers) if len(numbers) > 1 else 0.0,
    }
"#;

/// Upper bounds for per-tool runtime overrides.
const MAX_TIMEOUT_SECS: u32 = 300;
const MAX_MEMORY_MB: u32 = 2048;

/// A repairer's answer to a failing round.
#[derive(Debug, Clone)]
pub enum Repair {
    /// Replacement source code.
    Code(String),
    /// The failing case is a generated test whose expectation is wrong.
    DropTest { index: usize, reason: String },
}

/// Fixes a failing tool; `Ok(None)` gives up, `Err` explains why it could not try.
#[async_trait]
pub trait ToolRepairer: Send + Sync {
    async fn repair(
        &self,
        req: &CreateToolRequest,
        code: &str,
        cases: &[ToolTestCase],
        failure: &TestFailure,
        attempt: u32,
    ) -> Result<Option<Repair>, String>;
}

/// What went wrong in one build round (passed to the repairer).
#[derive(Debug, Clone)]
pub struct TestFailure {
    pub stage: String,
    /// Index of the failing test case (`None` for the static check).
    pub case: Option<usize>,
    pub input: Value,
    pub result: SandboxResult,
    /// Human-readable reason (schema / expectation mismatch or the error).
    pub reason: String,
}

pub struct BuildOutcome {
    pub code: String,
    pub test_cases: Vec<ToolTestCase>,
    pub repair_history: Vec<ToolRepairRecord>,
    pub last_sandbox: SandboxResult,
    pub activated: bool,
    /// Why the last round failed (when not activated).
    pub failure: Option<String>,
}

pub fn runtime_config(req: &CreateToolRequest) -> ToolRuntimeConfig {
    let mut cfg = req.runtime.clone().unwrap_or_default();
    cfg.timeout_secs = cfg.timeout_secs.clamp(1, MAX_TIMEOUT_SECS);
    cfg.memory_mb = cfg.memory_mb.clamp(32, MAX_MEMORY_MB);
    cfg
}

pub fn default_test_input(req: &CreateToolRequest) -> Value {
    req.test_input.clone().unwrap_or_else(|| {
        if req.name.contains("statistic") || req.description.to_lowercase().contains("median") {
            json!({ "numbers": [1, 2, 3, 4, 5] })
        } else {
            json!({})
        }
    })
}

pub fn default_output_schema(req: &CreateToolRequest) -> Value {
    if (req.output_schema.is_null() || req.output_schema.as_object().is_none()) && req.name.contains("statistic") {
        return json!({ "type": "object", "required": ["mean", "median", "stdev"] });
    }
    req.output_schema.clone()
}

/// The request's test cases: `test_input` (schema only) plus explicit `tests`.
pub fn test_cases(req: &CreateToolRequest) -> Vec<ToolTestCase> {
    let schema = default_output_schema(req);
    let mut cases = Vec::new();
    if req.test_input.is_some() || req.tests.is_empty() {
        cases.push(ToolTestCase {
            input: default_test_input(req),
            expected_schema: schema.clone(),
            expected: None,
            source: Some("user".into()),
        });
    }
    for t in &req.tests {
        cases.push(ToolTestCase {
            input: t.input.clone(),
            expected_schema: if t.expected_schema.is_null() { schema.clone() } else { t.expected_schema.clone() },
            expected: t.expected.clone(),
            source: Some(t.source.clone().unwrap_or_else(|| "user".into())),
        });
    }
    cases
}

pub async fn build_and_test_version(code: String, req: &CreateToolRequest) -> Result<BuildOutcome, String> {
    build_with_repairer(code, req, None).await
}

pub async fn build_with_repairer(
    mut code: String,
    req: &CreateToolRequest,
    repairer: Option<&dyn ToolRepairer>,
) -> Result<BuildOutcome, String> {
    security::validate_name(&req.name)?;
    security::validate_requirements(&req.requirements)?;
    let mut cases = test_cases(req);
    let config = runtime_config(req);
    let mut repair_history = Vec::new();

    for attempt in 0..=MAX_REPAIR_ATTEMPTS {
        let (last, failure) = match run_round(&code, req, &cases, &config).await {
            Ok(last) => {
                return Ok(BuildOutcome {
                    code,
                    test_cases: cases,
                    repair_history,
                    last_sandbox: last,
                    activated: true,
                    failure: None,
                });
            }
            Err(f) => (f.result.clone(), f),
        };
        let give_up = |code: String, cases: Vec<ToolTestCase>, history, note: Option<String>| BuildOutcome {
            code,
            test_cases: cases,
            repair_history: history,
            last_sandbox: last.clone(),
            activated: false,
            failure: Some(match note {
                Some(n) => format!("{} ({n})", failure.reason),
                None => failure.reason.clone(),
            }),
        };
        if attempt >= MAX_REPAIR_ATTEMPTS {
            return Ok(give_up(code, cases, repair_history, None));
        }
        let (repair, by) = match auto_fix(&code, &failure.result) {
            Some(fixed) => (Ok(Some(Repair::Code(fixed))), "auto"),
            None => match repairer {
                Some(r) => (r.repair(req, &code, &cases, &failure, attempt + 1).await, "ai"),
                None => (Ok(None), "none"),
            },
        };
        let repair = match repair {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(give_up(code, cases, repair_history, None)),
            Err(e) => return Ok(give_up(code, cases, repair_history, Some(format!("repair failed: {e}")))),
        };
        let mut record = ToolRepairRecord {
            attempt: attempt + 1,
            error_message: failure.reason.clone(),
            traceback: failure.result.traceback.clone(),
            fixed_by: Some(by.into()),
            at: Utc::now(),
        };
        match repair {
            Repair::Code(fixed) if fixed.trim() != code.trim() => code = fixed,
            Repair::Code(_) => return Ok(give_up(code, cases, repair_history, Some("the repair returned the same code".into()))),
            Repair::DropTest { index, reason } => {
                // Only generated tests may go; the user's examples define correctness.
                let droppable = failure.case == Some(index)
                    && cases.get(index).is_some_and(|c| c.source.as_deref() == Some("ai"))
                    && cases.len() > 1;
                if !droppable {
                    return Ok(give_up(code, cases, repair_history, Some(format!("refused to drop test #{}", index + 1))));
                }
                cases.remove(index);
                record.error_message = format!("{} → dropped generated test #{}: {reason}", failure.reason, index + 1);
            }
        }
        repair_history.push(record);
    }
    unreachable!("the loop returns on its last attempt")
}

/// Static check + every test case. `Err` carries the first failure.
async fn run_round(
    code: &str,
    req: &CreateToolRequest,
    cases: &[ToolTestCase],
    config: &ToolRuntimeConfig,
) -> Result<SandboxResult, TestFailure> {
    if let Err(reason) = sandbox::check_source(code, &req.permissions).await {
        let kind = reason.split(':').next().unwrap_or("Error").to_string();
        return Err(TestFailure {
            stage: "check".into(),
            case: None,
            input: Value::Null,
            result: SandboxResult {
                success: false,
                output: None,
                error_type: Some(kind),
                message: Some(reason.clone()),
                traceback: None,
                logs: String::new(),
                duration_ms: 0,
            },
            reason,
        });
    }
    let mut last = None;
    for (i, case) in cases.iter().enumerate() {
        let result = sandbox::execute_python(code, case.input.clone(), &req.permissions, config).await;
        let stage = format!("test #{}", i + 1);
        if !result.success {
            let reason = format!(
                "{stage}: {}: {}",
                result.error_type.as_deref().unwrap_or("Error"),
                result.message.as_deref().unwrap_or("failed")
            );
            return Err(TestFailure { stage, case: Some(i), input: case.input.clone(), result, reason });
        }
        let output = result.output.clone().unwrap_or(Value::Null);
        let mismatch = validate_schema(&output, &case.expected_schema, "output")
            .err()
            .or_else(|| case.expected.as_ref().and_then(|e| matches_expected(&output, e, "output").err()));
        if let Some(why) = mismatch {
            return Err(TestFailure {
                stage: stage.clone(),
                case: Some(i),
                input: case.input.clone(),
                reason: format!("{stage}: {why}"),
                result,
            });
        }
        last = Some(result);
    }
    Ok(last.expect("at least one test case"))
}

/// Deterministic fix: a standard module used without its import.
fn auto_fix(code: &str, fail: &SandboxResult) -> Option<String> {
    const MODULES: &[&str] = &[
        "math", "statistics", "re", "json", "datetime", "collections", "itertools", "functools",
        "decimal", "random", "string", "heapq", "bisect", "csv", "io", "hashlib", "base64", "uuid", "textwrap",
    ];
    if fail.error_type.as_deref() != Some("NameError") {
        return None;
    }
    let msg = fail.message.as_deref().unwrap_or("");
    let name = msg.strip_prefix("name '")?.split('\'').next()?;
    if !MODULES.contains(&name) || code.contains(&format!("import {name}")) {
        return None;
    }
    Some(format!("import {name}\n{code}"))
}

/// A small JSON-Schema subset: `type`, `required`, `properties`, `items`, `enum`.
pub fn validate_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    let Some(schema) = schema.as_object() else { return Ok(()) };
    if let Some(ty) = schema.get("type") {
        let types: Vec<&str> = match ty {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !types.is_empty() && !types.iter().any(|t| type_matches(value, t)) {
            return Err(format!("{path} should be {} but is {}", types.join(" | "), type_name(value)));
        }
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            return Err(format!("{path} = {value} is not one of {}", Value::Array(options.clone())));
        }
    }
    if let Some(obj) = value.as_object() {
        for key in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
            let k = key.as_str().unwrap_or_default();
            if !obj.contains_key(k) {
                return Err(format!("{path} is missing field `{k}`"));
            }
        }
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (k, sub) in props {
                if let Some(v) = obj.get(k) {
                    validate_schema(v, sub, &format!("{path}.{k}"))?;
                }
            }
        }
    } else if schema.get("required").and_then(Value::as_array).is_some_and(|r| !r.is_empty()) {
        return Err(format!("{path} should be an object but is {}", type_name(value)));
    }
    if let (Some(items), Some(list)) = (schema.get("items"), value.as_array()) {
        for (i, v) in list.iter().enumerate() {
            validate_schema(v, items, &format!("{path}[{i}]"))?;
        }
    }
    Ok(())
}

fn type_matches(v: &Value, ty: &str) -> bool {
    match ty {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64() || v.as_f64().is_some_and(|f| f.fract() == 0.0),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        _ => true,
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `expected` must be contained in `actual` (objects as subsets, numbers with a tolerance).
pub fn matches_expected(actual: &Value, expected: &Value, path: &str) -> Result<(), String> {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            for (k, ev) in e {
                let av = a.get(k).ok_or_else(|| format!("{path} is missing field `{k}`"))?;
                matches_expected(av, ev, &format!("{path}.{k}"))?;
            }
            Ok(())
        }
        (Value::Array(a), Value::Array(e)) => {
            if a.len() != e.len() {
                return Err(format!("{path} has {} items, expected {}", a.len(), e.len()));
            }
            for (i, (av, ev)) in a.iter().zip(e).enumerate() {
                matches_expected(av, ev, &format!("{path}[{i}]"))?;
            }
            Ok(())
        }
        (Value::Number(a), Value::Number(e)) => {
            let (a, e) = (a.as_f64().unwrap_or(f64::NAN), e.as_f64().unwrap_or(f64::NAN));
            if (a - e).abs() <= 1e-9_f64.max(e.abs() * 1e-9) {
                Ok(())
            } else {
                Err(format!("{path} = {a}, expected {e}"))
            }
        }
        _ if actual == expected => Ok(()),
        _ => Err(format!("{path} = {actual}, expected {expected}")),
    }
}

pub fn version_from_build(
    tool_id: Uuid,
    version: u32,
    req: &CreateToolRequest,
    outcome: &BuildOutcome,
) -> CustomToolVersion {
    CustomToolVersion {
        id: Uuid::now_v7(),
        tool_id,
        version,
        source_code: outcome.code.clone(),
        input_schema: req.input_schema.clone(),
        output_schema: default_output_schema(req),
        permissions: req.permissions.clone(),
        requirements: req.requirements.clone(),
        runtime_config: runtime_config(req),
        status: if outcome.activated {
            ToolVersionStatus::Active
        } else {
            ToolVersionStatus::Failed
        },
        purpose: req.purpose.clone(),
        specification: json!({
            "name": req.name,
            "description": req.description,
            "requirements": req.requirements,
        }),
        test_cases: outcome.test_cases.clone(),
        repair_history: outcome.repair_history.clone(),
        created_at: Utc::now(),
    }
}
