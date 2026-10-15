//! `projecthub.*` nodes — projects, code, `.brdd`, tasks and GitHub through the
//! ProjectHub extension (`crate::projecthub`). Available once it is installed
//! and enabled; otherwise each node fails with a clear message.

use async_trait::async_trait;
use boarddo_shared::Node;
use serde_json::{Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};
use crate::projecthub::{self, PhClient, client::urlencode};

pub const PROJECTS: &str = "projecthub.projects";
pub const PROJECT: &str = "projecthub.project";
pub const BRDD: &str = "projecthub.brdd";
pub const CODE: &str = "projecthub.code";
pub const GIT: &str = "projecthub.git";
pub const TASKS: &str = "projecthub.tasks";
pub const TASK_CREATE: &str = "projecthub.task.create";
pub const TASK_UPDATE: &str = "projecthub.task.update";
pub const COMMAND: &str = "projecthub.command";
pub const GITHUB: &str = "projecthub.github";

pub const ALL: &[&str] = &[
    PROJECTS,
    PROJECT,
    BRDD,
    CODE,
    GIT,
    TASKS,
    TASK_CREATE,
    TASK_UPDATE,
    COMMAND,
    GITHUB,
];

pub struct ProjectHubNode(pub &'static str);

fn text(node: &Node, ctx: &ExecutionContext, key: &str) -> Option<String> {
    let raw = node.config.get(key)?.clone();
    match ctx.resolve_value(raw).ok()? {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Null => None,
        other => Some(other.to_string().trim_matches('"').to_string()),
    }
}

fn number(node: &Node, ctx: &ExecutionContext, key: &str) -> Option<u64> {
    text(node, ctx, key).and_then(|s| s.parse().ok())
}

fn flag(node: &Node, key: &str) -> bool {
    node.config.get(key).and_then(Value::as_bool).unwrap_or(false)
}

async fn project_id(client: &PhClient, node: &Node, ctx: &ExecutionContext, kind: &str) -> Result<String, String> {
    let raw = text(node, ctx, "project")
        .or_else(|| text(node, ctx, "project_id"))
        .ok_or_else(|| format!("{kind}: `project` is required (id or name)"))?;
    let project = client.resolve_project(&raw).await?;
    project["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{kind}: project `{raw}` has no id"))
}

#[async_trait]
impl NodeHandler for ProjectHubNode {
    fn type_id(&self) -> &'static str {
        self.0
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let kind = self.0;
        let client = projecthub::client().await.map_err(|e| format!("{kind}: {e}"))?;
        let ctx = &*ctx;
        let out = match kind {
            PROJECTS => {
                let limit = number(node, ctx, "limit").unwrap_or(50).clamp(1, 500);
                let q = text(node, ctx, "query").map(|q| format!("&q={}", urlencode(&q))).unwrap_or_default();
                client.get(&format!("projects?limit={limit}{q}")).await?
            }
            PROJECT => {
                let id = project_id(&client, node, ctx, kind).await?;
                client.get(&format!("projects/{id}/detail")).await?
            }
            BRDD => {
                let id = project_id(&client, node, ctx, kind).await?;
                match text(node, ctx, "action").as_deref().unwrap_or("read") {
                    "refresh" => {
                        client
                            .post(&format!("projects/{id}/brdd/refresh"), json!({ "force_snapshot": flag(node, "force_snapshot") }))
                            .await?
                    }
                    "notes" => {
                        let markdown = text(node, ctx, "notes").ok_or(format!("{kind}: `notes` is required"))?;
                        client.put(&format!("projects/{id}/brdd/notes"), json!({ "markdown": markdown })).await?
                    }
                    _ => client.get(&format!("projects/{id}/brdd")).await?,
                }
            }
            CODE => {
                let id = project_id(&client, node, ctx, kind).await?;
                match text(node, ctx, "action").as_deref().unwrap_or("tree") {
                    "file" => {
                        let path = text(node, ctx, "path").ok_or(format!("{kind}: `path` is required"))?;
                        let max = number(node, ctx, "max_bytes").map(|m| format!("&max_bytes={m}")).unwrap_or_default();
                        client.get(&format!("projects/{id}/file?path={}{max}", urlencode(&path))).await?
                    }
                    "search" => {
                        let q = text(node, ctx, "query").ok_or(format!("{kind}: `query` is required"))?;
                        let limit = number(node, ctx, "limit").unwrap_or(100);
                        client
                            .get(&format!("projects/{id}/search?q={}&limit={limit}", urlencode(&q)))
                            .await?
                    }
                    _ => {
                        let path = text(node, ctx, "path").unwrap_or_default();
                        let depth = number(node, ctx, "depth").unwrap_or(2);
                        client
                            .get(&format!("projects/{id}/tree?path={}&depth={depth}", urlencode(&path)))
                            .await?
                    }
                }
            }
            GIT => {
                let id = project_id(&client, node, ctx, kind).await?;
                let limit = number(node, ctx, "limit").unwrap_or(20);
                client.get(&format!("projects/{id}/git?limit={limit}")).await?
            }
            TASKS => {
                let mut q = Vec::new();
                if text(node, ctx, "project").is_some() || text(node, ctx, "project_id").is_some() {
                    q.push(format!("project_id={}", project_id(&client, node, ctx, kind).await?));
                }
                for key in ["status", "source", "ids"] {
                    if let Some(v) = text(node, ctx, key) {
                        q.push(format!("{key}={}", urlencode(&v)));
                    }
                }
                let tasks = client.get(&format!("tasks?{}", q.join("&"))).await?;
                let count = tasks.as_array().map(|a| a.len()).unwrap_or(0);
                json!({ "count": count, "tasks": tasks })
            }
            TASK_CREATE => {
                let title = text(node, ctx, "title").ok_or(format!("{kind}: `title` is required"))?;
                let project = match text(node, ctx, "project").or_else(|| text(node, ctx, "project_id")) {
                    Some(_) => Some(project_id(&client, node, ctx, kind).await?),
                    None => None,
                };
                let created = client
                    .post(
                        "tasks",
                        json!({
                            "project_id": project,
                            "title": title,
                            "description": text(node, ctx, "description"),
                            "priority": text(node, ctx, "priority"),
                            "due_at": text(node, ctx, "due_at"),
                            "source": text(node, ctx, "source").unwrap_or_else(|| "boarddo".into()),
                            "external_ref": text(node, ctx, "external_ref")
                                .unwrap_or_else(|| format!("workflow:{}", ctx.workflow_id)),
                        }),
                    )
                    .await?;
                if flag(node, "github_issue") {
                    let id = created["id"].as_str().unwrap_or_default();
                    client.post(&format!("tasks/{id}/github-issue"), json!({ "labels": ["boarddo"] })).await?
                } else {
                    created
                }
            }
            TASK_UPDATE => {
                let id = text(node, ctx, "task_id").ok_or(format!("{kind}: `task_id` is required"))?;
                let mut patch = serde_json::Map::new();
                for key in ["status", "title", "description", "priority", "due_at"] {
                    if let Some(v) = text(node, ctx, key) {
                        patch.insert(key.into(), json!(v));
                    }
                }
                if patch.is_empty() {
                    return Err(format!("{kind}: nothing to update"));
                }
                client.patch(&format!("tasks/{id}"), Value::Object(patch)).await?
            }
            COMMAND => {
                let id = project_id(&client, node, ctx, kind).await?;
                let command = text(node, ctx, "command").ok_or(format!("{kind}: `command` is required"))?;
                // Accept a command name ("test", "Build") as well as the exact command line.
                let detected = client.get(&format!("projects/{id}/commands")).await.unwrap_or_default();
                let wanted = command.trim().to_lowercase();
                let command = detected["commands"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|c| {
                        c["command"].as_str().is_some_and(|x| x.to_lowercase() == wanted)
                            || c["name"].as_str().is_some_and(|x| x.to_lowercase() == wanted)
                    })
                    .or_else(|| {
                        detected["commands"].as_array().into_iter().flatten().find(|c| {
                            c["command"].as_str().is_some_and(|x| {
                                x.to_lowercase().split_whitespace().last() == Some(wanted.as_str())
                            })
                        })
                    })
                    .and_then(|c| c["command"].as_str())
                    .map(str::to_string)
                    .unwrap_or(command);
                let result = client
                    .post(
                        &format!("projects/{id}/commands/run"),
                        json!({ "command": command, "timeout_secs": number(node, ctx, "timeout_secs").unwrap_or(300) }),
                    )
                    .await?;
                if flag(node, "fail_on_error") && result["ok"] != Value::Bool(true) {
                    return Err(format!(
                        "{kind}: `{}` exited with {} — {}",
                        result["command"].as_str().unwrap_or(""),
                        result["exit_code"],
                        result["stderr"].as_str().unwrap_or("").chars().rev().take(400).collect::<String>().chars().rev().collect::<String>()
                    ));
                }
                result
            }
            GITHUB => match text(node, ctx, "action").as_deref().unwrap_or("sync") {
                "create_issue" => {
                    let id = text(node, ctx, "task_id").ok_or(format!("{kind}: `task_id` is required"))?;
                    let labels: Vec<String> = text(node, ctx, "labels")
                        .map(|l| l.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                        .unwrap_or_default();
                    client.post(&format!("tasks/{id}/github-issue"), json!({ "labels": labels })).await?
                }
                "issues" => {
                    let id = project_id(&client, node, ctx, kind).await?;
                    let state = text(node, ctx, "state").unwrap_or_else(|| "open".into());
                    let issues = client.get(&format!("projects/{id}/issues?state={state}")).await?;
                    json!({ "count": issues.as_array().map(|a| a.len()).unwrap_or(0), "issues": issues })
                }
                _ => client.post("github/issues/sync", json!({})).await?,
            },
            other => return Err(format!("unknown ProjectHub node `{other}`")),
        };
        Ok(NodeOutput::data(out))
    }
}
