use std::sync::Mutex;

use async_trait::async_trait;
use boarddo_shared::v2::{CreateToolRequest, ToolOrigin, ToolPermissions, ToolRuntimeConfig, ToolTestCase};
use serde_json::{Value, json};

use super::builder::{self, Repair, STATISTICS_ANALYZER_CODE, TestFailure, ToolRepairer};
use super::generate::{GenerateToolRequest, ToolAuthor, generate_with, sanitize_name};
use super::sandbox;
use super::service::{ToolError, ToolService};
use crate::storage::Storage;
use sqlx::sqlite::SqlitePoolOptions;

async fn mem_storage() -> Storage {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let storage = Storage::from_pool(pool);
    storage.migrate().await.unwrap();
    storage
}

fn skip_without_python() -> bool {
    sandbox::python3_path().is_none()
}

fn request(name: &str, code: &str) -> CreateToolRequest {
    CreateToolRequest {
        name: name.into(),
        description: name.into(),
        language: "python".into(),
        input_schema: json!({}),
        output_schema: json!({}),
        requirements: vec![],
        code: code.into(),
        permissions: ToolPermissions::locked_down(),
        test_input: Some(json!({})),
        tests: vec![],
        runtime: None,
        purpose: String::new(),
        tags: vec![],
    }
}

async fn run(source: &str, input: Value, perms: ToolPermissions) -> sandbox::SandboxResult {
    sandbox::execute_python(source, input, &perms, &ToolRuntimeConfig::default()).await
}

#[tokio::test]
async fn statistics_tool_create_and_run() {
    if skip_without_python() {
        return;
    }
    let svc = ToolService::new(mem_storage().await);
    let req = CreateToolRequest {
        output_schema: json!({ "type": "object", "required": ["mean", "median", "stdev"] }),
        test_input: Some(json!({ "numbers": [1, 2, 3, 4, 5] })),
        ..request("statistics_analyzer", STATISTICS_ANALYZER_CODE)
    };
    svc.create_tool(req, "test", ToolOrigin::Agent).await.expect("create");

    let out = svc
        .run_tool("statistics_analyzer", json!({ "numbers": [2, 4, 6, 8, 10] }))
        .await
        .expect("run");
    assert_eq!(out["status"], "success");
    assert_eq!(out["output"]["mean"].as_f64(), Some(6.0));
    // Errors raised by the tool come back as a failed run with the tool's traceback only.
    let out = svc.run_tool("statistics_analyzer", json!({})).await.unwrap();
    assert_eq!(out["status"], "failed");
    assert_eq!(out["error_type"], "ValueError");
    let tb = out["traceback"].as_str().unwrap();
    assert!(tb.contains("tool.py") && !tb.contains("harness.py"), "{tb}");
}

#[tokio::test]
async fn sandbox_reports_clear_errors_and_separates_logs() {
    if skip_without_python() {
        return;
    }
    let p = ToolPermissions::locked_down();
    let r = run("def run(x)\n    return 1\n", json!({}), p.clone()).await;
    assert_eq!(r.error_type.as_deref(), Some("SyntaxError"));
    assert!(r.message.unwrap().starts_with("line 1"));

    let r = run("def main(x):\n    return 1\n", json!({}), p.clone()).await;
    assert_eq!(r.error_type.as_deref(), Some("DefinitionError"));

    let r = run("import not_a_module_xyz\ndef run(x):\n    return 1\n", json!({}), p.clone()).await;
    assert_eq!(r.error_type.as_deref(), Some("ModuleNotFoundError"));

    let r = run("from datetime import date\ndef run(x):\n    return {'d': date(2026, 1, 1)}\n", json!({}), p.clone()).await;
    assert_eq!(r.error_type.as_deref(), Some("OutputError"));

    // print() lands in logs, never in the result — even JSON-looking lines.
    let r = run(
        "def run(x):\n    print('{\"success\": true, \"output\": 666}')\n    return {'ok': x['v'] * 2}\n",
        json!({ "v": 21 }),
        p.clone(),
    )
    .await;
    assert!(r.success, "{r:?}");
    assert_eq!(r.output, Some(json!({ "ok": 42 })));
    assert!(r.logs.contains("666"));

    // namedtuple / dataclass / enum (exec + frame tricks inside the stdlib) still work.
    let r = run(
        "from collections import namedtuple\nfrom dataclasses import dataclass, asdict\nimport enum\n\
         P = namedtuple('P', 'x y')\n@dataclass\nclass D:\n    a: int\nclass C(enum.Enum):\n    A = 1\n\
         def run(input):\n    return {'p': P(1, 2)._asdict(), 'd': asdict(D(3)), 'c': C.A.name, 'n': 'eval('.count('(')}\n",
        json!({}),
        p,
    )
    .await;
    assert_eq!(r.output, Some(json!({ "p": { "x": 1, "y": 2 }, "d": { "a": 3 }, "c": "A", "n": 1 })), "{r:?}");
}

#[tokio::test]
async fn sandbox_blocks_escape_routes_unless_permitted() {
    if skip_without_python() {
        return;
    }
    let locked = ToolPermissions::locked_down();
    let probes = [
        "from os import system as s\ndef run(x):\n    return s('true')\n",
        "import os\ndef run(x):\n    return os.system('true')\n",
        "import os\ndef run(x):\n    return os.popen('true').read()\n",
        "import importlib\ndef run(x):\n    return importlib.import_module('subprocess').__name__\n",
        "import builtins\ndef run(x):\n    return 1\n",
        "def run(x):\n    return str(getattr((), '__cla' + 'ss__'))\n",
        "def run(x):\n    return str(().__class__.__bases__[0].__subclasses__())\n",
        "import os\ndef run(x):\n    return os.listdir(os.path.expanduser('~'))\n",
        "def run(x):\n    return open('../../../../../../../etc/hosts').read()\n",
        "def run(x):\n    open('/tmp/boarddo_tool_escape.txt', 'w').write('x')\n    return 1\n",
        "import socket\ndef run(x):\n    return 1\n",
        "import sys\ndef run(x):\n    return str(sys.modules['os'])\n",
        "def run(x):\n    return eval('1+1')\n",
        "from numpy import ctypeslib\ndef run(x):\n    return 1\n",
        "import json\ndef run(x):\n    return str(getattr(json, 'ctypes' + 'lib'))\n",
        "def run(x):\n    return str(vars(x))\n",
    ];
    for src in probes {
        let r = run(src, json!({}), locked.clone()).await;
        assert!(!r.success, "not blocked:\n{src}\n{r:?}");
        assert!(
            matches!(r.error_type.as_deref(), Some("SecurityError" | "PermissionError")),
            "{src}\n{r:?}"
        );
    }
    assert!(!std::path::Path::new("/tmp/boarddo_tool_escape.txt").exists());

    // Own working directory is writable; permissions open the rest.
    let r = run("def run(x):\n    open('a.txt', 'w').write('ok')\n    return open('a.txt').read()\n", json!({}), locked).await;
    assert_eq!(r.output, Some(json!("ok")), "{r:?}");
    let read = ToolPermissions { filesystem_read: true, ..Default::default() };
    let r = run("import os\ndef run(x):\n    return len(os.listdir('/')) > 0\n", json!({}), read).await;
    assert_eq!(r.output, Some(json!(true)), "{r:?}");
}

#[tokio::test]
async fn sandbox_enforces_time_and_memory_and_cleans_up() {
    if skip_without_python() {
        return;
    }
    let p = ToolPermissions::locked_down();
    let quick = ToolRuntimeConfig { timeout_secs: 2, ..Default::default() };
    let started = std::time::Instant::now();
    let r = sandbox::execute_python("def run(x):\n    while True:\n        pass\n", json!({}), &p, &quick).await;
    assert_eq!(r.error_type.as_deref(), Some("TimeoutError"));
    assert!(started.elapsed().as_secs() < 6);

    let small = ToolRuntimeConfig { memory_mb: 64, ..Default::default() };
    let r = sandbox::execute_python(
        "import time\ndef run(x):\n    blob = b'x' * (300 * 1024 * 1024)\n    time.sleep(2)\n    return len(blob)\n",
        json!({}),
        &p,
        &small,
    )
    .await;
    assert!(!r.success, "{r:?}");
    assert_eq!(r.error_type.as_deref(), Some("MemoryError"), "{r:?}");

    let leftovers = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("boarddo-tool-"))
        .filter(|e| e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().map(|d| d.as_secs() < 20).unwrap_or(false)))
        .count();
    // Other tests may be mid-run; this one's directories must be gone.
    assert!(leftovers <= 4, "{leftovers} temp dirs left");
}

#[tokio::test]
async fn test_cases_and_schema_gate_activation() {
    if skip_without_python() {
        return;
    }
    let svc = ToolService::new(mem_storage().await);
    let code = "def run(x):\n    return {'sum': sum(x['n']), 'count': len(x['n'])}\n";
    let mut req = request("summer", code);
    req.output_schema = json!({ "type": "object", "required": ["sum"], "properties": { "sum": { "type": "integer" } } });
    req.tests = vec![
        ToolTestCase { input: json!({ "n": [1, 2, 3] }), expected_schema: Value::Null, expected: Some(json!({ "sum": 6 })), source: None },
        ToolTestCase { input: json!({ "n": [] }), expected_schema: Value::Null, expected: Some(json!({ "sum": 0, "count": 0 })), source: None },
    ];
    req.test_input = None;
    svc.create_tool(req.clone(), "test", ToolOrigin::User).await.expect("passes");

    // A wrong expectation blocks activation and says why.
    let mut bad = req.clone();
    bad.name = "summer_bad".into();
    bad.tests[0].expected = Some(json!({ "sum": 7 }));
    match svc.create_tool(bad, "test", ToolOrigin::User).await {
        Err(ToolError::BuildFailed { message, details }) => {
            assert!(message.contains("output.sum = 6, expected 7"), "{message}");
            assert_eq!(details["activated"], false);
        }
        other => panic!("{other:?}"),
    }
    // Schema type mismatch.
    let mut bad = req.clone();
    bad.name = "summer_type".into();
    bad.code = "def run(x):\n    return {'sum': str(sum(x['n'])), 'count': 0}\n".into();
    bad.tests.clear();
    bad.test_input = Some(json!({ "n": [1] }));
    let err = svc.create_tool(bad, "test", ToolOrigin::User).await.unwrap_err();
    assert!(err.message().contains("output.sum should be integer"), "{err}");

    // Name rules and duplicates.
    let err = svc.create_tool(request("Bad Name;", code), "test", ToolOrigin::User).await.unwrap_err();
    assert!(matches!(err, ToolError::Invalid(_)), "{err:?}");
    let err = svc.create_tool(request("summer", code), "test", ToolOrigin::User).await.unwrap_err();
    assert!(matches!(err, ToolError::Conflict(_)), "{err:?}");

    // A broken update never replaces the working version.
    let mut broken = req.clone();
    broken.code = "def run(x):\n    return {'sum': 1/0}\n".into();
    let res = svc.update_tool("summer", broken, "test").await.unwrap();
    assert_eq!(res["rolled_back"], true);
    let out = svc.run_tool("summer", json!({ "n": [5, 5] })).await.unwrap();
    assert_eq!(out["output"]["sum"], 10);
}

/// Repairer that returns scripted fixes and records what it was told.
struct ScriptedRepairer {
    fixes: Mutex<Vec<Repair>>,
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl ToolRepairer for ScriptedRepairer {
    async fn repair(
        &self,
        _req: &CreateToolRequest,
        _code: &str,
        _cases: &[ToolTestCase],
        failure: &TestFailure,
        _attempt: u32,
    ) -> Result<Option<Repair>, String> {
        self.seen.lock().unwrap().push(failure.reason.clone());
        let mut fixes = self.fixes.lock().unwrap();
        Ok((!fixes.is_empty()).then(|| fixes.remove(0)))
    }
}

#[tokio::test]
async fn repair_loop_fixes_from_failures() {
    if skip_without_python() {
        return;
    }
    // Deterministic fix: a stdlib module used without import.
    let mut req = request("auto_import", "def run(x):\n    return {'r': math.sqrt(x['v'])}\n");
    req.test_input = Some(json!({ "v": 16 }));
    let outcome = builder::build_and_test_version(req.code.clone(), &req).await.unwrap();
    assert!(outcome.activated);
    assert_eq!(outcome.repair_history[0].fixed_by.as_deref(), Some("auto"));

    // Repairer gets the reason (wrong value), then a syntax error, and fixes both.
    let mut req = request("avg", "def run(x):\n    return {'avg': sum(x['n']) // len(x['n'])}\n");
    req.test_input = None;
    req.tests = vec![ToolTestCase { input: json!({ "n": [1, 2] }), expected_schema: Value::Null, expected: Some(json!({ "avg": 1.5 })), source: None }];
    let repairer = ScriptedRepairer {
        fixes: Mutex::new(vec![
            Repair::Code("def run(x)\n    return 0\n".into()),
            Repair::Code("def run(x):\n    return {'avg': sum(x['n']) / len(x['n'])}\n".into()),
        ]),
        seen: Mutex::new(vec![]),
    };
    let outcome = builder::build_with_repairer(req.code.clone(), &req, Some(&repairer)).await.unwrap();
    assert!(outcome.activated);
    assert_eq!(outcome.repair_history.len(), 2);
    let seen = repairer.seen.lock().unwrap().clone();
    assert!(seen[0].contains("output.avg = 1, expected 1.5"), "{seen:?}");
    assert!(seen[1].contains("SyntaxError"), "{seen:?}");

    // A generated test with a wrong expectation may be dropped; a user example may not.
    let code = "def run(x):\n    return {'double': x['v'] * 2}\n";
    let mut req = request("double", code);
    req.test_input = None;
    let case = |v: i64, expect: i64, source: &str| ToolTestCase {
        input: json!({ "v": v }),
        expected_schema: Value::Null,
        expected: Some(json!({ "double": expect })),
        source: Some(source.into()),
    };
    req.tests = vec![case(2, 4, "user"), case(3, 7, "ai")];
    let drop = ScriptedRepairer {
        fixes: Mutex::new(vec![Repair::DropTest { index: 1, reason: "3 * 2 is 6".into() }]),
        seen: Mutex::new(vec![]),
    };
    let outcome = builder::build_with_repairer(code.into(), &req, Some(&drop)).await.unwrap();
    assert!(outcome.activated);
    assert_eq!(outcome.test_cases.len(), 1);
    assert!(outcome.repair_history[0].error_message.contains("dropped generated test #2"));

    req.tests = vec![case(2, 5, "user"), case(3, 6, "ai")];
    let refuse = ScriptedRepairer {
        fixes: Mutex::new(vec![Repair::DropTest { index: 0, reason: "nope".into() }]),
        seen: Mutex::new(vec![]),
    };
    let outcome = builder::build_with_repairer(code.into(), &req, Some(&refuse)).await.unwrap();
    assert!(!outcome.activated);
    assert!(outcome.failure.unwrap().contains("refused to drop test #1"));
}

struct MockAuthor {
    replies: Mutex<Vec<String>>,
    prompts: Mutex<Vec<String>>,
}

#[async_trait]
impl ToolAuthor for MockAuthor {
    async fn complete(&self, _system: &str, user: &str) -> Result<String, String> {
        self.prompts.lock().unwrap().push(user.to_string());
        let mut r = self.replies.lock().unwrap();
        if r.is_empty() { Err("no more replies".into()) } else { Ok(r.remove(0)) }
    }
    fn label(&self) -> String {
        "mock".into()
    }
}

#[tokio::test]
async fn generates_tool_from_description_and_repairs_it() {
    if skip_without_python() {
        return;
    }
    let svc = ToolService::new(mem_storage().await);
    let draft = r#"Here you go.
```json
{"name": "Word Counter", "description": "Считает слова", "input_schema": {"type": "object"},
 "output_schema": {"type": "object", "required": ["words"]},
 "tests": [{"input": {"text": "a b c"}, "expected": {"words": 3}}]}
```
```python
def run(input_data):
    return {"words": len(input_data["text"].split(" ")) + 1}
```"#;
    let fixed = "```python\ndef run(input_data):\n    text = input_data.get('text')\n    if not isinstance(text, str):\n        raise ValueError('text is required')\n    return {'words': len(text.split())}\n```";
    let author = MockAuthor { replies: Mutex::new(vec![draft.into(), fixed.into()]), prompts: Mutex::new(vec![]) };
    let req = GenerateToolRequest {
        description: "Посчитать количество слов в тексте".into(),
        examples: vec![ToolTestCase { input: json!({ "text": "  раз   два  " }), expected_schema: Value::Null, expected: Some(json!({ "words": 2 })), source: None }],
        ..Default::default()
    };
    let created = generate_with(&svc, &author, req).await.expect("generated");
    assert_eq!(created["tool"]["name"], "word_counter");
    assert_eq!(created["tool"]["description"], "Считает слова");
    assert_eq!(created["version"]["repair_history"].as_array().unwrap().len(), 1);
    assert_eq!(created["version"]["repair_history"][0]["fixed_by"], "ai");
    // The repair prompt carried the user's example failure.
    let prompts = author.prompts.lock().unwrap().clone();
    assert!(prompts[1].contains("expected 2"), "{}", prompts[1]);
    let out = svc.run_tool("word_counter", json!({ "text": "one two three four" })).await.unwrap();
    assert_eq!(out["output"]["words"], 4);

    // Same description again → a free name, not a conflict.
    let spec = "```json\n{\"name\": \"word counter\", \"tests\": [{\"input\": {\"text\": \"a b\"}, \"expected\": {\"words\": 2}}]}\n```\n```python";
    let author = MockAuthor { replies: Mutex::new(vec![fixed.replace("```python", spec)]), prompts: Mutex::new(vec![]) };
    let again = generate_with(&svc, &author, GenerateToolRequest { description: "count words again please".into(), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(again["tool"]["name"], "word_counter_2");

    // A model that never gets it right → build failure with the report and the last code.
    let author = MockAuthor {
        replies: Mutex::new(vec!["```python\ndef run(x):\n    return 1/0\n```".into(); 5]),
        prompts: Mutex::new(vec![]),
    };
    let err = generate_with(&svc, &author, GenerateToolRequest { description: "something that fails".into(), ..Default::default() })
        .await
        .unwrap_err();
    match err {
        ToolError::BuildFailed { details, .. } => assert!(details["code"].as_str().unwrap().contains("1/0")),
        other => panic!("{other:?}"),
    }
    assert_eq!(sanitize_name("  Анализ CSV-файлов 2.0 "), "csv_2_0");
    assert_eq!(sanitize_name("3d render"), "tool_3d_render");
}

#[tokio::test]
async fn api_returns_proper_status_codes() {
    if skip_without_python() {
        return;
    }
    let api = crate::objectives::tests::serve_api(crate::objectives::tests::test_state().await).await;
    let client = reqwest::Client::new();
    let post = |path: &str, body: Value| {
        let client = client.clone();
        let url = format!("{api}{path}");
        async move { client.post(url).json(&body).send().await.unwrap() }
    };
    let ok = post("/tools", json!({ "name": "api_tool", "description": "d", "code": "def run(x):\n    return {'v': x.get('v', 0) + 1}\n", "test_input": {} })).await;
    assert_eq!(ok.status(), 200);
    let dup = post("/tools", json!({ "name": "api_tool", "description": "d", "code": "def run(x):\n    return 1\n" })).await;
    assert_eq!(dup.status(), 409);
    let bad = post("/tools", json!({ "name": "syntax_err", "description": "d", "code": "def run(x)\n  return 1\n" })).await;
    assert_eq!(bad.status(), 422);
    let body: Value = bad.json().await.unwrap();
    assert_eq!(body["kind"], "build_failed");
    assert!(body["error"].as_str().unwrap().contains("SyntaxError"), "{body}");
    let missing = post("/tools/nope/run", json!({ "input": {} })).await;
    assert_eq!(missing.status(), 404);

    // Large inputs fit (default axum limit would reject > 2 MB).
    let rows: Vec<Value> = (0..60_000).map(|i| json!({ "id": i, "label": "строка" })).collect();
    let big = post("/tools/api_tool/run", json!({ "input": { "v": 1, "rows": rows } })).await;
    assert_eq!(big.status(), 200);
    let body: Value = big.json().await.unwrap();
    assert_eq!(body["output"]["v"], 2, "{body}");

    // New version via PUT; a failing one is reported and rolled back.
    let res = client
        .put(format!("{api}/tools/api_tool"))
        .json(&json!({ "name": "ignored", "description": "d2", "code": "def run(x):\n    return {'v': 100}\n", "test_input": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let out: Value = post("/tools/api_tool/run", json!({ "input": {} })).await.json().await.unwrap();
    assert_eq!(out["output"]["v"], 100);
    assert_eq!(out["version"], 2);
}
