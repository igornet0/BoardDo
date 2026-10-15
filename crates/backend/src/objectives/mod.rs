//! Objectives: goal → items (tree) → scenarios → results → evaluation.
//!
//! The REST layer lives in `api/objectives.rs`; this module owns the rules.

pub mod assist;
pub mod llm;
pub mod runner;
pub mod scenario;

#[cfg(test)]
pub(crate) mod tests;

use std::sync::LazyLock;

use boarddo_shared::v2::{
    CreateObjectiveRequest, GoalRuntimeEvent, OBJECTIVE_MAX_REFINE, Objective, ObjectiveItem,
    ObjectiveItemStatus, ObjectiveStatus,
};
use chrono::Utc;
use uuid::Uuid;

use crate::state::SharedState;

/// Serialises read-modify-write of objective documents (API edits vs run loop).
static EDIT_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Load → change → recompute → save → notify. `f` may veto with an error.
pub async fn mutate<F>(state: &SharedState, id: Uuid, f: F) -> Result<Objective, String>
where
    F: FnOnce(&mut Objective) -> Result<(), String>,
{
    let _guard = EDIT_LOCK.lock().await;
    let mut o = state
        .storage
        .get_objective(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "objective not found".to_string())?;
    f(&mut o)?;
    o.recompute();
    settle_draft_status(&mut o);
    o.updated_at = Utc::now();
    state.storage.save_objective(&o).await.map_err(|e| e.to_string())?;
    notify(state, &o, None);
    Ok(o)
}

pub fn notify(state: &SharedState, o: &Objective, item_id: Option<String>) {
    state.emit_goal(GoalRuntimeEvent::ObjectiveUpdated {
        objective_id: o.id,
        status: o.status,
        percent: o.progress.percent,
        item_id,
    });
}

/// Russian or English message, following the objective's language.
pub fn tr(o: &Objective, ru: &str, en: &str) -> String {
    if scenario::is_russian(scenario::objective_locale(o)) {
        ru.to_string()
    } else {
        en.to_string()
    }
}

/// Draft ↔ Ready follows whether there is anything to run.
fn settle_draft_status(o: &mut Objective) {
    match o.status {
        ObjectiveStatus::Draft if !o.items.is_empty() => o.status = ObjectiveStatus::Ready,
        ObjectiveStatus::Ready if o.items.is_empty() => o.status = ObjectiveStatus::Draft,
        _ => {}
    }
}

pub async fn create(state: &SharedState, req: CreateObjectiveRequest) -> Result<Objective, String> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err("title is required".into());
    }
    let mut o = Objective::new(title);
    apply_definition(&mut o, req);
    if o.connection_id.is_none() {
        o.connection_id = llm::default_connection(state).await.ok().flatten();
    }
    let msg = tr(&o, "Цель создана", "Objective created");
    o.push_log("info", None, msg);
    o.normalize();
    settle_draft_status(&mut o);
    state.storage.save_objective(&o).await.map_err(|e| e.to_string())?;
    notify(state, &o, None);
    Ok(o)
}

pub async fn update(
    state: &SharedState,
    id: Uuid,
    req: CreateObjectiveRequest,
) -> Result<Objective, String> {
    if req.title.trim().is_empty() {
        return Err("title is required".into());
    }
    mutate(state, id, |o| {
        apply_definition(o, req);
        o.normalize();
        Ok(())
    })
    .await
}

/// Copy editable fields; execution state of surviving items comes from storage.
fn apply_definition(o: &mut Objective, req: CreateObjectiveRequest) {
    o.title = req.title.trim().to_string();
    o.description = req.description;
    o.characteristics = req.characteristics;
    o.success_criteria = req.success_criteria;
    o.connection_id = req.connection_id;
    o.model = req.model.filter(|m| !m.trim().is_empty());
    o.deadline = req.deadline;
    o.auto_refine = req.auto_refine.min(OBJECTIVE_MAX_REFINE);
    o.project_hub_project_id = req.project_hub_project_id.filter(|p| !p.trim().is_empty());
    o.project_hub_project_name = if o.project_hub_project_id.is_some() {
        req.project_hub_project_name
    } else {
        None
    };

    let previous = std::mem::take(&mut o.items);
    o.items = req
        .items
        .into_iter()
        .map(|incoming| merge_item(previous.iter().find(|p| p.id == incoming.id), incoming))
        .collect();
}

fn merge_item(previous: Option<&ObjectiveItem>, mut incoming: ObjectiveItem) -> ObjectiveItem {
    let Some(prev) = previous else {
        // New item: start clean whatever the client sent.
        let workflow_id = incoming.workflow_id;
        incoming.reset();
        incoming.attempts = 0;
        incoming.scenario_hash = None;
        incoming.external_task_id = None;
        incoming.external_status = None;
        incoming.external_url = None;
        incoming.workflow_id = workflow_id;
        return incoming;
    };
    let mut item = prev.clone();
    item.parent_id = incoming.parent_id;
    item.title = incoming.title;
    item.description = incoming.description;
    item.kind = incoming.kind;
    item.depends_on = incoming.depends_on;
    item.prompt = incoming.prompt;
    item.query = incoming.query;
    if incoming.workflow_id != prev.workflow_id {
        item.workflow_id = incoming.workflow_id;
        item.scenario_hash = None;
    }
    // Manual completion / skip from the editor is allowed; running state is not.
    let tracked_elsewhere = prev.kind == project_hub_task_kind();
    if incoming.status != prev.status
        && (prev.status != ObjectiveItemStatus::Running || tracked_elsewhere)
        && incoming.status != ObjectiveItemStatus::Running
    {
        item.status = incoming.status;
        if incoming.status == ObjectiveItemStatus::Pending {
            item.reset();
        }
    }
    if incoming.result.is_some() && incoming.result != prev.result {
        item.result = incoming.result;
    }
    item
}

fn project_hub_task_kind() -> boarddo_shared::v2::ObjectiveItemKind {
    boarddo_shared::v2::ObjectiveItemKind::Task
}

/// Runs cut short by a restart: put them back into a resumable state.
pub async fn recover(state: &SharedState) {
    let Ok(list) = state.storage.list_objectives().await else {
        return;
    };
    for o in list {
        let interrupted = o.status == ObjectiveStatus::Running
            || o
                .items
                .iter()
                .any(|i| i.status == ObjectiveItemStatus::Running && i.kind != project_hub_task_kind());
        if !interrupted {
            continue;
        }
        let _ = mutate(state, o.id, |o| {
            for item in o.items.iter_mut() {
                // Task items are still tracked in ProjectHub across restarts.
                if item.status == ObjectiveItemStatus::Running && item.kind != project_hub_task_kind() {
                    item.reset();
                }
            }
            if o.status == ObjectiveStatus::Running {
                o.status = ObjectiveStatus::Paused;
            }
            let msg = tr(o, "Выполнение прервано перезапуском сервера — нажмите «Продолжить»", "Run interrupted by a server restart — press Continue");
            o.push_log("warning", None, msg);
            Ok(())
        })
        .await;
    }
}

/// Generated (unedited) scenarios go away with their objective.
pub async fn delete(state: &SharedState, id: Uuid) -> Result<bool, String> {
    let Some(o) = state.storage.get_objective(id).await.map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    if runner::is_looping(id) && o.status == ObjectiveStatus::Running {
        return Err(tr(&o, "Сначала остановите цель", "Stop the objective first"));
    }
    for item in &o.items {
        let (Some(wid), Some(hash)) = (item.workflow_id, item.scenario_hash.as_deref()) else {
            continue;
        };
        if let Ok(Some(wf)) = state.storage.get_workflow(wid).await
            && scenario::definition_hash(&wf.definition.nodes, &wf.definition.edges) == hash
        {
            let _ = state.storage.delete_workflow(wid).await;
        }
    }
    state.storage.delete_objective(id).await.map_err(|e| e.to_string())
}
