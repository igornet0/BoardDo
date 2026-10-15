//! ProjectHub integration against a mock `project-hub-server`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use boarddo_shared::v2::{ObjectiveItemStatus, ObjectiveStatus};
use serde_json::{Value, json};

use super::{PhClient, set_test_client};
use crate::objectives::tests::{call, serve_api, test_state, wait_status};

const PID: &str = "6f1c1a4e-8a5c-4a4e-9a51-1c2d3e4f5a6b";

/// Serialises tests that swap the global test client.
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Default)]
struct Mock {
    tasks: Vec<Value>,
    patches: Vec<(String, Value)>,
}

async fn mock_projecthub() -> (String, Arc<Mutex<Mock>>) {
    let mock = Arc::new(Mutex::new(Mock::default()));
    let (m1, m2, m3, m4) = (mock.clone(), mock.clone(), mock.clone(), mock.clone());
    let api = Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route(
            "/projects",
            get(|| async { Json(json!({ "total": 1, "items": [{ "id": PID, "name": "demo" }] })) }),
        )
        .route("/projects/{id}", get(|Path(id): Path<String>| async move { Json(json!({ "id": id, "name": "demo" })) }))
        .route(
            "/projects/{id}/brdd",
            get(|| async {
                Json(json!({
                    "exists": true,
                    "summary_md": "# demo\n\n| Stack | Axum |",
                    "notes_md": "Uses SQLite.",
                    "analysis": { "project": { "root_path": "/work/demo" } }
                }))
            }),
        )
        .route(
            "/tasks",
            post(move |Json(body): Json<Value>| {
                let m = m1.clone();
                async move {
                    let task = json!({
                        "id": uuid::Uuid::new_v4().to_string(),
                        "project_id": body["project_id"],
                        "title": body["title"],
                        "description": body["description"],
                        "status": "todo",
                        "link": { "source": body["source"], "external_ref": body["external_ref"] }
                    });
                    m.lock().unwrap().tasks.push(task.clone());
                    (StatusCode::CREATED, Json(task))
                }
            })
            .get(move |Query(q): Query<std::collections::HashMap<String, String>>| {
                let m = m2.clone();
                async move {
                    let tasks = m.lock().unwrap().tasks.clone();
                    let ids: Vec<String> = q
                        .get("ids")
                        .map(|s| s.split(',').map(str::to_string).collect())
                        .unwrap_or_default();
                    let out: Vec<Value> = tasks
                        .into_iter()
                        .filter(|t| ids.is_empty() || ids.iter().any(|i| t["id"] == i.as_str()))
                        .collect();
                    Json(json!(out))
                }
            }),
        )
        .route(
            "/tasks/{id}",
            get(move |Path(id): Path<String>| {
                let m = m3.clone();
                async move {
                    let task = m.lock().unwrap().tasks.iter().find(|t| t["id"] == id.as_str()).cloned();
                    match task {
                        Some(t) => (StatusCode::OK, Json(t)),
                        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "not found", "code": "not_found" }))),
                    }
                }
            })
            .patch(move |Path(id): Path<String>, Json(body): Json<Value>| {
                let m = m4.clone();
                async move {
                    let mut m = m.lock().unwrap();
                    m.patches.push((id.clone(), body.clone()));
                    let task = m.tasks.iter_mut().find(|t| t["id"] == id.as_str()).unwrap();
                    if let Some(s) = body["status"].as_str() {
                        task["status"] = json!(s);
                    }
                    Json(task.clone())
                }
            }),
        );
    let app = Router::new().nest("/api/v1", api);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), mock)
}

fn set_task_status(mock: &Arc<Mutex<Mock>>, title: &str, status: &str) {
    let mut m = mock.lock().unwrap();
    let t = m.tasks.iter_mut().find(|t| t["title"] == title).expect("task exists");
    t["status"] = json!(status);
}

#[tokio::test]
async fn goal_task_items_live_in_projecthub_and_are_tracked() {
    let _guard = LOCK.lock().await;
    let (url, mock) = mock_projecthub().await;
    set_test_client(Some(PhClient::new(&url, None)));

    let state = test_state().await;
    let api = serve_api(state.clone()).await;
    let client = reqwest::Client::new();
    use reqwest::Method;

    let created = call(
        &client,
        Method::POST,
        format!("{api}/objectives"),
        json!({
            "title": "Release demo 0.2",
            "project_hub_project_id": PID,
            "project_hub_project_name": "demo",
            "items": [
                { "id": "i1", "title": "Fix login bug", "kind": "task", "description": "Users get 500 on /login" },
                { "id": "i2", "title": "Tag the release", "kind": "task", "depends_on": ["i1"] }
            ]
        }),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["project_hub_project_id"], PID);

    // Run: i1 becomes a ProjectHub task, the goal waits for it.
    call(&client, Method::POST, format!("{api}/objectives/{id}/run"), json!({})).await;
    let paused = wait_status(&client, &api, &id, &[ObjectiveStatus::Paused, ObjectiveStatus::Failed]).await;
    assert_eq!(paused.status, ObjectiveStatus::Paused, "{:#?}", paused.log);
    let i1 = paused.item("i1").unwrap();
    assert_eq!(i1.status, ObjectiveItemStatus::Running);
    assert_eq!(i1.external_status.as_deref(), Some("todo"));
    assert!(paused.log.iter().any(|l| l.message.starts_with("Waiting for ProjectHub tasks")));
    {
        let m = mock.lock().unwrap();
        assert_eq!(m.tasks.len(), 1);
        let t = &m.tasks[0];
        assert_eq!(t["project_id"], PID);
        assert_eq!(t["link"]["source"], "boarddo");
        assert_eq!(t["link"]["external_ref"], format!("objective:{id}/i1"));
        assert!(t["description"].as_str().unwrap().contains("Users get 500"));
    }

    // In progress there → still running here; done there → item done, goal resumes, i2 created.
    set_task_status(&mock, "Fix login bug", "in_progress");
    assert_eq!(super::tracker::sync(&state).await.unwrap(), 1);
    set_task_status(&mock, "Fix login bug", "done");
    super::tracker::sync(&state).await.unwrap();
    let mut waited = None;
    for _ in 0..100 {
        let o = state.storage.get_objective(id.parse().unwrap()).await.unwrap().unwrap();
        if o.item("i2").is_some_and(|i| i.status == ObjectiveItemStatus::Running) {
            waited = Some(o);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let o = waited.expect("i2 started after i1 was done in ProjectHub");
    assert_eq!(o.item("i1").unwrap().status, ObjectiveItemStatus::Done);
    assert!(o.item("i1").unwrap().result.as_deref().unwrap().contains("Task completed in ProjectHub"));
    assert_eq!(mock.lock().unwrap().tasks.len(), 2);

    // Ticking i2 off in BoardDo closes its ProjectHub task and finishes the goal.
    call(
        &client,
        Method::PATCH,
        format!("{api}/objectives/{id}/items/i2"),
        json!({ "status": "done" }),
    )
    .await;
    let done = wait_status(&client, &api, &id, &[ObjectiveStatus::Done, ObjectiveStatus::Failed]).await;
    assert_eq!(done.status, ObjectiveStatus::Done, "{:#?}", done.log);
    let patches = mock.lock().unwrap().patches.clone();
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].1["status"], "done");
    // The item mirrors the status it pushed (the tracker no longer polls settled items).
    assert_eq!(done.item("i2").unwrap().external_status.as_deref(), Some("done"));

    set_test_client(None);
}

#[tokio::test]
async fn projecthub_nodes_and_project_context() {
    let _guard = LOCK.lock().await;
    let (url, mock) = mock_projecthub().await;
    set_test_client(Some(PhClient::new(&url, None)));

    let state = test_state().await;
    let api = serve_api(state.clone()).await;
    let client = reqwest::Client::new();
    use reqwest::Method;

    // Scenario: create a task by project *name*, then list the project's tasks.
    let wf = call(
        &client,
        Method::POST,
        format!("{api}/workflows"),
        json!({
            "name": "PH nodes",
            "nodes": [
                { "id": "trigger", "type_id": "trigger.manual", "position": { "x": 0, "y": 0 }, "config": {} },
                { "id": "create", "type_id": "projecthub.task.create", "position": { "x": 200, "y": 0 },
                  "config": { "project": "demo", "title": "{{trigger.title}}", "priority": "high" } },
                { "id": "list", "type_id": "projecthub.tasks", "position": { "x": 400, "y": 0 },
                  "config": { "project": "demo", "source": "boarddo" } },
                { "id": "brdd", "type_id": "projecthub.brdd", "position": { "x": 600, "y": 0 },
                  "config": { "project": PID } }
            ],
            "edges": [
                { "id": "e1", "source": "trigger", "target": "create" },
                { "id": "e2", "source": "create", "target": "list" },
                { "id": "e3", "source": "list", "target": "brdd" }
            ]
        }),
    )
    .await;
    let wid = wf["id"].as_str().unwrap();
    let run = call(
        &client,
        Method::POST,
        format!("{api}/workflows/{wid}/run"),
        json!({ "trigger": { "title": "Write README" } }),
    )
    .await;
    let eid = run["execution_id"].as_str().unwrap().to_string();
    let mut status = Value::Null;
    for _ in 0..100 {
        let ex = call(&client, Method::GET, format!("{api}/executions/{eid}"), Value::Null).await;
        status = ex["status"].clone();
        if status == "completed" || status == "failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let nodes = call(&client, Method::GET, format!("{api}/executions/{eid}/nodes"), Value::Null).await;
    assert_eq!(status, "completed", "{nodes:#}");
    let out = |id: &str| {
        nodes
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["node_id"] == id)
            .map(|n| n["output"].clone())
            .unwrap()
    };
    assert_eq!(out("create")["title"], "Write README");
    assert_eq!(out("create")["link"]["external_ref"], format!("workflow:{wid}"));
    assert_eq!(out("list")["count"], 1);
    assert_eq!(out("brdd")["exists"], true);
    assert_eq!(mock.lock().unwrap().tasks[0]["project_id"], PID);

    // Goals linked to a project get its .brdd digest as context.
    let mut o = boarddo_shared::v2::Objective::new("Ctx");
    o.project_hub_project_id = Some(PID.into());
    let ctx = crate::objectives::runner::project_context(&o).await.unwrap();
    assert!(ctx.contains("| Stack | Axum |") && ctx.contains("Uses SQLite.") && ctx.contains("/work/demo"));

    set_test_client(None);
}

#[tokio::test]
async fn extension_is_not_installed_by_default() {
    let _guard = LOCK.lock().await;
    set_test_client(None);
    let state = test_state().await;
    let hub = super::init(state.storage.clone()).await;
    let status = hub.status().await;
    assert_eq!(status["state"], "not_installed");
    assert_eq!(status["installed"], false);
    assert!(hub.enable().await.unwrap_err().contains("install"));
    let err = super::client().await.unwrap_err();
    assert!(err.contains("not installed") || err.contains("not initialised"), "{err}");
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("boarddo-ph-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fake `project-hub-server`: answers `--version` and `--self-test`.
#[cfg(unix)]
fn fake_server(path: &std::path::Path, version: &str, self_test_ok: bool) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo \"project-hub-server {version}\" ;;\n  --self-test) echo '{{\"ok\":{self_test_ok},\"version\":\"{version}\",\"passed\":13,\"failed\":{},\"steps\":[]}}' ;;\n  *) exit 2 ;;\nesac\n",
        if self_test_ok { 0 } else { 1 }
    );
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Lays out an installed desktop app the way this OS ships it; returns (app dir, server path).
#[cfg(unix)]
fn fake_app(root: &std::path::Path, version: &str, with_server: bool) -> (std::path::PathBuf, std::path::PathBuf) {
    if cfg!(target_os = "macos") {
        let app = root.join("Project Hub.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            format!("<plist><dict><key>CFBundleShortVersionString</key>\n<string>{version}</string></dict></plist>"),
        )
        .unwrap();
        let server = app.join("Contents/MacOS/project-hub-server");
        if with_server {
            fake_server(&server, version, true);
        }
        (app, server)
    } else {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("project-hub"), "").unwrap();
        let server = root.join("project-hub-server");
        if with_server {
            fake_server(&server, version, true);
        }
        (root.to_path_buf(), server)
    }
}

#[cfg(unix)]
async fn fake_db(path: &std::path::Path, projects: usize) {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite:{}?mode=rwc", path.display()))
        .await
        .unwrap();
    sqlx::query("CREATE TABLE projects (id TEXT); CREATE TABLE tasks (id TEXT)")
        .execute(&mut conn)
        .await
        .unwrap();
    for i in 0..projects {
        sqlx::query("INSERT INTO projects VALUES (?)").bind(i.to_string()).execute(&mut conn).await.unwrap();
    }
    conn.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn detects_installed_app_path_binary_and_database() {
    use super::detect::{Roots, detect_in};
    let tmp = TempDir::new();
    let (app, server) = fake_app(&tmp.path().join("Applications"), "0.1.1", true);
    let (old_app, _) = fake_app(&tmp.path().join("Old"), "0.1.0", false);
    let bin_dir = tmp.path().join("bin");
    fake_server(&bin_dir.join("project-hub-server"), "0.1.2", true);
    let db = tmp.path().join("project-hub.db");
    fake_db(&db, 3).await;

    let roots = Roots {
        app_dirs: vec![tmp.path().join("Applications"), tmp.path().join("Old")],
        bin_dirs: vec![bin_dir.clone(), tmp.path().join("missing")],
        env_bin: None,
        managed: Some(tmp.path().join("nope/project-hub-server")),
        default_db: db.clone(),
    };
    let d = detect_in(&roots, None).await;
    assert_eq!(d.servers.len(), 2, "{d:?}");
    let best = d.best().unwrap();
    assert_eq!(best.source, "app");
    assert_eq!(best.binary, server);
    assert_eq!(best.version.as_deref(), Some("0.1.1"));
    assert_eq!(d.servers[1].source, "path");
    assert_eq!(d.servers[1].version.as_deref(), Some("0.1.2"));
    assert_eq!(d.apps.len(), 2);
    assert!(d.apps.iter().any(|a| a.path == app && a.has_server));
    assert!(d.apps.iter().any(|a| a.path == old_app && !a.has_server));
    if cfg!(target_os = "macos") {
        assert_eq!(d.apps[0].version.as_deref(), Some("0.1.1"));
    }
    assert!(d.database.exists);
    assert_eq!(d.database.projects, Some(3));
    assert_eq!(d.database.tasks, Some(0));
    assert!(d.find(&bin_dir.join("project-hub-server")).is_some());

    // The same binary reached twice (app + PATH symlink) is listed once.
    std::os::unix::fs::symlink(&server, tmp.path().join("bin2")).ok();
    let link_dir = tmp.path().join("linkdir");
    std::fs::create_dir_all(&link_dir).unwrap();
    std::os::unix::fs::symlink(&server, link_dir.join("project-hub-server")).unwrap();
    let d = detect_in(&Roots { bin_dirs: vec![link_dir], ..roots.clone() }, None).await;
    assert_eq!(d.servers.len(), 1);

    // Nothing installed.
    let d = detect_in(&Roots { default_db: tmp.path().join("none.db"), ..Roots::default() }, None).await;
    assert!(d.best().is_none());
    assert!(!d.database.exists);
}

#[cfg(unix)]
#[tokio::test]
async fn already_installed_projecthub_is_adopted_and_tracked() {
    use super::detect::{Roots, set_test_roots};
    let _guard = LOCK.lock().await;
    set_test_client(None);
    let tmp = TempDir::new();
    let (_app, server) = fake_app(&tmp.path().join("Applications"), "0.1.1", true);
    set_test_roots(Some(Roots {
        app_dirs: vec![tmp.path().join("Applications")],
        default_db: tmp.path().join("project-hub.db"),
        ..Roots::default()
    }));

    let state = test_state().await;
    let hub = super::init(state.storage.clone()).await;
    // Startup reconcile adopts it after the self-test — installed, not enabled.
    let mut status = Value::Null;
    for _ in 0..100 {
        status = hub.status().await;
        if status["installed"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status["installed"], true, "{status}");
    assert_eq!(status["enabled"], false);
    assert_eq!(status["state"], "installed");
    assert_eq!(status["config"]["source"], "app");
    assert_eq!(status["config"]["version"], "0.1.1");
    assert_eq!(status["config"]["binary"], server.display().to_string());
    assert_eq!(status["self_test_ok"], true);

    // The app is updated in place → the version follows on the next check.
    fake_server(&server, "0.2.0", true);
    hub.reconcile().await.unwrap();
    assert_eq!(hub.config().await.version.as_deref(), Some("0.2.0"));

    // Uninstall forgets it but leaves the app alone, and does not re-adopt…
    hub.uninstall().await.unwrap();
    assert!(server.is_file());
    hub.reconcile().await.unwrap();
    let status = hub.status().await;
    assert_eq!(status["installed"], false);
    assert_eq!(status["state"], "detected");
    assert_eq!(status["detected"]["servers"][0]["source"], "app");
    // …until the user asks for it.
    hub.use_installed(None).await.unwrap();
    assert!(hub.config().await.installed);

    // The app was removed → "missing", and nothing to fall back to.
    std::fs::remove_file(&server).unwrap();
    hub.reconcile().await.unwrap();
    assert_eq!(hub.status().await["state"], "missing");
    assert!(hub.use_installed(None).await.unwrap_err().contains("no installed"));

    // A server failing its self-test is never adopted.
    fake_server(&server, "0.3.0", false);
    hub.uninstall().await.unwrap();
    let err = hub.use_installed(None).await.unwrap_err();
    assert!(err.contains("self-test"), "{err}");
    assert!(!hub.config().await.installed);
    // Paths that were not detected are refused.
    let err = hub.use_installed(Some("/bin/sh".into())).await.unwrap_err();
    assert!(err.contains("not a detected"), "{err}");

    set_test_roots(None);
}
