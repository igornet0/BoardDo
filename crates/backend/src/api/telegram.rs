//! Telegram user-account HTTP API. Frontend never talks to TDLib.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use tglib::{
    GetMessagesRequest, ListChatsRequest, ListChatsResponse, SearchChatsRequest, TelegramChat,
    TelegramMessage,
};
use tglib::{
    TelegramAccount, TelegramAccountCreated, TelegramAccountId, TelegramAccountPermissions,
    TelegramAccountStatusView, TelegramAuditEvent, TelegramChatId, TelegramError,
};
use boarddo_telegram::TelegramUserGateway;
use serde::Deserialize;
use uuid::Uuid;

use crate::state::SharedState;

fn map_err(err: TelegramError) -> (StatusCode, String) {
    tracing::warn!(error = %err, kind = err.kind(), "telegram.api.error");
    let status = match err {
        TelegramError::AccountNotFound => StatusCode::NOT_FOUND,
        TelegramError::PermissionDenied(_) => StatusCode::FORBIDDEN,
        TelegramError::InvalidRequest(_) | TelegramError::AuthorizationRequired => {
            StatusCode::BAD_REQUEST
        }
        TelegramError::NotReady => StatusCode::CONFLICT,
        TelegramError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::BAD_GATEWAY,
    };
    (status, err.to_string())
}

fn internal(err: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn parse_id(id: Uuid) -> TelegramAccountId {
    TelegramAccountId(id)
}

pub async fn list_accounts(
    State(state): State<SharedState>,
) -> Result<Json<Vec<TelegramAccount>>, (StatusCode, String)> {
    state
        .telegram
        .list_accounts()
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn create_account(
    State(state): State<SharedState>,
) -> Result<(StatusCode, Json<TelegramAccountCreated>), (StatusCode, String)> {
    let created = state.telegram.create_account().await.map_err(map_err)?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn get_account(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TelegramAccount>, (StatusCode, String)> {
    match state
        .telegram
        .get_account(parse_id(id))
        .await
        .map_err(map_err)?
    {
        Some(account) => Ok(Json(account)),
        None => Err((StatusCode::NOT_FOUND, "account not found".into())),
    }
}

pub async fn delete_account(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .delete_account(parse_id(id))
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn start_account(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .start_account(parse_id(id))
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn stop_account(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .stop_account(parse_id(id))
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn connect_account(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .connect_account(parse_id(id))
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn disconnect_account(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .disconnect_account(parse_id(id))
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn account_status(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TelegramAccountStatusView>, (StatusCode, String)> {
    match state
        .telegram
        .get_account(parse_id(id))
        .await
        .map_err(map_err)?
    {
        Some(account) => Ok(Json(TelegramAccountStatusView { account })),
        None => Err((StatusCode::NOT_FOUND, "account not found".into())),
    }
}

#[derive(Deserialize)]
pub struct PhoneRequest {
    pub phone: String,
}

#[derive(Deserialize)]
pub struct CodeRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct PasswordRequest {
    pub password: String,
}

pub async fn submit_phone(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PhoneRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .submit_phone(parse_id(id), req.phone)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn submit_code(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CodeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .submit_code(parse_id(id), req.code)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn submit_password(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PasswordRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .submit_password(parse_id(id), req.password)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_chats(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ListChatsResponse>, (StatusCode, String)> {
    state
        .telegram
        .list_chats(parse_id(id), ListChatsRequest { limit: Some(100) })
        .await
        .map(Json)
        .map_err(map_err)
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<u32>,
}

pub async fn search_chats(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<ListChatsResponse>, (StatusCode, String)> {
    state
        .telegram
        .search_chats(
            parse_id(id),
            SearchChatsRequest {
                query: q.q,
                limit: q.limit.or(Some(20)),
            },
        )
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn capabilities(State(state): State<SharedState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "mock_simulate": state.telegram.mock_simulate_available(),
        "tdlib": cfg!(feature = "tdlib"),
    }))
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    pub chat_id: i64,
}

pub async fn list_messages(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(q): Query<MessagesQuery>,
) -> Result<Json<Vec<TelegramMessage>>, (StatusCode, String)> {
    state
        .telegram
        .get_messages(
            parse_id(id),
            GetMessagesRequest {
                chat_id: TelegramChatId(q.chat_id),
                limit: Some(50),
            },
        )
        .await
        .map(Json)
        .map_err(map_err)
}

#[derive(Deserialize)]
pub struct SendRequest {
    pub chat_id: i64,
    pub text: String,
    #[serde(default)]
    pub parse_mode: Option<String>,
}

pub async fn send_message(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<SendRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let message_id = state
        .telegram
        .send_message(
            parse_id(id),
            TelegramChatId(req.chat_id),
            req.text,
            req.parse_mode,
            None,
            None,
        )
        .await
        .map_err(map_err)?;
    Ok(Json(serde_json::json!({ "message_id": message_id.0 })))
}

pub async fn update_permissions(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(permissions): Json<TelegramAccountPermissions>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .telegram
        .set_permissions(parse_id(id), permissions)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub account_id: Option<Uuid>,
    pub limit: Option<i64>,
}

pub async fn list_audit(
    State(state): State<SharedState>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<TelegramAuditEvent>>, (StatusCode, String)> {
    state
        .telegram
        .list_audit(q.account_id.map(TelegramAccountId), q.limit.unwrap_or(100))
        .await
        .map(Json)
        .map_err(internal)
}

#[derive(Deserialize)]
pub struct SimulateMessageRequest {
    pub chat_id: i64,
    pub message_id: i64,
    pub text: String,
}

pub async fn simulate_message(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(req): Json<SimulateMessageRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let Some(mock) = state.telegram.manager.mock_client(parse_id(id)) else {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            "simulate is only available with MockTelegramClient (default build without tdlib feature)"
                .into(),
        ));
    };
    mock.inject_incoming_message(
        TelegramChatId(req.chat_id),
        tglib::TelegramMessageId(req.message_id),
        req.text,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_chat(
    State(state): State<SharedState>,
    Path((id, chat_id)): Path<(Uuid, i64)>,
) -> Result<Json<TelegramChat>, (StatusCode, String)> {
    state
        .telegram
        .get_chat(parse_id(id), TelegramChatId(chat_id))
        .await
        .map(Json)
        .map_err(map_err)
}
