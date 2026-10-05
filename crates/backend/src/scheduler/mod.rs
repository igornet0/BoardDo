//! Background scheduler: polls due rows and fires schedule triggers
//! through the Runtime Supervisor.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use tracing::{debug, error, info, warn};

use crate::runtime::{RuntimeEvent, RuntimeEventSource};
use crate::state::SharedState;
use crate::triggers::schedule::next_run_after;

const TICK: Duration = Duration::from_secs(5);

pub fn spawn(state: SharedState) {
    tokio::spawn(async move {
        info!("scheduler started (tick={}s)", TICK.as_secs());
        loop {
            if let Err(err) = tick(&state).await {
                error!(error = %err, "scheduler tick failed");
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn tick(state: &SharedState) -> anyhow::Result<()> {
    let now = Utc::now();
    fire_queued(state).await?;
    fire_after_completion(state, now).await?;

    let due = state.storage.list_due_schedules(now).await?;
    if due.is_empty() {
        return Ok(());
    }

    debug!(count = due.len(), "scheduler due schedules");

    for schedule in due {
        let next = match next_run_after(&schedule.cron, &schedule.timezone, now) {
            Ok(t) => t,
            Err(err) => {
                warn!(
                    schedule_id = %schedule.id,
                    error = %err,
                    "failed to compute next_run_at"
                );
                continue;
            }
        };

        let claimed = state.storage.claim_schedule(schedule.id, now, next).await?;
        if !claimed {
            continue;
        }

        if !state.runtime.is_accepting(schedule.workflow_id) {
            debug!(
                workflow_id = %schedule.workflow_id,
                "schedule skipped — runtime not armed"
            );
            continue;
        }

        let wf = match state.storage.get_workflow(schedule.workflow_id).await? {
            Some(w) => w,
            None => {
                warn!(workflow_id = %schedule.workflow_id, "schedule references missing workflow");
                let _ = state.storage.set_schedule_enabled(schedule.id, false).await;
                continue;
            }
        };

        match state
            .runtime
            .dispatch(
                state,
                RuntimeEvent {
                    workflow_id: wf.id,
                    source: RuntimeEventSource::Schedule,
                    payload: json!({
                        "scheduled_at": now.to_rfc3339(),
                        "node_id": schedule.node_id,
                    }),
                    schedule_node_id: Some(schedule.node_id.clone()),
                },
            )
            .await
        {
            Ok(Some(res)) => {
                info!(
                    workflow_id = %wf.id,
                    execution_id = %res.execution_id,
                    node_id = %schedule.node_id,
                    "schedule fired"
                );
            }
            Ok(None) => {
                debug!(workflow_id = %wf.id, "schedule dispatch skipped");
            }
            Err(err) => {
                error!(
                    workflow_id = %wf.id,
                    error = %err,
                    "schedule fire failed"
                );
            }
        }
    }

    Ok(())
}

async fn fire_queued(state: &SharedState) -> anyhow::Result<()> {
    for event in state.runtime.take_queued() {
        if let Err(err) = state.runtime.dispatch(state, event).await {
            warn!(error = %err, "runtime.queued_dispatch_failed");
        }
    }
    Ok(())
}

async fn fire_after_completion(
    state: &SharedState,
    now: chrono::DateTime<Utc>,
) -> anyhow::Result<()> {
    let due = state.runtime.due_after_completion(now);
    for (workflow_id, node_id) in due {
        match state
            .runtime
            .dispatch(
                state,
                RuntimeEvent {
                    workflow_id,
                    source: RuntimeEventSource::Schedule,
                    payload: json!({
                        "scheduled_at": now.to_rfc3339(),
                        "node_id": node_id,
                        "kind": "after_completion",
                    }),
                    schedule_node_id: Some(node_id.clone()),
                },
            )
            .await
        {
            Ok(Some(res)) => {
                info!(
                    %workflow_id,
                    execution_id = %res.execution_id,
                    node_id,
                    "after_completion fired"
                );
            }
            Ok(None) => {}
            Err(err) => {
                warn!(%workflow_id, error = %err, "after_completion fire failed");
            }
        }
    }
    Ok(())
}

/// Helper so `main` can pass Arc without importing internals elsewhere.
pub fn spawn_arc(state: Arc<crate::state::AppState>) {
    spawn(state);
}
