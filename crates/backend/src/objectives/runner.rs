//! Executes an objective: ready items run one by one through their scenarios,
//! then the AI evaluates the outcome against the success criteria.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use boarddo_shared::v2::{
    Objective, ObjectiveAssistMode, ObjectiveAssistRequest, ObjectiveItem, ObjectiveItemKind,
    ObjectiveItemStatus, ObjectiveStatus, RunObjectiveRequest, TaskKind,
};
use boarddo_shared::{ExecutionStatus, UpdateWorkflowRequest, WorkflowRecord, WorkflowStatus};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use super::llm::{self, ObjectiveLlm};
use super::scenario::{self, is_russian, objective_locale};
use super::{mutate, tr};
use crate::api::run::spawn_execution;
use crate::integrations::openai::extract_json_object;
use crate::state::SharedState;

/// Objectives with a live run loop in this process.
static RUNNING: LazyLock<Mutex<HashSet<Uuid>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
const EXECUTION_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_AUTO_RETRIES: u32 = 1;

struct LoopGuard(Uuid);

impl Drop for LoopGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = RUNNING.lock() {
            set.remove(&self.0);
        }
    }
}

pub fn is_looping(id: Uuid) -> bool {
    RUNNING.lock().map(|s| s.contains(&id)).unwrap_or(false)
}

/// Mark the objective running and make sure a loop is working on it.
pub async fn start(
    state: &SharedState,
    id: Uuid,
    req: RunObjectiveRequest,
) -> Result<Objective, String> {
    let only = req.item_id.clone();
    let updated = mutate(state, id, |o| {
        if let Some(item_id) = only.as_deref() {
            let Some(item) = o.item(item_id) else {
                return Err(format!("item `{item_id}` not found"));
            };
            if !o.is_leaf(item_id) {
                return Err(tr(o, "Пункт состоит из подпунктов — запустите их", "Item has sub-items — run those instead"));
            }
            if item.status == ObjectiveItemStatus::Running {
                return Err(tr(o, "Пункт уже выполняется", "Item is already running"));
            }
        } else if o.items.is_empty() {
            return Err(tr(o, "В цели нет пунктов — заполните её через ИИ или добавьте вручную", "Objective has no items — fill it with AI or add them manually"));
        }

        let leaves: Vec<String> = o.items.iter().map(|i| i.id.clone()).filter(|id| o.is_leaf(id)).collect();
        for leaf in leaves {
            let Some(item) = o.item_mut(&leaf) else { continue };
            let selected = only.as_deref() == Some(leaf.as_str());
            let reset = item.status != ObjectiveItemStatus::Running
                && (selected
                    || req.reset
                    || (req.retry_failed && item.status == ObjectiveItemStatus::Failed));
            if reset {
                item.reset();
                item.attempts = 0;
            }
        }
        if req.reset || o.status == ObjectiveStatus::Done {
            o.refine_rounds = 0;
        }
        if req.reset {
            o.summary = None;
            o.achieved = None;
        }
        o.status = ObjectiveStatus::Running;
        o.finished_at = None;
        if o.started_at.is_none() || req.reset {
            o.started_at = Some(Utc::now());
        }
        let msg = match only.as_deref().and_then(|i| o.item(i)) {
            Some(item) => format!("{} «{}»", tr(o, "▶ Запуск пункта", "▶ Running item"), item.title),
            None => tr(o, "▶ Запуск цели", "▶ Objective started"),
        };
        o.push_log("info", only.as_deref(), msg);
        Ok(())
    })
    .await?;

    let spawn = RUNNING.lock().map(|mut s| s.insert(id)).unwrap_or(false);
    if spawn {
        let state = state.clone();
        tokio::spawn(async move {
            let _guard = LoopGuard(id);
            let scope = boarddo_shared::v2::LedgerScope {
                objective_id: Some(id),
                ..Default::default()
            };
            crate::ledger::scoped(scope, run_loop(&state, id, only)).await;
        });
    }
    Ok(updated)
}

pub async fn stop(state: &SharedState, id: Uuid) -> Result<Objective, String> {
    mutate(state, id, |o| {
        if o.status == ObjectiveStatus::Running {
            o.status = ObjectiveStatus::Paused;
            let msg = tr(o, "⏸ Остановлено пользователем — текущий пункт завершится", "⏸ Stopped by user — the current item will finish");
            o.push_log("warning", None, msg);
        }
        Ok(())
    })
    .await
}

async fn run_loop(state: &SharedState, id: Uuid, only: Option<String>) {
    let mut only = only;
    loop {
        let Ok(Some(o)) = state.storage.get_objective(id).await else {
            return;
        };
        if o.status != ObjectiveStatus::Running {
            return;
        }

        let next = match only.take() {
            Some(item_id) => match o.item(&item_id) {
                Some(item) if item.status == ObjectiveItemStatus::Pending => Some(item_id),
                _ => None,
            },
            None => o
                .ready_items()
                .into_iter()
                .find(|i| o.item(i).is_some_and(|x| x.kind != ObjectiveItemKind::Manual)),
        };

        let Some(item_id) = next else {
            if finalize(state, id).await {
                // Refinement appended items — keep going.
                continue;
            }
            return;
        };

        let is_task = o.item(&item_id).is_some_and(|i| i.kind == ObjectiveItemKind::Task);
        let result = if is_task {
            // Hand the work to ProjectHub and move on; the tracker completes it.
            start_task_item(state, id, &item_id).await
        } else {
            run_item(state, id, &item_id).await
        };
        if let Err(err) = result {
            tracing::warn!(objective = %id, item = %item_id, error = %err, "objective.item_failed");
        }
    }
}

/// `task` item: create (or re-attach to) its ProjectHub task and mark it running.
pub async fn start_task_item(state: &SharedState, id: Uuid, item_id: &str) -> Result<(), String> {
    let o = state
        .storage
        .get_objective(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("objective not found")?;
    let item = o.item(item_id).cloned().ok_or_else(|| format!("item `{item_id}` not found"))?;
    let outcome = create_or_reuse_task(&o, &item).await;
    mutate(state, id, |o| {
        let ru = is_russian(objective_locale(o));
        let item = o.item_mut(item_id).ok_or("item not found")?;
        let title = item.title.clone();
        match &outcome {
            Ok(task) => {
                let status = task["status"].as_str().unwrap_or("todo").to_string();
                item.external_task_id = task["id"].as_str().map(str::to_string);
                item.external_url = task["link"]["github_issue_url"].as_str().map(str::to_string);
                item.started_at.get_or_insert_with(Utc::now);
                item.error = None;
                item.status = match status.as_str() {
                    "done" => ObjectiveItemStatus::Done,
                    "cancelled" => ObjectiveItemStatus::Skipped,
                    _ => ObjectiveItemStatus::Running,
                };
                if item.status.is_settled() {
                    item.finished_at = Some(Utc::now());
                    item.result.get_or_insert_with(|| crate::projecthub::tracker::done_text(ru, &status));
                }
                item.external_status = Some(status);
                let msg = format!(
                    "{} «{title}»",
                    if ru { "→ Задача в ProjectHub:" } else { "→ ProjectHub task:" }
                );
                o.push_log("info", Some(item_id), msg);
            }
            Err(err) => {
                item.status = ObjectiveItemStatus::Failed;
                item.error = Some(err.clone());
                item.finished_at = Some(Utc::now());
                let msg = format!("✕ «{title}»: {err}");
                o.push_log("error", Some(item_id), msg);
            }
        }
        Ok(())
    })
    .await?;
    outcome.map(|_| ())
}

async fn create_or_reuse_task(o: &Objective, item: &ObjectiveItem) -> Result<serde_json::Value, String> {
    let project = o.project_hub_project_id.clone().ok_or_else(|| {
        tr(
            o,
            "Пункт «Задача» требует проект ProjectHub — выберите его в характеристиках цели",
            "A task item needs a ProjectHub project — pick one in the goal's characteristics",
        )
    })?;
    let client = crate::projecthub::client().await?;
    if let Some(existing) = item.external_task_id.as_deref() {
        if let Ok(task) = client.get(&format!("tasks/{existing}")).await {
            return Ok(task);
        }
    }
    let mut description = item.description.trim().to_string();
    if let Some(p) = item.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
        description.push_str(&format!("\n\n{}", p.trim()));
    }
    description.push_str(&format!(
        "\n\n— {} «{}» (BoardDo, {})",
        tr(o, "Пункт цели", "Goal item"),
        o.title,
        scenario::item_path(o, &item.id)
    ));
    client
        .post(
            "tasks",
            serde_json::json!({
                "project_id": project,
                "title": item.title,
                "description": description.trim(),
                "priority": "medium",
                "due_at": o.deadline.map(|d| d.to_rfc3339()),
                "source": "boarddo",
                "external_ref": format!("objective:{}/{}", o.id, item.id),
            }),
        )
        .await
}

/// Run one leaf item through its scenario and store the result.
pub async fn run_item(state: &SharedState, id: Uuid, item_id: &str) -> Result<(), String> {
    let o = mutate(state, id, |o| {
        let item = o
            .item_mut(item_id)
            .ok_or_else(|| format!("item `{item_id}` not found"))?;
        item.status = ObjectiveItemStatus::Running;
        item.started_at = Some(Utc::now());
        item.finished_at = None;
        item.error = None;
        item.attempts += 1;
        let title = item.title.clone();
        let msg = format!("{} «{title}»", tr(o, "Выполняется", "Running"));
        o.push_log("info", Some(item_id), msg);
        Ok(())
    })
    .await?;

    let scope = boarddo_shared::v2::LedgerScope {
        objective_id: Some(id),
        item_id: Some(item_id.to_string()),
        ..Default::default()
    };
    let outcome = crate::ledger::scoped(scope, execute_item(state, &o, item_id)).await;
    let retryable = matches!(&outcome, Err(e) if is_transient(e));

    mutate(state, id, |o| {
        let ru = is_russian(objective_locale(o));
        let item = o
            .item_mut(item_id)
            .ok_or_else(|| format!("item `{item_id}` not found"))?;
        item.finished_at = Some(Utc::now());
        let title = item.title.clone();
        match &outcome {
            Ok(done) => {
                item.status = ObjectiveItemStatus::Done;
                item.result = Some(done.result.clone());
                item.execution_id = Some(done.execution_id);
                let msg = format!("✓ «{title}»");
                o.push_log("success", Some(item_id), msg);
            }
            Err(err) if retryable && item.attempts <= MAX_AUTO_RETRIES => {
                item.status = ObjectiveItemStatus::Pending;
                item.error = Some(err.clone());
                let msg = format!(
                    "{} «{title}»: {err}",
                    if ru { "Повтор после временной ошибки" } else { "Retrying after a transient error" }
                );
                o.push_log("warning", Some(item_id), msg);
            }
            Err(err) => {
                item.status = ObjectiveItemStatus::Failed;
                item.error = Some(err.clone());
                let msg = format!("✕ «{title}»: {err}");
                o.push_log("error", Some(item_id), msg);
            }
        }
        Ok(())
    })
    .await?;
    outcome.map(|_| ())
}

struct ItemDone {
    execution_id: Uuid,
    result: String,
}

async fn execute_item(state: &SharedState, o: &Objective, item_id: &str) -> Result<ItemDone, String> {
    let item = o
        .item(item_id)
        .ok_or_else(|| format!("item `{item_id}` not found"))?
        .clone();
    let wf = ensure_scenario(state, o, &item).await?;
    let project = project_context(o).await;
    let trigger = scenario::trigger_payload(o, &item, project.as_deref());
    let resp = spawn_execution(state, &wf, trigger, "manual")
        .await
        .map_err(|e| e.to_string())?;
    let execution_id = resp.execution_id;
    let _ = mutate(state, o.id, |o| {
        if let Some(i) = o.item_mut(item_id) {
            i.execution_id = Some(execution_id);
        }
        Ok(())
    })
    .await;

    let exec = wait_execution(state, execution_id).await?;
    let nodes = state
        .storage
        .get_node_executions(execution_id)
        .await
        .map_err(|e| e.to_string())?;
    match exec.status {
        // Completed with an error means a node reported a failure and the
        // run went on: the item is not done.
        ExecutionStatus::Completed if exec.error.is_some() => Err(format!(
            "scenario degraded: {}",
            exec.error.unwrap_or_default()
        )),
        ExecutionStatus::Completed => {
            let result = scenario::extract_result(&nodes).ok_or_else(|| {
                "scenario finished without output — give the final node id `result`".to_string()
            })?;
            Ok(ItemDone {
                execution_id,
                result,
            })
        }
        _ => {
            let node_err = nodes
                .iter()
                .rev()
                .find_map(|n| n.error.clone().map(|e| format!("{}: {e}", n.node_id)));
            Err(exec
                .error
                .or(node_err)
                .unwrap_or_else(|| format!("execution {:?}", exec.status).to_lowercase()))
        }
    }
}

/// `.brdd` digest of the goal's ProjectHub project (stack, layout, versions,
/// AI notes) — gives AI items real knowledge of the codebase.
pub async fn project_context(o: &Objective) -> Option<String> {
    let project = o.project_hub_project_id.as_deref()?;
    let client = crate::projecthub::client().await.ok()?;
    let mut bundle = client.get(&format!("projects/{project}/brdd")).await.ok()?;
    if bundle["exists"] != Value::Bool(true) {
        // First use: analyse the project so items get context.
        let _ = client.post(&format!("projects/{project}/brdd/refresh"), serde_json::json!({})).await;
        bundle = client.get(&format!("projects/{project}/brdd")).await.ok()?;
    }
    let mut out = String::new();
    if let Some(summary) = bundle["summary_md"].as_str() {
        out.push_str(&scenario::truncate(summary, 6000));
    }
    if let Some(notes) = bundle["notes_md"].as_str().filter(|n| !n.trim().is_empty()) {
        out.push_str("\n\n### AI notes\n");
        out.push_str(&scenario::truncate(notes, 2500));
    }
    if let Some(root) = bundle["analysis"]["project"]["root_path"].as_str() {
        out.push_str(&format!("\n\nPath: {root}"));
    }
    (!out.trim().is_empty()).then_some(out)
}

/// Item scenario: user-picked, existing generated, or freshly generated.
pub async fn ensure_scenario(
    state: &SharedState,
    o: &Objective,
    item: &ObjectiveItem,
) -> Result<WorkflowRecord, String> {
    let existing = match item.workflow_id {
        Some(wid) => state.storage.get_workflow(wid).await.map_err(|e| e.to_string())?,
        None => None,
    };

    if item.kind == ObjectiveItemKind::Workflow {
        return existing.ok_or_else(|| {
            tr(o, "Для пункта типа «сценарий» выберите сценарий", "Pick a scenario for this item")
        });
    }
    if item.kind == ObjectiveItemKind::Manual {
        return Err(tr(o, "Ручной пункт отмечается выполненным вручную", "Manual items are completed by hand"));
    }
    if item.kind == ObjectiveItemKind::Task {
        return Err(tr(o, "Пункт «Задача» выполняется в ProjectHub, не сценарием", "Task items run in ProjectHub, not as a scenario"));
    }

    let llm: ObjectiveLlm = llm::resolve_for(state, o, TaskKind::Generation).await?;
    let def = scenario::build_definition(o, item, &llm.connection_id.to_string(), &llm.model);
    let fresh_hash = scenario::definition_hash(&def.nodes, &def.edges);

    let wf = match existing {
        // Edited on the canvas → the user's version wins.
        Some(wf) if item.scenario_hash.as_deref() != Some(&scenario::definition_hash(&wf.definition.nodes, &wf.definition.edges)) => {
            return Ok(wf);
        }
        Some(wf) if item.scenario_hash.as_deref() == Some(fresh_hash.as_str()) => wf,
        Some(wf) => state
            .storage
            .update_workflow(
                wf.id,
                UpdateWorkflowRequest {
                    name: def.name.clone(),
                    description: def.description.clone(),
                    status: WorkflowStatus::Draft,
                    nodes: def.nodes.clone(),
                    edges: def.edges.clone(),
                },
            )
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "scenario disappeared".to_string())?,
        None => state
            .storage
            .create_workflow(def)
            .await
            .map_err(|e| e.to_string())?,
    };

    let wid = wf.id;
    let item_id = item.id.clone();
    let hash = fresh_hash.clone();
    mutate(state, o.id, move |o| {
        if let Some(i) = o.item_mut(&item_id) {
            i.workflow_id = Some(wid);
            i.scenario_hash = Some(hash);
        }
        Ok(())
    })
    .await?;
    Ok(wf)
}

async fn wait_execution(state: &SharedState, id: Uuid) -> Result<boarddo_shared::Execution, String> {
    let deadline = tokio::time::Instant::now() + EXECUTION_TIMEOUT;
    loop {
        if let Some(ex) = state.storage.get_execution(id).await.map_err(|e| e.to_string())?
            && matches!(
                ex.status,
                ExecutionStatus::Completed | ExecutionStatus::Failed | ExecutionStatus::Cancelled
            )
        {
            return Ok(ex);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!("scenario timed out after {} min", EXECUTION_TIMEOUT.as_secs() / 60));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

fn is_transient(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    [
        "http 5",
        "http 429",
        "overloaded",
        "timed out",
        "timeout",
        "rate limit",
        "rate_limit",
        "connection refused",
        "connection reset",
        "error sending request",
    ]
        .iter()
        .any(|p| e.contains(p))
}

/// Nothing runnable left: wait for manual work, fail, refine, or evaluate and
/// finish. Returns `true` when new items were added and the loop should go on.
async fn finalize(state: &SharedState, id: Uuid) -> bool {
    let Ok(Some(o)) = state.storage.get_objective(id).await else {
        return false;
    };
    if o.status != ObjectiveStatus::Running {
        return false;
    }
    let ru = is_russian(objective_locale(&o));
    let leaves: Vec<&ObjectiveItem> = o.items.iter().filter(|i| o.is_leaf(&i.id)).collect();
    let all_settled = leaves.iter().all(|i| i.status.is_settled());
    let manual_waiting: Vec<String> = o
        .ready_items()
        .into_iter()
        .filter_map(|i| o.item(&i))
        .filter(|i| i.kind == ObjectiveItemKind::Manual)
        .map(|i| i.title.clone())
        .collect();
    let failed = leaves.iter().filter(|i| i.status == ObjectiveItemStatus::Failed).count();
    let task_waiting: Vec<String> = leaves
        .iter()
        .filter(|i| i.kind == ObjectiveItemKind::Task && i.status == ObjectiveItemStatus::Running)
        .map(|i| i.title.clone())
        .collect();

    if all_settled && !leaves.is_empty() {
        let evaluation = evaluate(state, &o).await;
        if let Ok((false, summary)) = &evaluation
            && o.refine_rounds < o.auto_refine
            && auto_refine(state, id, summary.clone()).await
        {
            return true;
        }
        let _ = mutate(state, id, |o| {
            let ru = is_russian(objective_locale(o));
            match evaluation {
                Ok((achieved, summary)) => {
                    o.achieved = Some(achieved);
                    o.summary = Some(summary);
                }
                Err(err) => {
                    o.achieved = None;
                    o.summary = Some(fallback_summary(o));
                    o.push_log(
                        "warning",
                        None,
                        format!("{}: {err}", if ru { "ИИ-оценка недоступна" } else { "AI evaluation unavailable" }),
                    );
                }
            }
            o.status = ObjectiveStatus::Done;
            o.finished_at = Some(Utc::now());
            let msg = match o.achieved {
                Some(false) => tr(o, "■ Все пункты выполнены, но цель не достигнута — см. итог", "■ All items done, but the objective is not met — see summary"),
                _ => tr(o, "★ Цель выполнена", "★ Objective completed"),
            };
            o.push_log("success", None, msg);
            Ok(())
        })
        .await;
        return false;
    }

    let _ = mutate(state, id, |o| {
        if !manual_waiting.is_empty() || !task_waiting.is_empty() {
            o.status = ObjectiveStatus::Paused;
            if !manual_waiting.is_empty() {
                o.push_log(
                    "warning",
                    None,
                    format!(
                        "{}: {}",
                        if ru { "Ждёт ручных пунктов" } else { "Waiting for manual items" },
                        manual_waiting.join(", ")
                    ),
                );
            }
            if !task_waiting.is_empty() {
                o.push_log(
                    "warning",
                    None,
                    format!(
                        "{}: {}",
                        if ru { "Ждём задачи ProjectHub" } else { "Waiting for ProjectHub tasks" },
                        task_waiting.join(", ")
                    ),
                );
            }
        } else if failed > 0 {
            o.status = ObjectiveStatus::Failed;
            o.finished_at = Some(Utc::now());
            o.push_log(
                "error",
                None,
                if ru {
                    format!("Остановлено: пунктов с ошибкой — {failed}. Исправьте и нажмите «Повторить»")
                } else {
                    format!("Stopped: {failed} item(s) failed. Fix them and press Retry")
                },
            );
        } else {
            // Single-item run finished, or pending items wait on unfinished ones.
            o.status = ObjectiveStatus::Ready;
            let ready = o.ready_items();
            let blocked: Vec<String> = o
                .items
                .iter()
                .filter(|i| o.is_leaf(&i.id) && i.status == ObjectiveItemStatus::Pending)
                .filter(|i| !ready.contains(&i.id))
                .map(|i| i.title.clone())
                .collect();
            if blocked.is_empty() {
                let msg = tr(o, "Готово к продолжению", "Ready to continue");
                o.push_log("info", None, msg);
            } else {
                o.push_log(
                    "warning",
                    None,
                    format!(
                        "{}: {}",
                        if ru { "Ждут незавершённых пунктов" } else { "Waiting on unfinished items" },
                        blocked.join(", ")
                    ),
                );
            }
        }
        Ok(())
    })
    .await;
    false
}

/// One automatic refinement round: keep the evaluation, let the planner append
/// gap-closing items. `true` when items were added.
async fn auto_refine(state: &SharedState, id: Uuid, summary: String) -> bool {
    let Ok(o) = mutate(state, id, |o| {
        o.summary = Some(summary);
        o.achieved = Some(false);
        o.refine_rounds += 1;
        let msg = format!(
            "{} {}/{}",
            tr(o, "✦ Цель не достигнута — доработка, раунд", "✦ Not achieved — refining, round"),
            o.refine_rounds,
            o.auto_refine
        );
        o.push_log("info", None, msg);
        Ok(())
    })
    .await
    else {
        return false;
    };
    let req = ObjectiveAssistRequest {
        mode: ObjectiveAssistMode::Refine,
        ..Default::default()
    };
    let project = project_context(&o).await;
    let drafted = match llm::resolve_for(state, &o, TaskKind::Planning).await {
        Ok(llm) => super::assist::draft(&llm, &o, &req, project.as_deref()).await,
        Err(err) => Err(err),
    };
    let draft = match drafted {
        Ok(d) if !d.items.is_empty() => d,
        Ok(_) => return false,
        Err(err) => {
            let _ = mutate(state, id, |o| {
                let msg = format!("{}: {err}", tr(o, "Доработка не удалась", "Refinement failed"));
                o.push_log("warning", None, msg);
                Ok(())
            })
            .await;
            return false;
        }
    };
    let added = draft.items.len();
    mutate(state, id, |o| {
        super::assist::apply(o, &draft, &req);
        let msg = format!("{}: {added}", tr(o, "Добавлено пунктов", "Items added"));
        o.push_log("info", None, msg);
        Ok(())
    })
    .await
    .is_ok()
}

async fn evaluate(state: &SharedState, o: &Objective) -> Result<(bool, String), String> {
    let llm = llm::resolve_for(state, o, TaskKind::Reasoning).await?;
    let ru = is_russian(objective_locale(o));
    let mut results = String::new();
    for id in o.ordered_ids() {
        let Some(item) = o.item(&id) else { continue };
        if !o.is_leaf(&id) {
            continue;
        }
        let body = match item.status {
            ObjectiveItemStatus::Skipped => "(skipped)".to_string(),
            _ => scenario::truncate(item.result.as_deref().unwrap_or("—"), scenario::ITEM_RESULT_LIMIT),
        };
        results.push_str(&format!(
            "### {} {}\n{}\n\n",
            scenario::item_path(o, &id),
            item.title,
            body
        ));
    }
    let system = format!(
        "You review whether an objective was achieved from its item results. {} \
         Return ONLY JSON: {{\"achieved\": true|false, \"summary\": \"markdown: what was produced, \
         which success criteria are met or not (one line each), key numbers\", \"next_steps\": [\"…\"]}}",
        if ru { "Пиши на русском." } else { "Write in English." }
    );
    let user = format!("{}\n\n## Item results\n{}", scenario::objective_brief(o), results);
    let raw = llm.chat(&system, &user, 0.2, 16_000, 300_000).await?;
    let v = extract_json_object(&raw).ok_or_else(|| "evaluation is not JSON".to_string())?;
    let achieved = v.get("achieved").and_then(Value::as_bool).unwrap_or(true);
    let mut summary = v
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if let Some(steps) = v.get("next_steps").and_then(Value::as_array) {
        let steps: Vec<&str> = steps.iter().filter_map(Value::as_str).collect();
        if !steps.is_empty() {
            summary.push_str(if ru { "\n\n**Дальше:**\n" } else { "\n\n**Next steps:**\n" });
            for s in steps {
                summary.push_str(&format!("- {s}\n"));
            }
        }
    }
    if summary.is_empty() {
        summary = fallback_summary(o);
    }
    Ok((achieved, summary))
}

fn fallback_summary(o: &Objective) -> String {
    o.ordered_ids()
        .iter()
        .filter(|id| o.is_leaf(id))
        .filter_map(|id| o.item(id))
        .map(|i| format!("- {} — {:?}", i.title, i.status).to_lowercase())
        .collect::<Vec<_>>()
        .join("\n")
}
