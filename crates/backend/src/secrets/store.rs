//! Local AES-256-GCM secret store backed by SQLite.
//!
//! Key source: `BOARDDO_SECRETS_KEY` (64 hex chars = 32 bytes).
//! If unset, a deterministic local-dev key is derived (log a warning).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use async_trait::async_trait;
use chrono::Utc;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use super::SecretStore;

const NONCE_LEN: usize = 12;

pub struct EncryptedSqliteSecretStore {
    pool: SqlitePool,
    cipher: Aes256Gcm,
}

impl EncryptedSqliteSecretStore {
    pub fn new(pool: SqlitePool, master_key: [u8; 32]) -> Self {
        let cipher = Aes256Gcm::new_from_slice(&master_key).expect("AES-256 key length");
        Self { pool, cipher }
    }

    pub fn from_env(pool: SqlitePool) -> anyhow::Result<Self> {
        let key = load_master_key()?;
        Ok(Self::new(pool, key))
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS secrets (
                key TEXT PRIMARY KEY NOT NULL,
                ciphertext BLOB NOT NULL,
                nonce BLOB NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl SecretStore for EncryptedSqliteSecretStore {
    async fn set(&self, key: &str, value: &[u8]) -> anyhow::Result<()> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, value)
            .map_err(|e| anyhow::anyhow!("encrypt failed: {e}"))?;

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO secrets (key, ciphertext, nonce, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                ciphertext = excluded.ciphertext,
                nonce = excluded.nonce,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key)
        .bind(&ciphertext)
        .bind(nonce_bytes.as_slice())
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let row: Option<(Vec<u8>, Vec<u8>)> =
            sqlx::query_as("SELECT ciphertext, nonce FROM secrets WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        let Some((ciphertext, nonce_bytes)) = row else {
            return Ok(None);
        };
        if nonce_bytes.len() != NONCE_LEN {
            anyhow::bail!("corrupt secret nonce for key");
        }
        let nonce = Nonce::from_slice(&nonce_bytes);
        let plain = self
            .cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|_| anyhow::anyhow!("decrypt failed (wrong BOARDDO_SECRETS_KEY?)"))?;
        Ok(Some(plain))
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM secrets WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn load_master_key() -> anyhow::Result<[u8; 32]> {
    if let Ok(hex_key) = std::env::var("BOARDDO_SECRETS_KEY") {
        let bytes = hex::decode(hex_key.trim())
            .map_err(|_| anyhow::anyhow!("BOARDDO_SECRETS_KEY must be 64 hex characters"))?;
        if bytes.len() != 32 {
            anyhow::bail!("BOARDDO_SECRETS_KEY must decode to 32 bytes");
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }

    tracing::warn!(
        "BOARDDO_SECRETS_KEY unset — using local-dev key. Set a 64-hex key for production."
    );
    let mut hasher = Sha256::new();
    hasher.update(b"boarddo-local-dev-secrets-v1");
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    Ok(key)
}

/// Redact secret-looking fields from JSON before logging / WS / debugger export.
pub fn redact_value(value: &serde_json::Value) -> serde_json::Value {
    const SENSITIVE: &[&str] = &[
        "token",
        "password",
        "secret",
        "api_key",
        "apikey",
        "authorization",
        "auth",
        "bearer",
        "bot_token",
        "access_token",
        "refresh_token",
        "private_key",
        "client_secret",
        "api_hash",
    ];

    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let lower = k.to_ascii_lowercase();
                if SENSITIVE.iter().any(|s| lower.contains(s)) {
                    out.insert(k.clone(), serde_json::Value::String("[REDACTED]".into()));
                } else {
                    out.insert(k.clone(), redact_value(v));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(redact_value).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redact_nested() {
        let v = json!({
            "url": "https://x",
            "headers": { "Authorization": "Bearer abc", "X-Trace": "1" },
            "token": "secret"
        });
        let r = redact_value(&v);
        assert_eq!(r["token"], "[REDACTED]");
        assert_eq!(r["headers"]["Authorization"], "[REDACTED]");
        assert_eq!(r["headers"]["X-Trace"], "1");
    }
}
