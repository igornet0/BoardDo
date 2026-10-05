//! Poll due GoalRuns and fire agent ticks (parallel to the workflow scheduler).

use std::sync::Arc;
use std::time::Duration;

use tracing::{error, info, warn};

use crate::state::SharedState;

const POLL: Duration = Duration::from_secs(5);

pub fn spawn(state: SharedState) {
    tokio::spawn(async move {
        info!("agent runtime started (poll={}s)", POLL.as_secs());
        loop {
            if let Err(err) = tick_due(&state).await {
                error!(error = %err, "agent runtime tick failed");
            }
            tokio::time::sleep(POLL).await;
        }
    });
}

pub fn spawn_arc(state: Arc<crate::state::AppState>) {
    spawn(state);
}

async fn tick_due(state: &SharedState) -> anyhow::Result<()> {
    let due = state.storage.list_due_goal_runs(chrono::Utc::now()).await?;
    for run in due {
        let Some(spec) = state.storage.get_goal_spec(run.goal_id).await? else {
            warn!(run_id = %run.id, "goal spec missing for run");
            continue;
        };
        if let Err(err) = super::tick::run_tick(state, spec, run).await {
            error!(error = %err, "goal tick failed");
        }
    }
    Ok(())
}
