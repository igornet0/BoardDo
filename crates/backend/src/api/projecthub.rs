//! ProjectHub extension API: lifecycle (`/api/extensions/projecthub/*`), a
//! proxy to the sidecar (`/api/projecthub/*` → `/api/v1/*`) and AI actions
//! (`/api/projecthub-ai/*`).

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use boarddo_shared::v2::Objective;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::objectives::{llm, scenario};
use crate::integrations::openai::extract_json_object;
use crate::projecthub::{self, PhClient};
use crate::state::SharedState;

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/extensions", get(list_extensions))
        .route("/extensions/projecthub", get(status))
        .route("/extensions/projecthub/install", post(install))
        .route("/extensions/projecthub/detect", post(detect))
        .route("/extensions/projecthub/use-installed", post(use_installed))
        .route("/extensions/projecthub/self-test", post(self_test))
        .route("/extensions/projecthub/enable", post(enable))
        .route("/extensions/projecthub/disable", post(disable))
        .route("/extensions/projecthub/restart", post(restart))
        .route("/extensions/projecthub/config", put(update_config))
        .route("/extensions/projecthub/uninstall", post(uninstall))
        .route("/projecthub/{*rest}", any(proxy))
        .route("/projecthub-ai/projects/{id}/analyze", post(ai_analyze))
        .route("/projecthub-ai/projects/{id}/suggest-tasks", post(ai_suggest_tasks))
}

fn hub() -> Result<std::sync::Arc<projecthub::ProjectHub>, (StatusCode, String)> {
    projecthub::hub().ok_or((StatusCode::SERVICE_UNAVAILABLE, "ProjectHub extension is not initialised".into()))
}

fn bad(err: String) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err)
}

async fn list_extensions() -> ApiResult<Value> {
    let hub = hub()?;
    let mut list = vec![hub.status().await];
    if let Some(aw) = crate::alertwim::get() {
        list.push(aw.status().await);
    }
    Ok(Json(Value::Array(list)))
}

async fn status() -> ApiResult<Value> {
    Ok(Json(hub()?.status().await))
}

#[derive(Deserialize, Default)]
struct InstallBody {
    repo: Option<String>,
    branch: Option<String>,
}

async fn install(body: Option<Json<InstallBody>>) -> ApiResult<Value> {
    let hub = hub()?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    hub.start_install(body.repo, body.branch).map_err(|e| (StatusCode::CONFLICT, e))?;
    Ok(Json(hub.status().await))
}

/// Re-scan the machine for an installed Project Hub.
async fn detect() -> ApiResult<Value> {
    let hub = hub()?;
    hub.detection(true).await;
    Ok(Json(hub.status().await))
}

#[derive(Deserialize, Default)]
struct UseInstalledBody {
    /// One of the detected binaries; default = the preferred one.
    binary: Option<std::path::PathBuf>,
}

async fn use_installed(body: Option<Json<UseInstalledBody>>) -> ApiResult<Value> {
    let hub = hub()?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    hub.use_installed(body.binary).await.map_err(bad)?;
    Ok(Json(hub.status().await))
}

async fn self_test() -> ApiResult<Value> {
    let hub = hub()?;
    let report = hub.self_test().await.map_err(bad)?;
    Ok(Json(json!({ "report": report, "status": hub.status().await })))
}

async fn enable() -> ApiResult<Value> {
    let hub = hub()?;
    hub.enable().await.map_err(bad)?;
    Ok(Json(hub.status().await))
}

async fn disable() -> ApiResult<Value> {
    let hub = hub()?;
    hub.disable().await.map_err(bad)?;
    Ok(Json(hub.status().await))
}

async fn restart() -> ApiResult<Value> {
    let hub = hub()?;
    hub.restart().await.map_err(bad)?;
    Ok(Json(hub.status().await))
}

#[derive(Deserialize)]
struct ConfigBody {
    commands: Option<String>,
    /// `""` resets to the shared desktop database.
    db_path: Option<String>,
    repo: Option<String>,
    branch: Option<String>,
}

async fn update_config(Json(body): Json<ConfigBody>) -> ApiResult<Value> {
    let hub = hub()?;
    if let Some(c) = body.commands.as_deref() {
        if !matches!(c, "off" | "detected" | "any") {
            return Err(bad("commands must be off | detected | any".into()));
        }
    }
    let db_changed = body.db_path.is_some();
    let cfg = hub
        .update(|cfg| {
            if let Some(c) = body.commands {
                cfg.commands = c;
            }
            if let Some(db) = body.db_path {
                cfg.db_path = Some(db.trim().to_string()).filter(|d| !d.is_empty());
            }
            if let Some(r) = body.repo.filter(|r| !r.trim().is_empty()) {
                cfg.repo = r.trim().to_string();
            }
            if let Some(b) = body.branch.filter(|b| !b.trim().is_empty()) {
                cfg.branch = b.trim().to_string();
            }
        })
        .await
        .map_err(bad)?;
    if db_changed {
        hub.detection(true).await;
    }
    // New flags apply on (re)start.
    if hub.is_running().await && cfg.enabled {
        hub.restart().await.map_err(bad)?;
    }
    Ok(Json(hub.status().await))
}

async fn uninstall() -> ApiResult<Value> {
    let hub = hub()?;
    hub.uninstall().await.map_err(bad)?;
    Ok(Json(hub.status().await))
}

/// Transparent proxy so the UI can talk to ProjectHub without its token.
async fn proxy(method: Method, Path(rest): Path<String>, RawQuery(query): RawQuery, body: Bytes) -> Response {
    let client = match projecthub::client().await {
        Ok(c) => c,
        Err(err) => {
            return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": err, "code": "projecthub_unavailable" })))
                .into_response();
        }
    };
    let path = match query {
        Some(q) if !q.is_empty() => format!("{rest}?{q}"),
        _ => rest,
    };
    let body = (!body.is_empty()).then(|| body.to_vec());
    match client.raw(method, &path, body).await {
        Ok((status, text)) => {
            let mut resp = (StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), text).into_response();
            resp.headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
            resp
        }
        Err(err) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": err, "code": "projecthub_error" }))).into_response(),
    }
}

// ─── AI actions ─────────────────────────────────────────────────────────────

async fn ph() -> Result<PhClient, (StatusCode, String)> {
    projecthub::client().await.map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))
}

/// Facts an LLM needs about a project: `.brdd` summary, notes, layout, README, history.
async fn project_facts(client: &PhClient, id: &str) -> Result<(Value, String), (StatusCode, String)> {
    let detail = client.get(&format!("projects/{id}/detail")).await.map_err(bad)?;
    let mut bundle = client.get(&format!("projects/{id}/brdd")).await.map_err(bad)?;
    if bundle["exists"] != Value::Bool(true) {
        client
            .post(&format!("projects/{id}/brdd/refresh"), json!({}))
            .await
            .map_err(bad)?;
        bundle = client.get(&format!("projects/{id}/brdd")).await.map_err(bad)?;
    }
    let tree = client.get(&format!("projects/{id}/tree?depth=2")).await.unwrap_or(Value::Null);
    let paths: Vec<&str> = tree["entries"]
        .as_array()
        .map(|e| e.iter().filter_map(|x| x["path"].as_str()).take(200).collect())
        .unwrap_or_default();
    let readme = client
        .get(&format!("projects/{id}/file?path=README.md&max_bytes=6000"))
        .await
        .ok()
        .and_then(|f| f["content"].as_str().map(str::to_string));
    let commits: Vec<String> = detail["git"]["recent_commits"]
        .as_array()
        .map(|c| {
            c.iter()
                .map(|x| format!("- {} {}", x["hash"].as_str().unwrap_or(""), x["message"].as_str().unwrap_or("")))
                .collect()
        })
        .unwrap_or_default();
    let tasks: Vec<String> = detail["tasks"]
        .as_array()
        .map(|t| {
            t.iter()
                .map(|x| format!("- [{}] {}", x["status"].as_str().unwrap_or("?"), x["title"].as_str().unwrap_or("")))
                .collect()
        })
        .unwrap_or_default();

    let mut out = String::new();
    out.push_str(&scenario::truncate(bundle["summary_md"].as_str().unwrap_or("(no .brdd summary)"), 8000));
    if let Some(notes) = bundle["notes_md"].as_str().filter(|n| !n.trim().is_empty()) {
        out.push_str(&format!("\n\n## Existing AI notes\n{}", scenario::truncate(notes, 3000)));
    }
    if !paths.is_empty() {
        out.push_str(&format!("\n\n## Layout (depth 2)\n{}", paths.join("\n")));
    }
    if let Some(r) = readme {
        out.push_str(&format!("\n\n## README.md\n{}", scenario::truncate(&r, 5000)));
    }
    if !commits.is_empty() {
        out.push_str(&format!("\n\n## Recent commits\n{}", commits.join("\n")));
    }
    out.push_str(&format!(
        "\n\n## Tasks in Project Hub\n{}",
        if tasks.is_empty() { "(none)".to_string() } else { tasks.join("\n") }
    ));
    Ok((detail, out))
}

#[derive(Deserialize, Default)]
struct AnalyzeBody {
    locale: Option<String>,
}

fn lang(locale: Option<&str>) -> &'static str {
    if locale.unwrap_or("ru").starts_with("en") {
        "Write in English."
    } else {
        "Пиши на русском языке."
    }
}

/// AI writes an engineering brief into `.brdd/ai-summary.md`.
async fn ai_analyze(State(state): State<SharedState>, Path(id): Path<String>, body: Option<Json<AnalyzeBody>>) -> ApiResult<Value> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let client = ph().await?;
    let (detail, facts) = project_facts(&client, &id).await?;
    let llm = llm::resolve(&state, &Objective::new("projecthub")).await.map_err(bad)?;
    let system = format!(
        "You are a senior engineer writing a project brief for teammates and AI agents. {} \
         Return Markdown only (no code fences around the whole answer) with these sections, headings \
         written in the output language: Purpose · Architecture & modules · Stack · How to run & test · \
         Current state (recent changes, uncommitted work, versions) · Risks & tech debt · Next steps \
         (3-7 concrete items). \
         Be specific to THIS codebase; do not invent files that are not listed.",
        lang(body.locale.as_deref())
    );
    let user = format!(
        "Project: {} ({})\n\n{facts}",
        detail["project"]["name"].as_str().unwrap_or("?"),
        detail["project"]["root_path"].as_str().unwrap_or("?")
    );
    let notes = llm
        .chat(&system, &user, 0.2, 16_000, 300_000)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    let stamped = format!(
        "{}\n\n---\n_Generated by BoardDo ({}) on {}._\n",
        notes.trim(),
        llm.model,
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    );
    let bundle = client
        .put(&format!("projects/{id}/brdd/notes"), json!({ "markdown": stamped }))
        .await
        .map_err(bad)?;
    Ok(Json(json!({ "model": llm.model, "bundle": bundle })))
}

#[derive(Deserialize, Default)]
struct SuggestBody {
    instruction: Option<String>,
    count: Option<usize>,
    #[serde(default)]
    apply: bool,
    locale: Option<String>,
    /// Open GitHub issues for the created tasks.
    #[serde(default)]
    github_issues: bool,
}

/// AI proposes tasks for the project; `apply` creates them in Project Hub.
async fn ai_suggest_tasks(State(state): State<SharedState>, Path(id): Path<String>, body: Option<Json<SuggestBody>>) -> ApiResult<Value> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let count = body.count.unwrap_or(5).clamp(1, 15);
    let client = ph().await?;
    let (detail, facts) = project_facts(&client, &id).await?;
    let llm = llm::resolve(&state, &Objective::new("projecthub")).await.map_err(bad)?;
    let system = format!(
        "You plan engineering work for a code project. {} Propose up to {count} concrete, independent tasks \
         that move the project forward (bugs, missing tests, docs, refactors, features from the README/notes, \
         release chores). Do not duplicate existing open tasks. Each task: imperative title (≤ 80 chars), \
         description with acceptance criteria and the files/modules involved, priority low|medium|high|critical, \
         due_in_days (integer or null). Return ONLY JSON: \
         {{\"tasks\":[{{\"title\":\"…\",\"description\":\"…\",\"priority\":\"medium\",\"due_in_days\":null}}]}}",
        lang(body.locale.as_deref())
    );
    let mut user = format!("Project: {}\n\n{facts}", detail["project"]["name"].as_str().unwrap_or("?"));
    if let Some(i) = body.instruction.as_deref().filter(|s| !s.trim().is_empty()) {
        user.push_str(&format!("\n\nFocus requested by the user: {i}"));
    }
    let raw = llm
        .chat(&system, &user, 0.3, 16_000, 300_000)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    let parsed = extract_json_object(&raw).ok_or_else(|| (StatusCode::BAD_GATEWAY, "AI reply is not JSON".to_string()))?;
    let drafts: Vec<Value> = parsed["tasks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t["title"].as_str().is_some_and(|s| !s.trim().is_empty()))
        .take(count)
        .collect();

    let mut created = Vec::new();
    let mut errors = Vec::new();
    if body.apply {
        for d in &drafts {
            let due = d["due_in_days"]
                .as_i64()
                .filter(|n| *n > 0 && *n < 3650)
                .map(|n| (chrono::Utc::now() + chrono::Duration::days(n)).to_rfc3339());
            let res = client
                .post(
                    "tasks",
                    json!({
                        "project_id": id,
                        "title": d["title"],
                        "description": d["description"],
                        "priority": d["priority"].as_str().unwrap_or("medium"),
                        "due_at": due,
                        "source": "boarddo",
                        "external_ref": "ai:suggest-tasks",
                    }),
                )
                .await;
            match res {
                Ok(mut task) => {
                    if body.github_issues {
                        let tid = task["id"].as_str().unwrap_or_default().to_string();
                        match client.post(&format!("tasks/{tid}/github-issue"), json!({ "labels": ["boarddo"] })).await {
                            Ok(t) => task = t,
                            Err(e) => errors.push(e),
                        }
                    }
                    created.push(task);
                }
                Err(e) => errors.push(e),
            }
        }
    }
    Ok(Json(json!({ "model": llm.model, "tasks": drafts, "created": created, "errors": errors })))
}
