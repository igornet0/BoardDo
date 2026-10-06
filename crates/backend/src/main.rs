//! SmartDo — BoardDo workflow execution engine.

mod api;
mod agent_runtime;
mod content;
mod connections;
mod engine;
mod ingress;
mod integrations;
mod runtime;
mod scheduler;
mod secrets;
mod state;
mod storage;
mod tools;
mod telegram_automation;
mod telegram_demand;
mod telegram_error_report;
mod triggers;
mod workflow;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tracing_subscriber::EnvFilter;

use crate::connections::StorageConnectionProvider;
use crate::secrets::EncryptedSqliteSecretStore;
use crate::state::AppState;
use crate::storage::Storage;
use boarddo_telegram::{TelegramEngine, TelegramSessionStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,smartdo=debug")),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:boarddo.db?mode=rwc".into());
    let listen = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let data_dir =
        PathBuf::from(std::env::var("BOARDDO_DATA_DIR").unwrap_or_else(|_| "data".into()));

    let storage = Storage::connect(&database_url).await?;
    storage.migrate().await?;

    let secrets_store = Arc::new(EncryptedSqliteSecretStore::from_env(
        storage.pool().clone(),
    )?);
    secrets_store.migrate().await?;

    let secrets: Arc<dyn crate::secrets::SecretStore> = secrets_store.clone();
    let connections = Arc::new(StorageConnectionProvider::new(storage.clone(), secrets.clone()));

    let telegram_store = Arc::new(TelegramSessionStore::new(storage.pool().clone()));
    telegram_store.migrate().await?;
    let telegram =
        build_telegram_engine(telegram_store, data_dir.clone(), secrets_store.as_ref()).await;
    // TDLib clients are started on demand (armed Telegram scenarios or explicit
    // Connect) — not by restoring every desired_running account at boot.

    let state = Arc::new(AppState::new(
        storage,
        secrets,
        connections,
        telegram.clone(),
        data_dir,
    ));
    scheduler::spawn_arc(state.clone());
    crate::agent_runtime::spawn_arc(state.clone());
    telegram_automation::spawn(state.clone());
    if let Err(err) = state.runtime.restore(&state).await {
        tracing::error!(error = %err, "runtime.restore_failed");
    }
    telegram_demand::sync_telegram_accounts(&state).await;

    let app = api::router(state);

    let addr: SocketAddr = listen.parse()?;
    tracing::info!(%addr, "SmartDo listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(telegram))
        .await?;
    // A BoardDo-managed RustBrowser goes down with us (it also exits on stdin EOF).
    crate::integrations::browser::shared().shutdown().await;

    Ok(())
}

async fn build_telegram_engine(
    store: Arc<TelegramSessionStore>,
    data_dir: PathBuf,
    secrets: &EncryptedSqliteSecretStore,
) -> Arc<TelegramEngine> {
    #[cfg(feature = "tdlib")]
    {
        use crate::secrets::SecretStore;
        use tglib::{
            SECRET_API_HASH, SECRET_API_ID, TdlibClientFactory, TelegramConfigInput,
            TelegramConfigProvider,
        };

        let secret_api_id = secrets
            .get(SECRET_API_ID)
            .await
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse().ok());
        let secret_api_hash = secrets
            .get(SECRET_API_HASH)
            .await
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .filter(|s| !s.is_empty());

        let resolved = TelegramConfigProvider::resolve(TelegramConfigInput {
            data_dir: data_dir.clone(),
            secret_api_id,
            secret_api_hash,
        });
        let config = resolved.config;

        if config.is_complete() {
            tracing::info!(
                source = %config.source,
                "telegram.application.credentials_loaded"
            );
            if resolved.persist_to_secure_storage {
                let _ = secrets
                    .set(SECRET_API_ID, config.api_id.to_string().as_bytes())
                    .await;
                let _ = secrets
                    .set(SECRET_API_HASH, config.api_hash.as_bytes())
                    .await;
                match TelegramConfigProvider::store_in_keychain(&config) {
                    Ok(()) => tracing::info!("telegram.application.credentials_stored_keychain"),
                    Err(err) => tracing::debug!(
                        error = %err,
                        "telegram.application.keychain_store_skipped"
                    ),
                }
                tracing::warn!(
                    "telegram.application.plaintext_hash_copied_to_secure_storage — remove api_hash from telegram.toml"
                );
            }
        } else {
            tracing::warn!(
                "telegram.application.credentials_missing — user accounts cannot authorize until BoardDo application api_id/api_hash are configured"
            );
        }

        let factory = Arc::new(TdlibClientFactory::from_config(config));
        tracing::info!("telegram.tdlib.feature_enabled");
        Arc::new(TelegramEngine::new(store, data_dir, factory, None))
    }
    #[cfg(not(feature = "tdlib"))]
    {
        let _ = secrets;
        let (engine, _) = TelegramEngine::with_mock(store, data_dir);
        tracing::info!("telegram.mock.enabled");
        Arc::new(engine)
    }
}

async fn shutdown_signal(telegram: Arc<TelegramEngine>) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("telegram.shutdown.begin");
    telegram.shutdown_all().await;
    tracing::info!("telegram.shutdown.complete");
}
