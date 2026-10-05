//! Keep TDLib clients aligned with armed scenario demand.
//!
//! Boot no longer restores every `desired_running` account — that made inbound
//! traffic flow with zero armed scenarios. Clients are started when an armed
//! Telegram trigger needs them (or via explicit Connect for auth/workspace),
//! and paused again when no armed runtime needs them.

use std::collections::HashSet;

use tglib::TelegramAccountId;
use uuid::Uuid;

use crate::runtime::TelegramAccountDemand;
use crate::state::SharedState;

pub async fn sync_telegram_accounts(state: &SharedState) {
    let demand = state.runtime.telegram_account_demand();
    if let Err(err) = apply_demand(state, demand).await {
        tracing::error!(error = %err, "telegram.demand.sync_failed");
    }
}

async fn apply_demand(state: &SharedState, demand: TelegramAccountDemand) -> anyhow::Result<()> {
    let accounts = state.telegram.list_accounts().await?;
    let needed: HashSet<Uuid> = match &demand {
        TelegramAccountDemand::None => HashSet::new(),
        TelegramAccountDemand::Any => accounts.iter().map(|a| a.id.0).collect(),
        TelegramAccountDemand::Specific(ids) => ids.clone(),
    };

    for id in &needed {
        if let Err(err) = state.telegram.ensure_running(TelegramAccountId(*id)).await {
            tracing::warn!(account_id = %id, error = %err, "telegram.demand.ensure_failed");
        }
    }

    for account in &accounts {
        let id = account.id.0;
        if needed.contains(&id) {
            continue;
        }
        if !state.telegram.is_running(account.id) {
            continue;
        }
        // Pause even if desired_running: listening follows armed runtimes.
        // Connect still starts a client for auth/workspace until the next
        // demand sync (or process exit).
        if let Err(err) = state.telegram.pause_account(account.id).await {
            tracing::warn!(account_id = %id, error = %err, "telegram.demand.pause_failed");
        } else {
            tracing::info!(account_id = %id, "telegram.demand.paused");
        }
    }

    match &demand {
        TelegramAccountDemand::None => {
            tracing::debug!("telegram.demand.synced needed=0");
        }
        TelegramAccountDemand::Any => {
            tracing::info!(count = needed.len(), "telegram.demand.synced needed=all");
        }
        TelegramAccountDemand::Specific(_) => {
            tracing::info!(count = needed.len(), "telegram.demand.synced");
        }
    }
    Ok(())
}
