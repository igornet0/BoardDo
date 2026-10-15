//! Tracks goal items that live as ProjectHub tasks: polls their status and
//! completes the items (resuming the goal) when the work is done there.

use std::time::Duration;

use boarddo_shared::v2::{ObjectiveItemKind, ObjectiveItemStatus, ObjectiveStatus, RunObjectiveRequest};
use chrono::Utc;
use serde_json::Value;

use crate::objectives::{self, runner};
use crate::state::SharedState;

const POLL_EVERY: Duration = Duration::from_secs(15);

pub fn spawn(state: SharedState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(POLL_EVERY).await;
            let Some(hub) = super::hub() else { continue };
            let cfg = hub.config().await;
            if !(cfg.installed && cfg.enabled) {
                continue;
            }
            if let Err(err) = sync(&state).await {
                tracing::debug!(error = %err, "projecthub.tracker");
            }
        }
    });
}

pub fn done_text(ru: bool, status: &str) -> String {
    match (ru, status) {
        (true, "cancelled") => "Задача отменена в ProjectHub".into(),
        (true, _) => "Задача выполнена в ProjectHub".into(),
        (false, "cancelled") => "Task cancelled in ProjectHub".into(),
        (false, _) => "Task completed in ProjectHub".into(),
    }
}

/// One pass over every goal with running `task` items. Returns how many items changed.
pub async fn sync(state: &SharedState) -> Result<usize, String> {
    let goals = state.storage.list_objectives().await.map_err(|e| e.to_string())?;
    let watched: Vec<_> = goals
        .into_iter()
        .filter(|o| {
            o.items.iter().any(|i| {
                i.kind == ObjectiveItemKind::Task
                    && i.status == ObjectiveItemStatus::Running
                    && i.external_task_id.is_some()
            })
        })
        .collect();
    if watched.is_empty() {
        return Ok(0);
    }
    let client = super::client().await?;
    let mut changed_total = 0;

    for goal in watched {
        let ids: Vec<String> = goal
            .items
            .iter()
            .filter(|i| i.kind == ObjectiveItemKind::Task && i.status == ObjectiveItemStatus::Running)
            .filter_map(|i| i.external_task_id.clone())
            .collect();
        let tasks = client.get(&format!("tasks?ids={}", ids.join(","))).await?;
        let tasks = tasks.as_array().cloned().unwrap_or_default();
        let find = |id: &str| tasks.iter().find(|t| t["id"].as_str() == Some(id)).cloned();
        let any_finished = tasks
            .iter()
            .any(|t| matches!(t["status"].as_str(), Some("done" | "cancelled")));
        // Evidence from the code: commits made while the task was open.
        let commits: Vec<Value> = match (&goal.project_hub_project_id, any_finished) {
            (Some(project), true) => client
                .get(&format!("projects/{project}/git?limit=50"))
                .await
                .ok()
                .and_then(|g| g["recent_commits"].as_array().cloned())
                .unwrap_or_default(),
            _ => Vec::new(),
        };

        let mut changed = 0;
        let updated = objectives::mutate(state, goal.id, |o| {
            let ru = objectives::scenario::is_russian(objectives::scenario::objective_locale(o));
            let mut log = Vec::new();
            for item in o.items.iter_mut().filter(|i| {
                i.kind == ObjectiveItemKind::Task && i.status == ObjectiveItemStatus::Running
            }) {
                let Some(task) = item.external_task_id.as_deref().and_then(find) else {
                    continue;
                };
                let status = task["status"].as_str().unwrap_or("todo").to_string();
                let url = task["link"]["github_issue_url"].as_str().map(str::to_string);
                if url.is_some() {
                    item.external_url = url;
                }
                if item.external_status.as_deref() != Some(status.as_str()) {
                    item.external_status = Some(status.clone());
                    changed += 1;
                }
                let next = match status.as_str() {
                    "done" => ObjectiveItemStatus::Done,
                    "cancelled" => ObjectiveItemStatus::Skipped,
                    _ => continue,
                };
                item.status = next;
                item.finished_at = Some(Utc::now());
                item.result = Some(task_result(ru, &task, &status, &commits_since(&commits, item.started_at)));
                changed += 1;
                log.push((item.id.clone(), item.title.clone(), next));
            }
            for (id, title, next) in log {
                let msg = match next {
                    ObjectiveItemStatus::Done => format!("✓ «{title}» — {}", done_text(ru, "done")),
                    _ => format!("«{title}» — {}", done_text(ru, "cancelled")),
                };
                o.push_log(if next == ObjectiveItemStatus::Done { "success" } else { "warning" }, Some(&id), msg);
            }
            Ok(())
        })
        .await?;
        changed_total += changed;
        if changed > 0 && any_finished {
            if let Some(project) = &goal.project_hub_project_id {
                // Keep versions / changelog current for the next items and the evaluation.
                let _ = client.post(&format!("projects/{project}/brdd/refresh"), serde_json::json!({})).await;
            }
        }

        // Work finished while the goal waited → continue the run.
        let settled_any = updated
            .items
            .iter()
            .any(|i| i.kind == ObjectiveItemKind::Task && i.status.is_settled());
        if changed > 0
            && settled_any
            && matches!(updated.status, ObjectiveStatus::Paused | ObjectiveStatus::Running)
            && !runner::is_looping(updated.id)
            && !updated.log.iter().rev().take(5).any(|l| l.message.starts_with("⏸"))
        {
            let _ = runner::start(state, updated.id, RunObjectiveRequest::default()).await;
        }
    }
    Ok(changed_total)
}

fn commits_since(commits: &[Value], since: Option<chrono::DateTime<Utc>>) -> Vec<String> {
    let Some(since) = since else { return Vec::new() };
    commits
        .iter()
        .filter(|c| {
            c["date"]
                .as_str()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .is_some_and(|d| d.with_timezone(&Utc) >= since)
        })
        .map(|c| {
            format!(
                "`{}` {} (author: {}, {})",
                c["hash"].as_str().unwrap_or(""),
                c["message"].as_str().unwrap_or(""),
                c["author"].as_str().unwrap_or("?"),
                c["date"].as_str().unwrap_or("").get(..16).unwrap_or("")
            )
        })
        .collect()
}

fn task_result(ru: bool, task: &Value, status: &str, commits: &[String]) -> String {
    let mut out = done_text(ru, status);
    if let Some(title) = task["title"].as_str() {
        out.push_str(&format!(": «{title}»"));
    }
    if let Some(desc) = task["description"].as_str().filter(|d| !d.trim().is_empty()) {
        out.push_str(&format!("\n\n{}", desc.trim()));
    }
    if let Some(url) = task["link"]["github_issue_url"].as_str() {
        out.push_str(&format!("\n\nGitHub: {url}"));
    }
    out.push_str(if ru {
        "\n\n### Коммиты за время задачи\n"
    } else {
        "\n\n### Commits while the task was open\n"
    });
    if commits.is_empty() {
        out.push_str(if ru { "— нет новых коммитов" } else { "— no new commits" });
    } else {
        for c in commits.iter().take(20) {
            out.push_str(&format!("- {c}\n"));
        }
    }
    out
}
