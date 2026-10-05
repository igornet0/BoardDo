use boarddo_shared::v2::{ContentItem, ContentLifecycle, GoalRun, GoalSpec};
use uuid::Uuid;

use crate::state::SharedState;

use super::adapters::{adapter_for, PublishContext};

pub async fn prepare_publication(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    content_id: Uuid,
    account_id: Option<String>,
    chat_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let item = state
        .storage
        .get_content_item(content_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "content not found".to_string())?;
    let key = publish_idempotency_key(spec.id, run.id, item.id);
    let ctx = PublishContext {
        state,
        spec,
        run,
        item: &item,
        account_id,
        chat_id,
        idempotency_key: key,
    };
    let adapter = adapter_for(item.channel);
    let prep = adapter.prepare(&ctx).await?;
    let mut updated = item;
    updated.lifecycle = ContentLifecycle::Review;
    updated.updated_at = chrono::Utc::now();
    state
        .storage
        .update_content_item(&updated)
        .await
        .map_err(|e| e.to_string())?;
    content_event(state, run.id, "content.reviewed", &updated.id).await;
    Ok(serde_json::json!({
        "ok": true,
        "prepare": prep,
        "content": updated
    }))
}

pub async fn publish_content(
    state: &SharedState,
    spec: &GoalSpec,
    run: &GoalRun,
    content_id: Uuid,
    account_id: Option<String>,
    chat_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let item = state
        .storage
        .get_content_item(content_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "content not found".to_string())?;
    if item.lifecycle == ContentLifecycle::Published {
        if let Some(pub_rec) = state
            .storage
            .latest_publication_for_content(item.id)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(serde_json::json!({
                "ok": true,
                "idempotent": true,
                "publication": pub_rec,
                "content": item
            }));
        }
    }
    let key = publish_idempotency_key(spec.id, run.id, item.id);
    let ctx = PublishContext {
        state,
        spec,
        run,
        item: &item,
        account_id,
        chat_id,
        idempotency_key: key,
    };
    let adapter = adapter_for(item.channel);
    let publication = adapter.publish(&ctx).await?;
    let mut updated = item;
    updated.lifecycle = ContentLifecycle::Published;
    updated.updated_at = chrono::Utc::now();
    state
        .storage
        .update_content_item(&updated)
        .await
        .map_err(|e| e.to_string())?;
    if let Ok(Some(mut draft)) = state.storage.get_content_draft(updated.id).await {
        draft.status = boarddo_shared::v2::ContentDraftStatus::Published;
        draft.updated_at = chrono::Utc::now();
        if publication.simulated {
            draft.simulated = true;
            draft.telegram_message_id = publication.external_id.clone();
        }
        let _ = state.storage.update_content_draft(&draft).await;
    }
    content_event(state, run.id, "content.published", &updated.id).await;
    Ok(serde_json::json!({
        "ok": true,
        "publication": publication,
        "content": updated,
        "simulated": publication.simulated
    }))
}

pub fn publish_idempotency_key(goal_id: Uuid, run_id: Uuid, content_id: Uuid) -> String {
    format!("{goal_id}:{run_id}:{content_id}:publish")
}

async fn content_event(state: &SharedState, run_id: Uuid, kind: &str, content_id: &Uuid) {
    let _ = state
        .storage
        .insert_audit(&boarddo_shared::v2::AgentAuditEntry {
            id: Uuid::now_v7(),
            run_id,
            at: chrono::Utc::now(),
            kind: kind.into(),
            message: Some(format!("content {content_id}")),
            payload: Some(serde_json::json!({ "content_id": content_id })),
        })
        .await;
}
