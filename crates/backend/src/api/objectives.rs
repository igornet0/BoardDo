//! Objectives API — goals split into items executed through scenarios.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use boarddo_shared::v2::{
    CreateObjectiveRequest, Objective, ObjectiveAssistMode, ObjectiveAssistRequest,
    ObjectiveAssistResponse,
    ObjectiveItemKind, ObjectiveItemStatus, ObjectiveStatus, RunObjectiveRequest,
    UpdateObjectiveItemRequest,
};
use chrono::Utc;
use uuid::Uuid;

use crate::objectives::{self, assist, llm, runner, tr};
use crate::state::SharedState;

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/objectives", get(list).post(create))
        .route("/objectives/{id}", get(get_one).put(update).delete(remove))
        .route("/objectives/{id}/assist", post(assist_objective))
        .route("/objectives/{id}/run", post(run))
        .route("/objectives/{id}/stop", post(stop))
        .route("/objectives/{id}/items/{item_id}", patch(update_item))
        .route("/objectives/{id}/items/{item_id}/scenario", post(item_scenario))
}

fn bad(err: String) -> (StatusCode, String) {
    if err.contains("not found") {
        (StatusCode::NOT_FOUND, err)
    } else {
        (StatusCode::BAD_REQUEST, err)
    }
}

async fn load(state: &SharedState, id: Uuid) -> Result<Objective, (StatusCode, String)> {
    state
        .storage
        .get_objective(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "objective not found".into()))
}

async fn list(State(state): State<SharedState>) -> ApiResult<Vec<Objective>> {
    state
        .storage
        .list_objectives()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn create(
    State(state): State<SharedState>,
    Json(req): Json<CreateObjectiveRequest>,
) -> Result<(StatusCode, Json<Objective>), (StatusCode, String)> {
    let o = objectives::create(&state, req).await.map_err(bad)?;
    Ok((StatusCode::CREATED, Json(o)))
}

async fn get_one(State(state): State<SharedState>, Path(id): Path<Uuid>) -> ApiResult<Objective> {
    load(&state, id).await.map(Json)
}

async fn update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateObjectiveRequest>,
) -> ApiResult<Objective> {
    objectives::update(&state, id, req).await.map(Json).map_err(bad)
}

async fn remove(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    match objectives::delete(&state, id).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err((StatusCode::NOT_FOUND, "objective not found".into())),
        Err(err) => Err((StatusCode::CONFLICT, err)),
    }
}

async fn assist_objective(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<ObjectiveAssistRequest>,
) -> ApiResult<ObjectiveAssistResponse> {
    let current = load(&state, id).await?;
    let model = llm::resolve(&state, &current).await.map_err(bad)?;
    let project = runner::project_context(&current).await;
    let draft = assist::draft(&model, &current, &req, project.as_deref())
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    if !req.apply {
        let mut preview = current.clone();
        assist::apply(&mut preview, &draft, &req);
        return Ok(Json(ObjectiveAssistResponse {
            draft,
            objective: preview,
            applied: false,
            model: model.model,
        }));
    }

    let used_model = model.model.clone();
    let objective = objectives::mutate(&state, id, |o| {
        if o.status == ObjectiveStatus::Running && req.item_id.is_none() {
            return Err(tr(o, "Нельзя перестраивать цель во время выполнения", "Cannot restructure a running objective"));
        }
        let note = assist::apply(o, &draft, &req);
        if req.mode == ObjectiveAssistMode::Refine
            && matches!(o.status, ObjectiveStatus::Done | ObjectiveStatus::Failed)
        {
            // New work to do — the run button comes back.
            o.status = ObjectiveStatus::Ready;
            o.finished_at = None;
        }
        let mut msg = match req.item_id.as_deref().and_then(|i| o.item(i)) {
            Some(item) => format!("{} «{}» ({used_model})", tr(o, "✦ ИИ разбил пункт", "✦ AI split item"), item.title),
            None if req.mode == ObjectiveAssistMode::Refine => {
                format!("{} ({used_model})", tr(o, "✦ ИИ добавил пункты доработки", "✦ AI added refinement items"))
            }
            None => format!("{} ({used_model})", tr(o, "✦ ИИ заполнил цель", "✦ AI filled the objective")),
        };
        if let Some(note) = note {
            msg.push_str(&format!(" — {note}"));
        }
        o.push_log("info", req.item_id.as_deref(), msg);
        Ok(())
    })
    .await
    .map_err(bad)?;

    Ok(Json(ObjectiveAssistResponse {
        draft,
        objective,
        applied: true,
        model: model.model,
    }))
}

async fn run(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    body: Option<Json<RunObjectiveRequest>>,
) -> ApiResult<Objective> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    runner::start(&state, id, req).await.map(Json).map_err(bad)
}

async fn stop(State(state): State<SharedState>, Path(id): Path<Uuid>) -> ApiResult<Objective> {
    runner::stop(&state, id).await.map(Json).map_err(bad)
}

async fn update_item(
    State(state): State<SharedState>,
    Path((id, item_id)): Path<(Uuid, String)>,
    Json(req): Json<UpdateObjectiveItemRequest>,
) -> ApiResult<Objective> {
    if let Some(wid) = req.workflow_id {
        let exists = state
            .storage
            .get_workflow(wid)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .is_some();
        if !exists {
            return Err((StatusCode::BAD_REQUEST, "workflow not found".into()));
        }
    }
    let mut updated = objectives::mutate(&state, id, |o| {
        if !o.is_leaf(&item_id) && req.status.is_some() {
            return Err(tr(o, "Статус пункта с подпунктами считается автоматически", "Status of an item with sub-items is derived"));
        }
        let item = o
            .item_mut(&item_id)
            .ok_or_else(|| format!("item `{item_id}` not found"))?;
        // A `task` item "runs" in ProjectHub, so it may be ticked off here.
        if item.status == ObjectiveItemStatus::Running
            && req.status.is_some()
            && item.kind != ObjectiveItemKind::Task
        {
            return Err("item is running".into());
        }
        if req.unlink_workflow {
            item.workflow_id = None;
            item.scenario_hash = None;
            if item.kind == ObjectiveItemKind::Workflow {
                item.kind = ObjectiveItemKind::Ai;
            }
        }
        if let Some(wid) = req.workflow_id {
            item.workflow_id = Some(wid);
            item.scenario_hash = None;
            item.kind = ObjectiveItemKind::Workflow;
        }
        if let Some(result) = req.result.clone() {
            item.result = Some(result).filter(|r| !r.trim().is_empty());
        }
        let title = item.title.clone();
        if let Some(status) = req.status {
            match status {
                ObjectiveItemStatus::Pending => item.reset(),
                ObjectiveItemStatus::Running => return Err("use /run to execute an item".into()),
                other => {
                    item.status = other;
                    item.error = None;
                    item.finished_at = Some(Utc::now());
                }
            }
            let label = match status {
                ObjectiveItemStatus::Done => tr(o, "отмечен выполненным", "marked done"),
                ObjectiveItemStatus::Skipped => tr(o, "пропущен", "skipped"),
                ObjectiveItemStatus::Failed => tr(o, "отмечен как ошибка", "marked failed"),
                _ => tr(o, "сброшен", "reset"),
            };
            o.push_log("info", Some(&item_id), format!("«{title}» {label}"));
        }
        Ok(())
    })
    .await
    .map_err(bad)?;

    // A `task` item ticked off in BoardDo closes its ProjectHub task too.
    if let (Some(status), Some(item)) = (req.status, updated.item(&item_id)) {
        if item.kind == ObjectiveItemKind::Task
            && let Some(task_id) = item.external_task_id.clone()
        {
            let ph_status = match status {
                ObjectiveItemStatus::Done => Some("done"),
                ObjectiveItemStatus::Skipped => Some("cancelled"),
                ObjectiveItemStatus::Pending => Some("todo"),
                _ => None,
            };
            if let Some(ph_status) = ph_status {
                let pushed = match crate::projecthub::client().await {
                    Ok(client) => client
                        .patch(&format!("tasks/{task_id}"), serde_json::json!({ "status": ph_status }))
                        .await
                        .map(|_| ()),
                    Err(err) => Err(err),
                };
                // Mirror the ProjectHub side on the item, or say why it was not updated.
                let item_id = item_id.clone();
                let result = objectives::mutate(&state, id, move |o| {
                    match &pushed {
                        Ok(()) => {
                            if let Some(item) = o.item_mut(&item_id) {
                                item.external_status = Some(ph_status.to_string());
                            }
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "projecthub.task_status_push_failed");
                            let msg = format!(
                                "{}: {err}",
                                tr(o, "Статус задачи в ProjectHub не обновлён", "ProjectHub task status was not updated")
                            );
                            o.push_log("warning", Some(&item_id), msg);
                        }
                    }
                    Ok(())
                })
                .await;
                if let Ok(o) = result {
                    updated = o;
                }
            }
        }
    }

    // Manual work finished while the run waited for it → continue automatically.
    if updated.status == ObjectiveStatus::Paused
        && req.status.is_some_and(|s| s.is_settled())
        && updated.log.iter().rev().take(3).any(|l| {
            l.message.starts_with("Ждёт ручных")
                || l.message.starts_with("Waiting for manual")
                || l.message.starts_with("Ждём задачи ProjectHub")
                || l.message.starts_with("Waiting for ProjectHub tasks")
        })
        && let Ok(o) = runner::start(&state, id, RunObjectiveRequest::default()).await
    {
        return Ok(Json(o));
    }
    Ok(Json(updated))
}

/// Create / refresh the item's scenario without running it (to open on canvas).
async fn item_scenario(
    State(state): State<SharedState>,
    Path((id, item_id)): Path<(Uuid, String)>,
) -> ApiResult<Objective> {
    let o = load(&state, id).await?;
    let item = o
        .item(&item_id)
        .cloned()
        .ok_or((StatusCode::NOT_FOUND, format!("item `{item_id}` not found")))?;
    if !o.is_leaf(&item_id) {
        return Err((StatusCode::BAD_REQUEST, tr(&o, "У пункта есть подпункты", "Item has sub-items")));
    }
    runner::ensure_scenario(&state, &o, &item).await.map_err(bad)?;
    load(&state, id).await.map(Json)
}
