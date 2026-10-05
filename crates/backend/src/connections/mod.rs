//! Connection registry + credential resolution for nodes.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use boarddo_shared::{Connection, CreateConnectionRequest, UpdateConnectionRequest};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::secrets::{SecretStore, redact_value};
use crate::storage::Storage;

/// In-memory credentials resolved for a single node execution (never logged as-is).
#[derive(Clone)]
pub struct ResolvedConnection {
    pub id: Uuid,
    pub name: String,
    pub connection_type: String,
    pub config: Value,
    pub credentials: Value,
}

impl std::fmt::Debug for ResolvedConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedConnection")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("connection_type", &self.connection_type)
            .field("config", &self.config)
            .field("credentials", &redact_value(&self.credentials))
            .finish()
    }
}

#[async_trait]
pub trait ConnectionProvider: Send + Sync {
    async fn resolve(&self, connection_id: &str) -> Result<ResolvedConnection, String>;
    async fn get_public(&self, connection_id: &str) -> Result<Option<Connection>, String>;
}

pub struct StorageConnectionProvider {
    storage: Storage,
    secrets: Arc<dyn SecretStore>,
}

impl StorageConnectionProvider {
    pub fn new(storage: Storage, secrets: Arc<dyn SecretStore>) -> Self {
        Self { storage, secrets }
    }

    pub fn secret_ref_for(id: Uuid) -> String {
        format!("connection:{id}")
    }

    pub async fn create(&self, req: CreateConnectionRequest) -> anyhow::Result<Connection> {
        let id = Uuid::now_v7();
        let now = Utc::now();
        let secret_ref = if req.secret.is_some() {
            Some(Self::secret_ref_for(id))
        } else {
            None
        };

        if let Some(secret) = &req.secret {
            let bytes = serde_json::to_vec(secret)?;
            self.secrets
                .set(secret_ref.as_ref().unwrap(), &bytes)
                .await?;
        }

        self.storage
            .insert_connection(
                id,
                &req.name,
                &req.connection_type,
                &req.config,
                secret_ref.as_deref(),
                req.enabled,
                now,
            )
            .await?;

        Ok(Connection {
            id,
            name: req.name,
            connection_type: req.connection_type,
            config: req.config,
            enabled: req.enabled,
            has_secret: secret_ref.is_some(),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn update(
        &self,
        id: Uuid,
        req: UpdateConnectionRequest,
    ) -> anyhow::Result<Option<Connection>> {
        let existing = match self.storage.get_connection(id).await? {
            Some(c) => c,
            None => return Ok(None),
        };

        let now = Utc::now();
        let mut secret_ref = if existing.has_secret {
            Some(Self::secret_ref_for(id))
        } else {
            None
        };

        if let Some(secret) = &req.secret {
            let key = Self::secret_ref_for(id);
            let bytes = serde_json::to_vec(secret)?;
            self.secrets.set(&key, &bytes).await?;
            secret_ref = Some(key);
        }

        self.storage
            .update_connection(
                id,
                &req.name,
                &req.connection_type,
                &req.config,
                secret_ref.as_deref(),
                req.enabled,
                now,
            )
            .await?;

        Ok(Some(Connection {
            id,
            name: req.name,
            connection_type: req.connection_type,
            config: req.config,
            enabled: req.enabled,
            has_secret: secret_ref.is_some(),
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    pub async fn delete(&self, id: Uuid) -> anyhow::Result<bool> {
        let key = Self::secret_ref_for(id);
        let _ = self.secrets.delete(&key).await;
        self.storage.delete_connection(id).await
    }

    pub async fn list(&self) -> anyhow::Result<Vec<Connection>> {
        self.storage.list_connections().await
    }

    pub async fn get(&self, id: Uuid) -> anyhow::Result<Option<Connection>> {
        self.storage.get_connection(id).await
    }

    /// Smoke-test: ensure connection exists, is enabled, and secret (if any) decrypts.
    /// For `telegram` type, optionally calls getMe against configured api_base.
    pub async fn test(&self, id: Uuid) -> anyhow::Result<(bool, String)> {
        let Some(conn) = self.storage.get_connection(id).await? else {
            return Ok((false, "connection not found".into()));
        };
        if !conn.enabled {
            return Ok((false, "connection is disabled".into()));
        }

        let credentials: Value = if conn.has_secret {
            let key = Self::secret_ref_for(id);
            let bytes = match self.secrets.get(&key).await? {
                Some(b) => b,
                None => return Ok((false, "secret_ref present but secret missing".into())),
            };
            serde_json::from_slice(&bytes).map_err(|e| anyhow::anyhow!("corrupt secret: {e}"))?
        } else {
            Value::Object(Default::default())
        };

        if conn.connection_type == "telegram" {
            if !conn.has_secret {
                return Ok((false, "telegram connection requires bot_token secret".into()));
            }
            match crate::integrations::telegram::TelegramClient::from_resolved(
                &conn.config,
                &credentials,
            ) {
                Ok(client) => match client.get_me().await {
                    Ok(me) => {
                        let username = me.get("username").and_then(Value::as_str).unwrap_or("bot");
                        Ok((true, format!("telegram ok (@{username})")))
                    }
                    Err(e) => Ok((false, format!("telegram {}: {e}", e.kind()))),
                },
                Err(e) => Ok((false, format!("telegram {}: {e}", e.kind()))),
            }
        } else if conn.connection_type == "openai" || conn.connection_type == "ai" {
            let api_key = credentials
                .get("api_key")
                .or_else(|| credentials.get("token"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            if api_key.is_none() {
                return Ok((false, "openai connection requires api_key in secret".into()));
            }
            Ok((true, "openai credentials readable".into()))
        } else if conn.connection_type == "github" {
            match crate::integrations::github::GitHubClient::from_resolved(
                &conn.config,
                &credentials,
            ) {
                Ok(client) => {
                    let owner = conn
                        .config
                        .get("owner")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let repo = conn
                        .config
                        .get("repo")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    if let (Some(owner), Some(repo)) = (owner, repo) {
                        match client.get_repo(owner, repo).await {
                            Ok(info) => {
                                let full = info
                                    .get("full_name")
                                    .and_then(Value::as_str)
                                    .unwrap_or(repo);
                                Ok((true, format!("github ok ({full})")))
                            }
                            Err(e) => Ok((false, e)),
                        }
                    } else if client.token.is_some() {
                        match client.get_authenticated_user().await {
                            Ok(user) => {
                                let login = user
                                    .get("login")
                                    .and_then(Value::as_str)
                                    .unwrap_or("user");
                                Ok((true, format!("github ok (@{login})")))
                            }
                            Err(e) => Ok((false, e)),
                        }
                    } else {
                        Ok((
                            true,
                            "github config readable (set owner/repo or token to verify access)"
                                .into(),
                        ))
                    }
                }
                Err(e) => Ok((false, e)),
            }
        } else if !conn.has_secret {
            Ok((true, "connection has no secret (config-only)".into()))
        } else {
            Ok((
                true,
                format!("{} credentials readable", conn.connection_type),
            ))
        }
    }
}

#[async_trait]
impl ConnectionProvider for StorageConnectionProvider {
    async fn resolve(&self, connection_id: &str) -> Result<ResolvedConnection, String> {
        let id = Uuid::parse_str(connection_id).map_err(|_| "invalid connection_id".to_string())?;
        let conn = self
            .storage
            .get_connection(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("connection `{connection_id}` not found"))?;

        if !conn.enabled {
            return Err(format!("connection `{connection_id}` is disabled"));
        }

        let credentials = if conn.has_secret {
            let key = Self::secret_ref_for(id);
            let bytes = self
                .secrets
                .get(&key)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "connection secret missing".to_string())?;
            serde_json::from_slice(&bytes).map_err(|e| format!("corrupt secret JSON: {e}"))?
        } else {
            Value::Object(Default::default())
        };

        Ok(ResolvedConnection {
            id: conn.id,
            name: conn.name,
            connection_type: conn.connection_type,
            config: conn.config,
            credentials,
        })
    }

    async fn get_public(&self, connection_id: &str) -> Result<Option<Connection>, String> {
        let id = Uuid::parse_str(connection_id).map_err(|_| "invalid connection_id".to_string())?;
        self.storage
            .get_connection(id)
            .await
            .map_err(|e| e.to_string())
    }
}

/// No-op provider for unit tests / engine without connections.
pub struct NullConnectionProvider;

#[async_trait]
impl ConnectionProvider for NullConnectionProvider {
    async fn resolve(&self, connection_id: &str) -> Result<ResolvedConnection, String> {
        Err(format!(
            "no connection provider (requested `{connection_id}`)"
        ))
    }

    async fn get_public(&self, _connection_id: &str) -> Result<Option<Connection>, String> {
        Ok(None)
    }
}
