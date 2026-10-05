//! Secrets abstraction — swappable backends without changing nodes.

mod store;

pub use store::{EncryptedSqliteSecretStore, redact_value};

use async_trait::async_trait;

#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn set(&self, key: &str, value: &[u8]) -> anyhow::Result<()>;
    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>>;
    async fn delete(&self, key: &str) -> anyhow::Result<()>;
}
