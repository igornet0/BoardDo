use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::broadcast;
use uuid::Uuid;

use tglib::{GetChatMembersRequest, TelegramUser, TelegramUserId};
use tglib::{
    DeleteMessagesRequest, EditMessageRequest, ForwardMessagesRequest, GetMessagesRequest,
    ListChatsRequest, ListChatsResponse, MockClientFactory, SearchChatsRequest, SendMessageRequest,
    TelegramChat, TelegramClientFactory, TelegramMessage,
};
use tglib::{
    TelegramAccount, TelegramAccountCreated, TelegramAccountId, TelegramAccountPermissions,
    TelegramAccountProfile, TelegramActionKind, TelegramAuditEvent, TelegramAuditStatus,
    TelegramChatId, TelegramError, TelegramEvent, TelegramMessageId, TelegramScenarioExecution,
};

use crate::account_manager::TelegramAccountManager;
use crate::audit::TelegramAuditService;
use crate::members_cache::{ChatMembersResult, MembersCache};
use crate::outbound::OutboundTracker;
use crate::permission::TelegramPermissionService;
use crate::store::TelegramSessionStore;

/// Permission-checked Telegram user operations used by SmartDo action nodes.
#[async_trait]
pub trait TelegramUserGateway: Send + Sync {
    /// `reply_to` sends the text as a reply to that message in `chat_id`.
    #[allow(clippy::too_many_arguments)]
    async fn send_message(
        &self,
        account_id: TelegramAccountId,
        chat_id: TelegramChatId,
        text: String,
        parse_mode: Option<String>,
        reply_to: Option<TelegramMessageId>,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
    ) -> Result<TelegramMessageId, TelegramError>;

    async fn get_user(
        &self,
        account_id: TelegramAccountId,
        user_id: TelegramUserId,
    ) -> Result<TelegramUser, TelegramError>;

    /// With `max_age`, a cached list at most that old is reused.
    async fn get_chat_members(
        &self,
        account_id: TelegramAccountId,
        request: GetChatMembersRequest,
        max_age: Option<Duration>,
    ) -> Result<ChatMembersResult, TelegramError>;

    async fn forward_messages(
        &self,
        account_id: TelegramAccountId,
        request: ForwardMessagesRequest,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError>;

    async fn edit_message(
        &self,
        account_id: TelegramAccountId,
        request: EditMessageRequest,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError>;

    async fn delete_messages(
        &self,
        account_id: TelegramAccountId,
        request: DeleteMessagesRequest,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError>;
}

pub struct NullTelegramUserGateway;

#[async_trait]
impl TelegramUserGateway for NullTelegramUserGateway {
    async fn send_message(
        &self,
        _account_id: TelegramAccountId,
        _chat_id: TelegramChatId,
        _text: String,
        _parse_mode: Option<String>,
        _reply_to: Option<TelegramMessageId>,
        _execution_id: Option<Uuid>,
        _scenario_id: Option<Uuid>,
    ) -> Result<TelegramMessageId, TelegramError> {
        Err(TelegramError::Unavailable("telegram user engine not configured".into()))
    }

    async fn get_user(
        &self,
        _account_id: TelegramAccountId,
        _user_id: TelegramUserId,
    ) -> Result<TelegramUser, TelegramError> {
        Err(TelegramError::Unavailable("telegram user engine not configured".into()))
    }

    async fn get_chat_members(
        &self,
        _account_id: TelegramAccountId,
        _request: GetChatMembersRequest,
        _max_age: Option<Duration>,
    ) -> Result<ChatMembersResult, TelegramError> {
        Err(TelegramError::Unavailable("telegram user engine not configured".into()))
    }

    async fn forward_messages(
        &self,
        _account_id: TelegramAccountId,
        _request: ForwardMessagesRequest,
        _execution_id: Option<Uuid>,
        _scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError> {
        Err(TelegramError::Unavailable("telegram user engine not configured".into()))
    }

    async fn edit_message(
        &self,
        _account_id: TelegramAccountId,
        _request: EditMessageRequest,
        _execution_id: Option<Uuid>,
        _scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError> {
        Err(TelegramError::Unavailable("telegram user engine not configured".into()))
    }

    async fn delete_messages(
        &self,
        _account_id: TelegramAccountId,
        _request: DeleteMessagesRequest,
        _execution_id: Option<Uuid>,
        _scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError> {
        Err(TelegramError::Unavailable("telegram user engine not configured".into()))
    }
}

pub struct TelegramEngine {
    pub store: Arc<TelegramSessionStore>,
    pub manager: Arc<TelegramAccountManager>,
    pub permissions: Arc<TelegramPermissionService>,
    pub audit: Arc<TelegramAuditService>,
    pub outbound: Arc<OutboundTracker>,
    pub members_cache: Arc<MembersCache>,
}

impl TelegramEngine {
    pub fn new(
        store: Arc<TelegramSessionStore>,
        data_dir: PathBuf,
        factory: Arc<dyn TelegramClientFactory>,
        mock_factory: Option<Arc<MockClientFactory>>,
    ) -> Self {
        let manager = Arc::new(TelegramAccountManager::new(
            store.clone(),
            data_dir,
            factory,
            mock_factory,
        ));
        let permissions = Arc::new(TelegramPermissionService::new(store.clone()));
        let audit = Arc::new(TelegramAuditService::new(store.clone()));
        Self {
            store,
            manager,
            permissions,
            audit,
            outbound: Arc::new(OutboundTracker::new()),
            members_cache: Arc::new(MembersCache::new()),
        }
    }

    pub fn with_mock(store: Arc<TelegramSessionStore>, data_dir: PathBuf) -> (Self, Arc<MockClientFactory>) {
        let mock = Arc::new(MockClientFactory::new());
        let engine = Self::new(store, data_dir, mock.clone(), Some(mock.clone()));
        (engine, mock)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<TelegramEvent> {
        self.manager.subscribe()
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        self.store.migrate().await
    }

    pub async fn restore(&self) -> Result<(), TelegramError> {
        self.manager.restore_running_accounts().await
    }

    pub async fn shutdown_all(&self) {
        self.manager.shutdown_all().await;
    }

    pub async fn create_account(&self) -> Result<TelegramAccountCreated, TelegramError> {
        self.manager.create_account().await
    }

    pub async fn list_accounts(&self) -> Result<Vec<TelegramAccount>, TelegramError> {
        self.manager.list_accounts().await
    }

    pub async fn get_account(
        &self,
        id: TelegramAccountId,
    ) -> Result<Option<TelegramAccount>, TelegramError> {
        self.manager.get_account(id).await
    }

    pub async fn delete_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.manager.delete_account(id).await
    }

    pub async fn start_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.manager.start_account(id).await
    }

    pub async fn ensure_running(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.manager.ensure_running(id).await
    }

    pub async fn pause_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.manager.pause_account(id).await
    }

    pub fn is_running(&self, id: TelegramAccountId) -> bool {
        self.manager.is_running(id)
    }

    pub async fn stop_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.manager.stop_account(id).await
    }

    pub async fn connect_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.manager.connect_account(id).await
    }

    pub async fn disconnect_account(&self, id: TelegramAccountId) -> Result<(), TelegramError> {
        self.manager.disconnect_account(id).await
    }

    pub async fn submit_phone(&self, id: TelegramAccountId, phone: String) -> Result<(), TelegramError> {
        self.manager.submit_phone(id, phone).await
    }

    pub async fn submit_code(&self, id: TelegramAccountId, code: String) -> Result<(), TelegramError> {
        self.manager.submit_code(id, code).await
    }

    pub async fn submit_password(
        &self,
        id: TelegramAccountId,
        password: String,
    ) -> Result<(), TelegramError> {
        self.manager.submit_password(id, password).await
    }

    pub async fn list_chats(
        &self,
        id: TelegramAccountId,
        request: ListChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        self.permissions
            .check(id, TelegramActionKind::ReadMessages)
            .await?;
        self.manager.list_chats(id, request).await
    }

    pub async fn search_chats(
        &self,
        id: TelegramAccountId,
        request: SearchChatsRequest,
    ) -> Result<ListChatsResponse, TelegramError> {
        self.permissions
            .check(id, TelegramActionKind::ReadMessages)
            .await?;
        self.manager.search_chats(id, request).await
    }

    pub fn mock_simulate_available(&self) -> bool {
        self.manager.mock_factory_present()
    }

    pub async fn get_chat(
        &self,
        id: TelegramAccountId,
        chat_id: TelegramChatId,
    ) -> Result<TelegramChat, TelegramError> {
        self.permissions
            .check(id, TelegramActionKind::ReadMessages)
            .await?;
        self.manager.get_chat(id, chat_id).await
    }

    pub async fn get_messages(
        &self,
        id: TelegramAccountId,
        request: GetMessagesRequest,
    ) -> Result<Vec<TelegramMessage>, TelegramError> {
        self.permissions
            .check(id, TelegramActionKind::ReadMessages)
            .await?;
        self.manager.get_messages(id, request).await
    }

    pub async fn get_message(
        &self,
        id: TelegramAccountId,
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
    ) -> Result<TelegramMessage, TelegramError> {
        self.permissions
            .check(id, TelegramActionKind::ReadMessages)
            .await?;
        self.manager.get_message(id, chat_id, message_id).await
    }

    pub async fn get_account_info(
        &self,
        id: TelegramAccountId,
    ) -> Result<TelegramAccountProfile, TelegramError> {
        self.manager.get_account_info(id).await
    }

    pub async fn set_permissions(
        &self,
        id: TelegramAccountId,
        permissions: TelegramAccountPermissions,
    ) -> Result<(), TelegramError> {
        self.permissions.set(id, permissions).await
    }

    pub async fn list_audit(
        &self,
        account_id: Option<TelegramAccountId>,
        limit: i64,
    ) -> anyhow::Result<Vec<TelegramAuditEvent>> {
        self.audit.list(account_id, limit).await
    }

    pub fn is_boarddo_originated(&self, payload: &tglib::TelegramMessageReceived) -> bool {
        self.outbound.is_boarddo_originated(
            payload.account_id,
            payload.chat_id,
            payload.message_id,
            payload.is_outgoing,
        )
    }

    pub async fn claim_event(&self, event: &TelegramEvent) -> Result<bool, TelegramError> {
        let Some(key) = event.event_key() else {
            return Ok(true);
        };
        self.store
            .claim_idempotency(event.account_id(), &key)
            .await
            .map_err(|e| TelegramError::Account(e.to_string()))
    }

    pub async fn record_scenario_execution(
        &self,
        execution_id: Uuid,
        workflow_id: Uuid,
        account_id: TelegramAccountId,
        event_key: String,
        status: &str,
        error: Option<String>,
    ) -> anyhow::Result<()> {
        self.store
            .insert_scenario_execution(&TelegramScenarioExecution {
                id: Uuid::now_v7(),
                execution_id,
                workflow_id,
                account_id,
                event_key,
                status: status.into(),
                error,
                created_at: chrono::Utc::now(),
            })
            .await
    }

    async fn gated_action<T, F, Fut>(
        &self,
        account_id: TelegramAccountId,
        kind: TelegramActionKind,
        target: Option<String>,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
        op: F,
    ) -> Result<T, TelegramError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, TelegramError>>,
    {
        tracing::info!(
            account_id = %account_id,
            execution_id = ?execution_id,
            scenario_id = ?scenario_id,
            action = kind.as_str(),
            "telegram.action.started"
        );
        if let Err(err) = self.permissions.check(account_id, kind).await {
            let _ = self
                .audit
                .record(
                    account_id,
                    scenario_id,
                    execution_id,
                    "action",
                    kind.as_str(),
                    target.clone(),
                    TelegramAuditStatus::Failed,
                    Some(err.kind().into()),
                )
                .await;
            tracing::warn!(
                account_id = %account_id,
                execution_id = ?execution_id,
                action = kind.as_str(),
                error = %err,
                "telegram.action.failed"
            );
            return Err(err);
        }
        match op().await {
            Ok(value) => {
                let _ = self
                    .audit
                    .record(
                        account_id,
                        scenario_id,
                        execution_id,
                        "action",
                        kind.as_str(),
                        target,
                        TelegramAuditStatus::Success,
                        None,
                    )
                    .await;
                tracing::info!(
                    account_id = %account_id,
                    execution_id = ?execution_id,
                    action = kind.as_str(),
                    "telegram.action.completed"
                );
                Ok(value)
            }
            Err(err) => {
                let _ = self
                    .audit
                    .record(
                        account_id,
                        scenario_id,
                        execution_id,
                        "action",
                        kind.as_str(),
                        target,
                        TelegramAuditStatus::Failed,
                        Some(err.kind().into()),
                    )
                    .await;
                tracing::warn!(
                    account_id = %account_id,
                    execution_id = ?execution_id,
                    action = kind.as_str(),
                    error = %err,
                    "telegram.action.failed"
                );
                Err(err)
            }
        }
    }
}

#[async_trait]
impl TelegramUserGateway for TelegramEngine {
    async fn send_message(
        &self,
        account_id: TelegramAccountId,
        chat_id: TelegramChatId,
        text: String,
        parse_mode: Option<String>,
        reply_to: Option<TelegramMessageId>,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
    ) -> Result<TelegramMessageId, TelegramError> {
        let manager = self.manager.clone();
        let outbound = self.outbound.clone();
        let mode = tglib::ParseMode::parse(parse_mode.as_deref());
        let _guard = outbound.begin_send(account_id);
        let message_id = self
            .gated_action(
                account_id,
                TelegramActionKind::SendMessage,
                Some(format!("chat:{chat_id}")),
                execution_id,
                scenario_id,
                move || async move {
                    manager
                        .send_message(
                            account_id,
                            SendMessageRequest {
                                chat_id,
                                text,
                                parse_mode: mode,
                                reply_to_message_id: reply_to,
                            },
                        )
                        .await
                },
            )
            .await?;
        outbound.note_sent(account_id, chat_id, message_id);
        Ok(message_id)
    }

    async fn get_user(
        &self,
        account_id: TelegramAccountId,
        user_id: TelegramUserId,
    ) -> Result<TelegramUser, TelegramError> {
        self.permissions
            .check(account_id, TelegramActionKind::ReadMessages)
            .await?;
        self.manager.get_user(account_id, user_id).await
    }

    async fn get_chat_members(
        &self,
        account_id: TelegramAccountId,
        request: GetChatMembersRequest,
        max_age: Option<Duration>,
    ) -> Result<ChatMembersResult, TelegramError> {
        // Checked before the cache so a revoked permission takes effect immediately.
        self.permissions
            .check(account_id, TelegramActionKind::ReadMessages)
            .await?;
        let chat_id = request.chat_id;
        let limit = request.limit.unwrap_or(200);
        if let Some(max_age) = max_age {
            if let Some(hit) = self.members_cache.get(account_id, chat_id, limit, max_age) {
                return Ok(hit);
            }
        }
        let response = self.manager.get_chat_members(account_id, request).await?;
        let fetched_at = self.members_cache.put(account_id, chat_id, limit, &response);
        Ok(ChatMembersResult {
            response,
            fetched_at,
            from_cache: false,
        })
    }

    async fn forward_messages(
        &self,
        account_id: TelegramAccountId,
        request: ForwardMessagesRequest,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError> {
        let target = format!("chat:{}->{}", request.from_chat_id, request.to_chat_id);
        let manager = self.manager.clone();
        self.gated_action(
            account_id,
            TelegramActionKind::ForwardMessage,
            Some(target),
            execution_id,
            scenario_id,
            move || async move { manager.forward_messages(account_id, request).await },
        )
        .await
    }

    async fn edit_message(
        &self,
        account_id: TelegramAccountId,
        request: EditMessageRequest,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError> {
        let target = format!("chat:{}", request.chat_id);
        let manager = self.manager.clone();
        self.gated_action(
            account_id,
            TelegramActionKind::EditMessage,
            Some(target),
            execution_id,
            scenario_id,
            move || async move { manager.edit_message(account_id, request).await },
        )
        .await
    }

    async fn delete_messages(
        &self,
        account_id: TelegramAccountId,
        request: DeleteMessagesRequest,
        execution_id: Option<Uuid>,
        scenario_id: Option<Uuid>,
    ) -> Result<(), TelegramError> {
        let target = format!("chat:{}", request.chat_id);
        let manager = self.manager.clone();
        self.gated_action(
            account_id,
            TelegramActionKind::DeleteMessages,
            Some(target),
            execution_id,
            scenario_id,
            move || async move { manager.delete_messages(account_id, request).await },
        )
        .await
    }
}
