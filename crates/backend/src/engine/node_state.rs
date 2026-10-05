//! Persistent key/value store for node-level runtime state (e.g. last seen release).

use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;

use crate::storage::Storage;

#[async_trait]
pub trait NodeStateStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, String>;
    async fn set(&self, key: &str, value: &str) -> Result<(), String>;
}

/// No persistence — used in unit tests / engine without storage.
pub struct NullNodeStateStore;

#[async_trait]
impl NodeStateStore for NullNodeStateStore {
    async fn get(&self, _key: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    async fn set(&self, _key: &str, _value: &str) -> Result<(), String> {
        Ok(())
    }
}

/// In-memory store (tests / single-process demos).
pub struct MemoryNodeStateStore {
    inner: DashMap<String, String>,
}

impl MemoryNodeStateStore {
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }
}

impl Default for MemoryNodeStateStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NodeStateStore for MemoryNodeStateStore {
    async fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.inner.get(key).map(|v| v.clone()))
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.inner.insert(key.to_string(), value.to_string());
        Ok(())
    }
}

/// SQLite-backed store (`node_state` table).
pub struct SqliteNodeStateStore {
    storage: Storage,
}

impl SqliteNodeStateStore {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl NodeStateStore for SqliteNodeStateStore {
    async fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.storage
            .get_node_state(key)
            .await
            .map_err(|e| e.to_string())
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let now = Utc::now();
        self.storage
            .set_node_state(key, value, now)
            .await
            .map_err(|e| e.to_string())
    }
}
