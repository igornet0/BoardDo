//! Call journal seen by the engine: single-execution nodes (sends,
//! irreversible actions) are claimed before they run, so a repeat of the same
//! node in the same execution never acts twice.

use serde_json::Value;
use uuid::Uuid;

use crate::storage::{JournalClaim, Storage};

#[async_trait::async_trait]
pub trait CallJournal: Send + Sync {
    async fn claim(&self, key: &str, run_id: Uuid, type_id: &str) -> anyhow::Result<JournalClaim>;
    async fn finish(
        &self,
        key: &str,
        outcome: &Result<Value, String>,
        retry_safe: bool,
    ) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
impl CallJournal for Storage {
    async fn claim(&self, key: &str, run_id: Uuid, type_id: &str) -> anyhow::Result<JournalClaim> {
        self.tool_journal_claim(key, run_id, type_id).await
    }

    async fn finish(
        &self,
        key: &str,
        outcome: &Result<Value, String>,
        retry_safe: bool,
    ) -> anyhow::Result<()> {
        self.tool_journal_finish(key, outcome, retry_safe).await
    }
}
