use boarddo_shared::v2::{CreateToolRequest, ToolPermissions};
use serde_json::json;

use super::builder::{self, STATISTICS_ANALYZER_CODE};
use super::sandbox;
use super::service::ToolService;
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

#[tokio::test]
async fn statistics_tool_create_and_run() {
    if skip_without_python() {
        return;
    }
    let storage = mem_storage().await;
    let svc = ToolService::new(storage);
    let req = CreateToolRequest {
        name: "statistics_analyzer".into(),
        description: "Mean, median, stdev".into(),
        language: "python".into(),
        input_schema: json!({}),
        output_schema: json!({
            "type": "object",
            "required": ["mean", "median", "stdev"]
        }),
        requirements: vec![],
        code: STATISTICS_ANALYZER_CODE.into(),
        permissions: ToolPermissions::locked_down(),
        test_input: Some(json!({ "numbers": [1, 2, 3, 4, 5] })),
        purpose: "acceptance test".into(),
        tags: vec![],
    };
    svc.create_tool(req, "test", boarddo_shared::v2::ToolOrigin::Agent)
        .await
        .expect("create");

    let out = svc
        .run_tool(
            "statistics_analyzer",
            json!({ "numbers": [2, 4, 6, 8, 10] }),
        )
        .await
        .expect("run");
    assert_eq!(out.get("status").and_then(|v| v.as_str()), Some("success"));
    let output = out.get("output").unwrap();
    assert_eq!(output.get("mean").and_then(|v| v.as_f64()), Some(6.0));
}

#[tokio::test]
async fn repair_loop_promotes_v2() {
    if skip_without_python() {
        return;
    }
    let storage = mem_storage().await;
    let svc = ToolService::new(storage);

    let broken = r#"
def run(input_data):
    BUG_INTENTIONAL
    numbers = input_data["numbers"]
    return {"mean": sum(numbers)/len(numbers), "median": 0, "stdev": 0}
"#;

    let req = CreateToolRequest {
        name: "stats_repair".into(),
        description: "repair test".into(),
        language: "python".into(),
        input_schema: json!({}),
        output_schema: json!({
            "type": "object",
            "required": ["mean", "median", "stdev"]
        }),
        requirements: vec![],
        code: broken.into(),
        permissions: ToolPermissions::locked_down(),
        test_input: Some(json!({ "numbers": [1, 2, 3] })),
        purpose: "repair".into(),
        tags: vec![],
    };

    let outcome = builder::build_and_test_version(broken.into(), &req)
        .await
        .expect("build");
    assert!(outcome.activated);
    assert!(!outcome.repair_history.is_empty());

    svc.create_tool(
        CreateToolRequest {
            code: outcome.code,
            ..req
        },
        "test",
        boarddo_shared::v2::ToolOrigin::Agent,
    )
    .await
    .expect("persist");

    let run = svc
        .run_tool("stats_repair", json!({ "numbers": [1, 2, 3, 4] }))
        .await
        .expect("run");
    assert_eq!(run.get("status").and_then(|v| v.as_str()), Some("success"));
}
