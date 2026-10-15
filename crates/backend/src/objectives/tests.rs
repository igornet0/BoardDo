//! End-to-end: REST API + scenarios + engine against a mock Claude Messages API.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use boarddo_shared::v2::{Objective, ObjectiveItemStatus, ObjectiveStatus};
use boarddo_telegram::{TelegramEngine, TelegramSessionStore};
use serde_json::{Value, json};
use sqlx::sqlite::SqlitePoolOptions;

use crate::connections::StorageConnectionProvider;
use crate::secrets::{EncryptedSqliteSecretStore, SecretStore};
use crate::state::{AppState, SharedState};
use crate::storage::Storage;

pub(crate) async fn test_state() -> SharedState {
    let dir = std::env::temp_dir().join(format!("boarddo-objectives-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite:{}?mode=rwc", dir.join("test.db").display());
    let pool = SqlitePoolOptions::new().connect(&url).await.unwrap();
    let storage = Storage::from_pool(pool.clone());
    storage.migrate().await.unwrap();
    let secrets = Arc::new(EncryptedSqliteSecretStore::new(pool.clone(), [7u8; 32]));
    secrets.migrate().await.unwrap();
    let secrets_dyn: Arc<dyn SecretStore> = secrets;
    let connections = Arc::new(StorageConnectionProvider::new(storage.clone(), secrets_dyn.clone()));
    let telegram_store = Arc::new(TelegramSessionStore::new(pool));
    telegram_store.migrate().await.unwrap();
    let data_dir = PathBuf::from(&dir);
    let (telegram, _) = TelegramEngine::with_mock(telegram_store, data_dir.clone());
    Arc::new(AppState::new(storage, secrets_dyn, connections, Arc::new(telegram), data_dir))
}

#[derive(Default)]
struct Seen {
    /// (system, user, raw body) per call.
    calls: Vec<(String, String, Value)>,
    headers_ok: bool,
}

/// Claude `/v1/messages` mock that answers by role of the request.
async fn mock_claude() -> (String, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen {
        headers_ok: true,
        ..Default::default()
    }));
    let log = seen.clone();
    let app = axum::Router::new().route(
        "/v1/messages",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                let log = log.clone();
                async move {
                    let system = body["system"].as_str().unwrap_or_default().to_string();
                    let user = body["messages"][0]["content"].as_str().unwrap_or_default().to_string();
                    let reply = reply_for(&system, &user);
                    {
                        let mut s = log.lock().unwrap();
                        let ok = headers.get("x-api-key").is_some_and(|v| v == "sk-test")
                            && headers.get("anthropic-version").is_some_and(|v| v == "2023-06-01");
                        s.headers_ok &= ok;
                        s.calls.push((system, user, body.clone()));
                    }
                    axum::Json(json!({
                        "id": "msg_test",
                        "type": "message",
                        "role": "assistant",
                        "model": body["model"],
                        "stop_reason": "end_turn",
                        "content": [
                            { "type": "thinking", "thinking": "" },
                            { "type": "text", "text": reply }
                        ],
                        "usage": { "input_tokens": 100, "output_tokens": 20 }
                    }))
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/v1"), seen)
}

fn reply_for(system: &str, user: &str) -> String {
    if system.contains("objective planner") && user.contains("## Evaluation") {
        return json!({
            "message": "Закрываю пробел",
            "items": [{ "key": "g", "title": "Закрыть пробел GAP-CLOSED", "kind": "ai" }]
        })
        .to_string();
    }
    if system.contains("review whether an objective was achieved")
        && user.contains("[refine-test]")
        && !user.contains("GAP-CLOSED")
    {
        return json!({ "achieved": false, "summary": "Не хватает GAP", "next_steps": [] }).to_string();
    }
    if system.contains("objective planner") {
        if user.contains("Target item to split") {
            return json!({
                "message": "Разбил на два шага",
                "items": [
                    { "key": "x", "title": "Собрать 10 вариантов", "kind": "ai" },
                    { "key": "y", "title": "Отсеять занятые", "kind": "ai", "depends_on": ["x"] }
                ]
            })
            .to_string();
        }
        return json!({
            "message": "Цель заполнена",
            "description": "Подобрать короткое запоминающееся название для кофейни у университета.",
            "characteristics": [
                { "key": "Город", "value": "Москва" },
                { "key": "Длина названия", "value": "до 12", "unit": "символов" }
            ],
            "success_criteria": ["Есть финальное название", "Есть 3 запасных варианта"],
            "items": [
                { "key": "a", "title": "Придумать варианты", "kind": "ai", "prompt": "Дай список" },
                { "key": "b", "title": "Выбрать лучший", "kind": "ai", "depends_on": ["a"] }
            ]
        })
        .to_string();
    }
    if system.contains("review whether an objective was achieved") {
        return json!({ "achieved": true, "summary": "Название выбрано: «Зерно»", "next_steps": ["Проверить домен"] })
            .to_string();
    }
    // Item executor: echo the task line so tests can follow the context chain.
    let task = user
        .split("## Текущий пункт")
        .nth(1)
        .and_then(|s| s.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or("?")
        .trim()
        .to_string();
    format!("RESULT[{task}]")
}

pub(crate) async fn serve_api(state: SharedState) -> String {
    let app = crate::api::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/api")
}

pub(crate) async fn call(client: &reqwest::Client, method: reqwest::Method, url: String, body: Value) -> Value {
    let resp = client.request(method, &url).json(&body).send().await.unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap();
    assert!(status.is_success(), "{url} → {status}: {text}");
    if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap() }
}

pub(crate) async fn wait_status(client: &reqwest::Client, api: &str, id: &str, want: &[ObjectiveStatus]) -> Objective {
    for _ in 0..200 {
        let v = call(client, reqwest::Method::GET, format!("{api}/objectives/{id}"), Value::Null).await;
        let o: Objective = serde_json::from_value(v).unwrap();
        if want.contains(&o.status) {
            return o;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("objective never reached {want:?}");
}

async fn setup() -> (SharedState, reqwest::Client, String, String, Arc<Mutex<Seen>>) {
    let state = test_state().await;
    let (claude_url, seen) = mock_claude().await;
    let api = serve_api(state.clone()).await;
    let client = reqwest::Client::new();
    let conn = call(
        &client,
        reqwest::Method::POST,
        format!("{api}/connections"),
        json!({
            "name": "Claude",
            "type": "anthropic",
            "config": { "provider": "anthropic", "base_url": claude_url, "default_model": "claude-opus-5-5" },
            "secret": { "api_key": "sk-test" },
        }),
    )
    .await;
    let conn_id = conn["id"].as_str().unwrap().to_string();
    (state, client, api, conn_id, seen)
}

#[tokio::test]
async fn small_objective_is_filled_split_executed_and_evaluated_with_claude() {
    let (state, client, api, conn_id, seen) = setup().await;
    use reqwest::Method;

    // Connection test pings Claude through the same client.
    let test = call(&client, Method::POST, format!("{api}/connections/{conn_id}/test"), json!({})).await;
    assert_eq!(test["ok"], true, "{test}");

    let created = call(
        &client,
        Method::POST,
        format!("{api}/objectives"),
        json!({
            "title": "Название для кофейни",
            "characteristics": [{ "key": "Город", "value": "Казань" }],
        }),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["status"], "draft");
    assert_eq!(created["connection_id"], conn_id.as_str(), "Claude picked as default");

    // 1. AI fills the objective.
    let filled = call(
        &client,
        Method::POST,
        format!("{api}/objectives/{id}/assist"),
        json!({ "mode": "fill", "apply": true }),
    )
    .await;
    assert_eq!(filled["applied"], true);
    let o: Objective = serde_json::from_value(filled["objective"].clone()).unwrap();
    assert_eq!(o.status, ObjectiveStatus::Ready);
    assert_eq!(o.items.len(), 2);
    assert_eq!(o.success_criteria.len(), 2);
    let city = o.characteristics.iter().find(|c| c.key == "Город").unwrap();
    assert_eq!(city.value, "Казань", "user value kept over AI suggestion");
    assert!(o.characteristics.iter().any(|c| c.key == "Длина названия"));

    // 2. AI splits the first item into sub-items.
    let split = call(
        &client,
        Method::POST,
        format!("{api}/objectives/{id}/assist"),
        json!({ "mode": "decompose", "item_id": "i1", "apply": true }),
    )
    .await;
    let o: Objective = serde_json::from_value(split["objective"].clone()).unwrap();
    assert_eq!(o.ordered_ids(), vec!["i1", "i1.1", "i1.2", "i2"]);
    assert_eq!(o.item("i1.2").unwrap().depends_on, vec!["i1.1"]);
    assert_eq!(o.progress.total, 3);

    // 3. Run: every leaf goes through its own scenario.
    call(&client, Method::POST, format!("{api}/objectives/{id}/run"), json!({})).await;
    let done = wait_status(&client, &api, &id, &[ObjectiveStatus::Done, ObjectiveStatus::Failed]).await;
    assert_eq!(done.status, ObjectiveStatus::Done, "{:#?}", done.log);
    assert_eq!(done.progress.percent, 100.0);
    assert_eq!(done.achieved, Some(true));
    assert!(done.summary.as_deref().unwrap().contains("Зерно"));
    assert!(done.summary.as_deref().unwrap().contains("Проверить домен"));
    assert_eq!(done.item("i1").unwrap().status, ObjectiveItemStatus::Done, "parent derived");

    for leaf in ["i1.1", "i1.2", "i2"] {
        let item = done.item(leaf).unwrap();
        assert_eq!(item.status, ObjectiveItemStatus::Done);
        assert!(item.result.as_deref().unwrap().starts_with("RESULT["), "{leaf}: {:?}", item.result);
        let wid = item.workflow_id.expect("scenario generated");
        let wf = state.storage.get_workflow(wid).await.unwrap().unwrap();
        assert!(wf.name.starts_with("🎯 Название для кофейни"));
        let ex = state.storage.get_execution(item.execution_id.unwrap()).await.unwrap().unwrap();
        assert_eq!(ex.status, boarddo_shared::ExecutionStatus::Completed);
        assert_eq!(ex.workflow_id, wid);
    }
    assert_eq!(done.item("i1.1").unwrap().result.as_deref(), Some("RESULT[1.1 Собрать 10 вариантов (⊂ Придумать варианты)]"));

    let s = seen.lock().unwrap();
    assert!(s.headers_ok, "x-api-key / anthropic-version sent on every call");
    let exec_calls: Vec<&(String, String, Value)> =
        s.calls.iter().filter(|(sys, _, _)| sys.contains("исполнитель пунктов")).collect();
    assert_eq!(exec_calls.len(), 3);
    // Later items see earlier results (context chain).
    assert!(exec_calls[2].1.contains("RESULT[1.2 Отсеять занятые"));
    assert!(exec_calls[2].1.contains("Город: Казань"));
    // Opus 5.5 request shape: no sampling params, model from the connection.
    let body = &exec_calls[0].2;
    assert_eq!(body["model"], "claude-opus-5-5");
    assert!(body.get("temperature").is_none());
    assert!(body["max_tokens"].as_u64().unwrap() >= 4096);
}

#[tokio::test]
async fn manual_item_pauses_run_and_marking_it_done_resumes() {
    let (_state, client, api, _conn, _seen) = setup().await;
    use reqwest::Method;

    let created = call(
        &client,
        Method::POST,
        format!("{api}/objectives"),
        json!({
            "title": "Short launch note",
            "items": [
                { "id": "i1", "title": "Draft the note", "kind": "ai" },
                { "id": "i2", "title": "Approve with the team", "kind": "manual", "depends_on": ["i1"] },
                { "id": "i3", "title": "Polish", "kind": "ai", "depends_on": ["i2"] }
            ]
        }),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["status"], "ready");

    call(&client, Method::POST, format!("{api}/objectives/{id}/run"), json!({})).await;
    let paused = wait_status(&client, &api, &id, &[ObjectiveStatus::Paused, ObjectiveStatus::Failed]).await;
    assert_eq!(paused.status, ObjectiveStatus::Paused, "{:#?}", paused.log);
    assert_eq!(paused.item("i1").unwrap().status, ObjectiveItemStatus::Done);
    assert_eq!(paused.item("i3").unwrap().status, ObjectiveItemStatus::Pending);

    call(
        &client,
        Method::PATCH,
        format!("{api}/objectives/{id}/items/i2"),
        json!({ "status": "done", "result": "Team approved" }),
    )
    .await;
    let done = wait_status(&client, &api, &id, &[ObjectiveStatus::Done, ObjectiveStatus::Failed]).await;
    assert_eq!(done.status, ObjectiveStatus::Done, "{:#?}", done.log);
    assert_eq!(done.item("i3").unwrap().status, ObjectiveItemStatus::Done);
    assert_eq!(done.item("i2").unwrap().result.as_deref(), Some("Team approved"));
}

#[tokio::test]
async fn failed_item_stops_run_and_retry_recovers() {
    let (state, client, api, _conn, _seen) = setup().await;
    use reqwest::Method;

    // Scenario-kind item pointing at a workflow that fails validation-free but errors at runtime.
    let wf = state
        .storage
        .create_workflow(boarddo_shared::CreateWorkflowRequest {
            name: "broken".into(),
            description: String::new(),
            nodes: vec![
                boarddo_shared::Node {
                    id: "trigger".into(),
                    type_id: "trigger.manual".into(),
                    category: None,
                    position: Default::default(),
                    config: json!({}),
                },
                boarddo_shared::Node {
                    id: "result".into(),
                    type_id: "ai.chat".into(),
                    category: None,
                    position: Default::default(),
                    // No connection → node error.
                    config: json!({ "prompt": "{{trigger.task}}" }),
                },
            ],
            edges: vec![boarddo_shared::Edge {
                id: "e1".into(),
                source: "trigger".into(),
                target: "result".into(),
                source_port: None,
                target_port: None,
            }],
        })
        .await
        .unwrap();

    let created = call(
        &client,
        Method::POST,
        format!("{api}/objectives"),
        json!({
            "title": "Broken scenario",
            "items": [{ "id": "i1", "title": "Use custom scenario", "kind": "workflow", "workflow_id": wf.id }]
        }),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    call(&client, Method::POST, format!("{api}/objectives/{id}/run"), json!({})).await;
    let failed = wait_status(&client, &api, &id, &[ObjectiveStatus::Failed, ObjectiveStatus::Done]).await;
    assert_eq!(failed.status, ObjectiveStatus::Failed);
    let item = failed.item("i1").unwrap();
    assert_eq!(item.status, ObjectiveItemStatus::Failed);
    assert!(item.error.as_deref().unwrap().contains("connection_id"), "{:?}", item.error);

    // Switch the item back to a generated scenario and retry.
    call(
        &client,
        Method::PATCH,
        format!("{api}/objectives/{id}/items/i1"),
        json!({ "unlink_workflow": true }),
    )
    .await;
    call(&client, Method::POST, format!("{api}/objectives/{id}/run"), json!({ "retry_failed": true })).await;
    let done = wait_status(&client, &api, &id, &[ObjectiveStatus::Done, ObjectiveStatus::Failed]).await;
    assert_eq!(done.status, ObjectiveStatus::Done, "{:#?}", done.log);
    assert_ne!(done.item("i1").unwrap().workflow_id, Some(wf.id));
}

#[tokio::test]
async fn auto_refine_adds_gap_items_until_achieved() {
    let (_state, client, api, _conn, seen) = setup().await;
    use reqwest::Method;

    let created = call(
        &client,
        Method::POST,
        format!("{api}/objectives"),
        json!({
            "title": "Проверка доработки [refine-test]",
            "auto_refine": 2,
            "items": [{ "id": "i1", "title": "Первый шаг", "kind": "ai" }]
        }),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["auto_refine"], 2);

    call(&client, Method::POST, format!("{api}/objectives/{id}/run"), json!({})).await;
    let done = wait_status(&client, &api, &id, &[ObjectiveStatus::Done, ObjectiveStatus::Failed]).await;
    assert_eq!(done.status, ObjectiveStatus::Done, "{:#?}", done.log);
    assert_eq!(done.achieved, Some(true));
    assert_eq!(done.refine_rounds, 1, "one round was enough");
    assert_eq!(done.ordered_ids(), vec!["i1", "i2"]);
    let added = done.item("i2").unwrap();
    assert_eq!(added.title, "Закрыть пробел GAP-CLOSED");
    assert_eq!(added.status, ObjectiveItemStatus::Done);
    assert!(done.log.iter().any(|l| l.message.contains("доработка, раунд 1/2")));

    // The refine planner saw the failed evaluation and earlier results.
    let s = seen.lock().unwrap();
    let refine_call = s
        .calls
        .iter()
        .find(|(sys, user, _)| sys.contains("objective planner") && user.contains("## Evaluation"))
        .expect("refine planner called");
    assert!(refine_call.1.contains("Не хватает GAP"));
    assert!(refine_call.1.contains("RESULT[1 Первый шаг]"));
}

#[tokio::test]
async fn manual_refine_reopens_a_done_objective() {
    let (_state, client, api, _conn, _seen) = setup().await;
    use reqwest::Method;

    let created = call(
        &client,
        Method::POST,
        format!("{api}/objectives"),
        json!({
            "title": "Ручная доработка [refine-test]",
            "items": [{ "id": "i1", "title": "Шаг", "kind": "ai" }]
        }),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    call(&client, Method::POST, format!("{api}/objectives/{id}/run"), json!({})).await;
    let done = wait_status(&client, &api, &id, &[ObjectiveStatus::Done, ObjectiveStatus::Failed]).await;
    assert_eq!(done.achieved, Some(false), "auto_refine is off by default");

    let refined = call(
        &client,
        Method::POST,
        format!("{api}/objectives/{id}/assist"),
        json!({ "mode": "refine", "apply": true }),
    )
    .await;
    let o: Objective = serde_json::from_value(refined["objective"].clone()).unwrap();
    assert_eq!(o.status, ObjectiveStatus::Ready);
    assert_eq!(o.item("i2").unwrap().status, ObjectiveItemStatus::Pending);

    call(&client, Method::POST, format!("{api}/objectives/{id}/run"), json!({})).await;
    let done = wait_status(&client, &api, &id, &[ObjectiveStatus::Done, ObjectiveStatus::Failed]).await;
    assert_eq!(done.achieved, Some(true), "{:#?}", done.log);
}
