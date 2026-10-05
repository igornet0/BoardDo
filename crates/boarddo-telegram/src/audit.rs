use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use tglib::{
    TelegramAccountId, TelegramAuditEvent, TelegramAuditStatus,
};

use crate::store::TelegramSessionStore;

pub struct TelegramAuditService {
    store: Arc<TelegramSessionStore>,
}

impl TelegramAuditService {
    pub fn new(store: Arc<TelegramSessionStore>) -> Self {
        Self { store }
    }

    pub async fn record(
        &self,
        account_id: TelegramAccountId,
        scenario_id: Option<Uuid>,
        execution_id: Option<Uuid>,
        event_type: impl Into<String>,
        action: impl Into<String>,
        target: Option<String>,
        status: TelegramAuditStatus,
        error: Option<String>,
    ) -> anyhow::Result<TelegramAuditEvent> {
        let event = TelegramAuditEvent {
            id: Uuid::now_v7(),
            account_id,
            scenario_id,
            execution_id,
            event_type: event_type.into(),
            action: action.into(),
            target,
            status,
            error,
            created_at: Utc::now(),
        };
        self.store.insert_audit(&event).await?;
        Ok(event)
    }

    pub async fn list(
        &self,
        account_id: Option<TelegramAccountId>,
        limit: i64,
    ) -> anyhow::Result<Vec<TelegramAuditEvent>> {
        self.store.list_audit(account_id, limit).await
    }
}
