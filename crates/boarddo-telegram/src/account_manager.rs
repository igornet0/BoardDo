use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use tglib::{
    DeleteMessagesRequest, EditMessageRequest, ForwardMessagesRequest, GetMessagesRequest,
    ListChatsRequest, ListChatsResponse, MockClientFactory, MockTelegramClient, SendMessageRequest,
    TelegramChat, TelegramClient, TelegramClientFactory, TelegramClientUpdate, TelegramMessage,
};
use tglib::{ChatMembersResponse, GetChatMembersRequest, TelegramUser, TelegramUserId};
use tglib::{
    TelegramAccount, TelegramAccountCreated, TelegramAccountId, TelegramAccountPermissions,
    TelegramAccountProfile, TelegramAccountStatus, TelegramChatId, TelegramError, TelegramEvent,
    TelegramMessageId, ACCOUNT_CONSENT,
};

use crate::events::TelegramEventRouter;
use crate::reconnect::ReconnectBackoff;
use crate::session::SessionPaths;
use crate::store::TelegramSessionStore;

const EVENT_CAPACITY: usize = 512;

struct LiveAccount {
    client: Arc<dyn TelegramClient>,
    shutdown: watch::Sender<bool>,
    join: JoinHandle<()>,
}

pub struct TelegramAccountManager {
    store: Arc<TelegramSessionStore>,
    data_dir: PathBuf,
    factory: Arc<dyn TelegramClientFactory>,
    mock_factory: Option<Arc<MockClientFactory>>,
    live: DashMap<TelegramAccountId, LiveAccount>,
    events: broadcast::Sender<TelegramEvent>,
}

impl TelegramAccountManager {
    pub fn new(
        store: Arc<TelegramSessionStore>,
        data_dir: PathBuf,
        factory: Arc<dyn TelegramClientFactory>,
        mock_factory: Option<Arc<MockClientFactory>>,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            store,
            data_dir,
            factory,
            mock_factory,
            live: DashMap::new(),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TelegramEvent> {
        self.events.subscribe()
    }

    pub fn mock_client(&self, id: TelegramAccountId) -> Option<Arc<MockTelegramClient>> {
        self.mock_factory.as_ref().and_then(|f| f.get(id))
    }

    pub fn mock_factory_present(&self) -> bool {
        self.mock_factory.is_some()
    }

    pub async fn create_account(&self) -> Result<TelegramAccountCreated, TelegramError> {
        let now = Utc::now();
        let account = TelegramAccount {
            id: TelegramAccountId::new(),
            status: TelegramAccountStatus::Created,
            phone_masked: None,
            username: None,
            display_name: None,
            permissions: TelegramAccountPermissions::default(),
            desired_running: true,
            error: None,
            created_at: now,
            updated_at: now,
        };
        let paths = SessionPaths::for_account(&self.data_dir, account.id);
        paths
            .ensure()
            .map_err(|e| TelegramError::Account(e.to_string()))?;
        self.store
            .insert_account(&account)
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))?;
        tracing::info!(account_id = %account.id, "telegram.account.created");
        self.start_account(account.id).await?;
        let account = self
            .wait_until_user_facing(account.id, Duration::from_secs(45))
            .await?;
        Ok(TelegramAccountCreated {
            account,
            consent: ACCOUNT_CONSENT.to_string(),
        })
    }

    pub async fn get_account(
        &self,
        id: TelegramAccountId,
    ) -> Result<Option<TelegramAccount>, TelegramError> {
        self.store
            .get_account(id)
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))
    }

    pub async fn list_accounts(&self) -> Result<Vec<TelegramAccount>, TelegramError> {
        self.store
            .list_accounts()
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))
    }

    pub async fn start_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        let mut account = self
            .get_account(id)
            .await?
            .ok_or(TelegramError::AccountNotFound)?;
        account.desired_running = true;
        account.updated_at = Utc::now();
        self.persist(&account).await?;
        self.spawn_runtime(id).await
    }

    /// Bring an account online for scenario demand without flipping `desired_running`.
    /// Used when an armed runtime needs TDLib; user Connect still owns persistence intent.
    pub async fn ensure_running(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        let _ = self
            .get_account(id)
            .await?
            .ok_or(TelegramError::AccountNotFound)?;
        self.spawn_runtime(id).await
    }

    /// Halt the TDLib client without clearing `desired_running` (scenario demand ended).
    pub async fn pause_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.halt_runtime(id, false).await
    }

    pub fn is_running(&self, id: TelegramAccountId) -> bool {
        self.live.contains_key(&id)
    }

    pub async fn connect_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.start_account(id).await
    }

    pub async fn stop_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.set_desired_running(id, false).await?;
        self.halt_runtime(id, false).await
    }

    pub async fn disconnect_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.set_desired_running(id, false).await?;
        self.halt_runtime(id, true).await
    }

    pub async fn delete_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.halt_runtime(id, true).await?;
        let paths = SessionPaths::for_account(&self.data_dir, id);
        paths
            .remove_all()
            .map_err(|e| TelegramError::Account(e.to_string()))?;
        self.store
            .delete_account(id)
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))?;
        tracing::info!(account_id = %id, "telegram.account.deleted");
        Ok(())
    }

    pub async fn restore_running_accounts(&self) -> Result<(), TelegramError> {
        let accounts = self.list_accounts().await?;
        for account in accounts {
            if account.desired_running {
                if let Err(err) = self.spawn_runtime(account.id).await {
                    tracing::error!(account_id = %account.id, error = %err, "telegram.account.restore_failed");
                }
            }
        }
        Ok(())
    }

    pub async fn shutdown_all(&self) {
        let ids: Vec<_> = self.live.iter().map(|e| *e.key()).collect();
        for id in ids {
            if let Err(err) = self.halt_runtime(id, false).await {
                tracing::warn!(account_id = %id, error = %err, "telegram.account.shutdown_error");
            }
        }
    }

    pub async fn submit_phone(
        &self,
        id: TelegramAccountId,
        phone: String,
    ) -> Result<(), TelegramError> {
        let client = self.require_client(id).await?;
        client.submit_phone(phone).await
    }

    pub async fn submit_code(
        &self,
        id: TelegramAccountId,
        code: String,
    ) -> Result<(), TelegramError> {
        let client = self.require_client(id).await?;
        client.submit_code(code).await
    }

    pub async fn submit_password(
        &self,
        id: TelegramAccountId,
        password: String,
    ) -> Result<(), TelegramError> {
        let client = self.require_client(id).await?;
        client.submit_password(password).await
    }

    pub async fn list_chats(
        &self,
        id: TelegramAccountId,
        request: ListChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        self.require_client(id).await?.list_chats(request).await
    }

    pub async fn search_chats(
        &self,
        id: TelegramAccountId,
        request: tglib::SearchChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        self.require_client(id).await?.search_chats(request).await
    }

    pub async fn get_chat(
        &self,
        id: TelegramAccountId,
        chat_id: TelegramChatId,
    ) -> Result<TelegramChat, TelegramError> {
        self.require_client(id).await?.get_chat(chat_id).await
    }

    pub async fn get_messages(
        &self,
        id: TelegramAccountId,
        request: GetMessagesRequest,
    ) -> Result<Vec<TelegramMessage>, TelegramError> {
        self.require_client(id).await?.get_messages(request).await
    }

    pub async fn get_user(
        &self,
        id: TelegramAccountId,
        user_id: TelegramUserId,
    ) -> Result<TelegramUser, TelegramError> {
        self.require_client(id).await?.get_user(user_id).await
    }

    pub async fn get_chat_members(
        &self,
        id: TelegramAccountId,
        request: GetChatMembersRequest,
    ) -> Result<ChatMembersResponse, TelegramError> {
        self.require_client(id).await?.get_chat_members(request).await
    }

    pub async fn get_message(
        &self,
        id: TelegramAccountId,
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
    ) -> Result<TelegramMessage, TelegramError> {
        self.require_client(id)
            .await?
            .get_message(chat_id, message_id)
            .await
    }

    pub async fn send_message(
        &self,
        id: TelegramAccountId,
        request: SendMessageRequest,
    ) -> Result<TelegramMessageId, TelegramError> {
        self.require_client(id).await?.send_message(request).await
    }

    pub async fn forward_messages(
        &self,
        id: TelegramAccountId,
        request: ForwardMessagesRequest,
    ) -> Result<(), TelegramError> {
        self.require_client(id)
            .await?
            .forward_messages(request)
            .await
    }

    pub async fn edit_message(
        &self,
        id: TelegramAccountId,
        request: EditMessageRequest,
    ) -> Result<(), TelegramError> {
        self.require_client(id).await?.edit_message(request).await
    }

    pub async fn delete_messages(
        &self,
        id: TelegramAccountId,
        request: DeleteMessagesRequest,
    ) -> Result<(), TelegramError> {
        self.require_client(id)
            .await?
            .delete_messages(request)
            .await
    }

    pub async fn get_account_info(
        &self,
        id: TelegramAccountId,
    ) -> Result<TelegramAccountProfile, TelegramError> {
        self.require_client(id).await?.get_account_info().await
    }

    pub async fn check_ready(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        let account = self.get_account(id).await?.ok_or(TelegramError::AccountNotFound)?;
        if account.status == TelegramAccountStatus::Ready {
            Ok(())
        } else {
            Err(TelegramError::NotReady)
        }
    }

    async fn require_client(
        &self,
        id: TelegramAccountId,
    ) -> Result<Arc<dyn TelegramClient>, TelegramError> {
        if let Some(live) = self.live.get(&id) {
            return Ok(live.client.clone());
        }
        self.start_account(id).await?;
        self.live
            .get(&id)
            .map(|l| l.client.clone())
            .ok_or(TelegramError::AccountNotFound)
    }

    async fn set_desired_running(
        &self,
        id: TelegramAccountId,
        running: bool,
    ) -> Result<(), TelegramError> {
        let mut account = self.get_account(id).await?.ok_or(TelegramError::AccountNotFound)?;
        account.desired_running = running;
        account.updated_at = Utc::now();
        self.persist(&account).await
    }

    async fn persist(&self, account: &TelegramAccount) -> Result<(), TelegramError> {
        self.store
            .update_account(account)
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))
    }

    async fn wait_until_user_facing(
        &self,
        id: TelegramAccountId,
        timeout: Duration,
    ) -> Result<TelegramAccount, TelegramError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let account = self
                .get_account(id)
                .await?
                .ok_or(TelegramError::AccountNotFound)?;
            if account.status.is_user_facing() {
                return Ok(account);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(account);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn spawn_runtime(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        if self.live.contains_key(&id) {
            return Ok(());
        }
        let _ = self.get_account(id).await?.ok_or(TelegramError::AccountNotFound)?;
        let paths = SessionPaths::for_account(&self.data_dir, id);
        paths
            .ensure()
            .map_err(|e| TelegramError::Account(e.to_string()))?;
        let client = self.factory.create(id, &paths.root);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let store = self.store.clone();
        let events = self.events.clone();
        let task_client = client.clone();
        let join = tokio::spawn(async move {
            account_runtime(id, task_client, store, events, shutdown_rx).await;
        });
        self.live.insert(
            id,
            LiveAccount {
                client,
                shutdown: shutdown_tx,
                join,
            },
        );
        Ok(())
    }

    async fn halt_runtime(&self, id: TelegramAccountId, logout: bool) -> Result<(), TelegramError> {
        let Some((_, live)) = self.live.remove(&id) else {
            self.mark_status(id, TelegramAccountStatus::Disconnected, None)
                .await?;
            return Ok(());
        };
        if logout {
            let _ = live.client.logout().await;
        }
        let _ = live.shutdown.send(true);
        match tokio::time::timeout(Duration::from_secs(5), live.join).await {
            Ok(_) => {}
            Err(_) => tracing::warn!(account_id = %id, "telegram.account.shutdown_timeout"),
        }
        let _ = live.client.shutdown().await;
        self.mark_status(id, TelegramAccountStatus::Disconnected, None)
            .await?;
        tracing::info!(account_id = %id, "telegram.account.disconnected");
        Ok(())
    }

    async fn mark_status(
        &self,
        id: TelegramAccountId,
        status: TelegramAccountStatus,
        error: Option<String>,
    ) -> Result<(), TelegramError> {
        if let Some(mut account) = self.get_account(id).await? {
            account.status = status;
            account.error = error;
            account.updated_at = Utc::now();
            self.persist(&account).await?;
        }
        Ok(())
    }
}

async fn account_runtime(
    account_id: TelegramAccountId,
    client: Arc<dyn TelegramClient>,
    store: Arc<TelegramSessionStore>,
    events: broadcast::Sender<TelegramEvent>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = ReconnectBackoff::new();
    loop {
        if *shutdown.borrow() {
            break;
        }
        match run_session(
            account_id,
            client.clone(),
            store.clone(),
            events.clone(),
            &mut shutdown,
            &mut backoff,
        )
        .await
        {
            SessionEnd::Stopped => break,
            SessionEnd::Failed => {
                if *shutdown.borrow() {
                    break;
                }
                match backoff.next_delay() {
                    Some(delay) => {
                        tracing::info!(
                            account_id = %account_id,
                            delay_ms = delay.as_millis() as u64,
                            "telegram.account.reconnecting"
                        );
                        tokio::select! {
                            _ = tokio::time::sleep(delay) => {}
                            _ = shutdown.changed() => {
                                if *shutdown.borrow() {
                                    break;
                                }
                            }
                        }
                    }
                    None => {
                        tracing::error!(account_id = %account_id, "telegram.account.reconnect_exhausted");
                        let _ = apply_status(
                            &store,
                            account_id,
                            TelegramAccountStatus::Error,
                            Some("reconnect exhausted".into()),
                            None,
                            None,
                        )
                        .await;
                        let _ = shutdown.changed().await;
                        break;
                    }
                }
            }
        }
    }
    let _ = client.shutdown().await;
}

enum SessionEnd {
    Stopped,
    Failed,
}

async fn run_session(
    account_id: TelegramAccountId,
    client: Arc<dyn TelegramClient>,
    store: Arc<TelegramSessionStore>,
    events: broadcast::Sender<TelegramEvent>,
    shutdown: &mut watch::Receiver<bool>,
    backoff: &mut ReconnectBackoff,
) -> SessionEnd {
    let mut rx = client.subscribe();
    let _ = apply_status(
        &store,
        account_id,
        TelegramAccountStatus::Initializing,
        None,
        None,
        None,
    )
    .await;

    if let Err(err) = client.start().await {
        let _ = apply_status(
            &store,
            account_id,
            TelegramAccountStatus::Error,
            Some(err.to_string()),
            None,
            None,
        )
        .await;
        return SessionEnd::Failed;
    }
    backoff.reset();

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return SessionEnd::Stopped;
                }
            }
            msg = rx.recv() => {
                match msg {
                    Ok(update) => {
                        let fail = matches!(update, TelegramClientUpdate::Error { .. });
                        apply_client_update(&store, account_id, &update).await;
                        if let Some(event) = TelegramEventRouter::map(account_id, update) {
                            debug_assert!(TelegramEventRouter::ensure_account_id(&event, account_id));
                            let _ = events.send(event);
                        }
                        if fail {
                            return SessionEnd::Failed;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return SessionEnd::Failed,
                }
            }
        }
    }
}

async fn apply_client_update(
    store: &TelegramSessionStore,
    account_id: TelegramAccountId,
    update: &TelegramClientUpdate,
) {
    match update {
        TelegramClientUpdate::AuthState { status, error } => {
            let _ = apply_status(store, account_id, *status, error.clone(), None, None).await;
            match status {
                TelegramAccountStatus::Ready => {
                    tracing::info!(account_id = %account_id, "telegram.account.ready");
                }
                TelegramAccountStatus::Disconnected => {
                    tracing::info!(account_id = %account_id, "telegram.account.disconnected");
                }
                _ => {}
            }
        }
        TelegramClientUpdate::Profile { profile } => {
            let _ = apply_status(
                store,
                account_id,
                TelegramAccountStatus::Ready,
                None,
                Some(profile),
                None,
            )
            .await;
        }
        TelegramClientUpdate::Error { message } => {
            let _ = apply_status(
                store,
                account_id,
                TelegramAccountStatus::Error,
                Some(message.clone()),
                None,
                None,
            )
            .await;
        }
        TelegramClientUpdate::Closed => {
            let _ = apply_status(
                store,
                account_id,
                TelegramAccountStatus::Disconnected,
                None,
                None,
                None,
            )
            .await;
        }
        TelegramClientUpdate::MessageReceived { chat_id, .. } => {
            // Inbound TDLib traffic (before scenario filters). Keep at debug so
            // ignored chats do not flood info logs; scenario fires stay at info.
            tracing::debug!(
                account_id = %account_id,
                chat_id = %chat_id,
                "telegram.message.received"
            );
        }
        _ => {}
    }
}

async fn apply_status(
    store: &TelegramSessionStore,
    account_id: TelegramAccountId,
    status: TelegramAccountStatus,
    error: Option<String>,
    profile: Option<&TelegramAccountProfile>,
    _unused: Option<()>,
) -> anyhow::Result<()> {
    if let Some(mut account) = store.get_account(account_id).await? {
        account.status = status;
        account.error = error;
        if let Some(profile) = profile {
            account.phone_masked = profile.phone_masked.clone();
            account.username = profile.username.clone();
            account.display_name = profile.display_name.clone();
        }
        account.updated_at = Utc::now();
        store.update_account(&account).await?;
    }
    Ok(())
}
