#[cfg(test)]
mod connection_tests {
    use std::sync::Arc;

    use boarddo_shared::CreateConnectionRequest;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    use crate::connections::{ConnectionProvider, StorageConnectionProvider};
    use crate::secrets::{EncryptedSqliteSecretStore, SecretStore, redact_value};

    async fn setup() -> (StorageConnectionProvider, sqlx::SqlitePool) {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // minimal schema for connections + secrets
        sqlx::query(
            r#"
            CREATE TABLE connections (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                type TEXT NOT NULL,
                config TEXT NOT NULL DEFAULT '{}',
                secret_ref TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let secrets = Arc::new(EncryptedSqliteSecretStore::new(pool.clone(), [7u8; 32]));
        secrets.migrate().await.unwrap();

        let storage = crate::storage::Storage::from_pool(pool.clone());
        let provider = StorageConnectionProvider::new(storage, secrets);
        (provider, pool)
    }

    #[tokio::test]
    async fn secret_roundtrip_and_redaction() {
        let store = EncryptedSqliteSecretStore::new(
            SqlitePoolOptions::new()
                .connect("sqlite::memory:")
                .await
                .unwrap(),
            [9u8; 32],
        );
        store.migrate().await.unwrap();
        store.set("k1", br#"{"token":"abc"}"#).await.unwrap();
        let got = store.get("k1").await.unwrap().unwrap();
        assert_eq!(got, br#"{"token":"abc"}"#);

        let redacted = redact_value(&json!({"token":"abc","ok":1}));
        assert_eq!(redacted["token"], "[REDACTED]");
        assert_eq!(redacted["ok"], 1);
    }

    #[tokio::test]
    async fn connection_api_never_returns_secret() {
        let (provider, _pool) = setup().await;
        let created = provider
            .create(CreateConnectionRequest {
                name: "Prod HTTP".into(),
                connection_type: "http".into(),
                config: json!({"base_url": "https://api.example.com"}),
                secret: Some(json!({"auth": {"type": "bearer", "token": "super-secret"}})),
                enabled: true,
            })
            .await
            .unwrap();

        assert!(created.has_secret);
        let public = serde_json::to_value(&created).unwrap();
        let s = public.to_string();
        assert!(!s.contains("super-secret"));
        assert!(public.get("secret").is_none());
        assert!(public.get("token").is_none());
        assert_eq!(public["has_secret"], true);

        let resolved = provider.resolve(&created.id.to_string()).await.unwrap();
        assert_eq!(resolved.credentials["auth"]["token"], "super-secret");
        // Debug redacts
        let dbg = format!("{resolved:?}");
        assert!(!dbg.contains("super-secret"));
        assert!(dbg.contains("REDACTED"));
    }
}
