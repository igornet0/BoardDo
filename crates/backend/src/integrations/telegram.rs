//! Telegram Bot API client used by telegram.* nodes.
//!
//! Credentials come only from ConnectionProvider — this module never touches SecretStore.
//! HTTP lives in `tglib::bot`; this wrapper keeps Connection-based construction.

use serde_json::Value;
use tglib::bot::{BotClient, BotError};

pub use tglib::bot::{extract_bot_token, BotError as TelegramError};

pub struct TelegramClient {
    inner: BotClient,
}

impl TelegramClient {
    pub fn from_resolved(config: &Value, credentials: &Value) -> Result<Self, TelegramError> {
        let bot_token = extract_bot_token(credentials).ok_or_else(|| {
            BotError::Authentication("connection secret missing bot_token".into())
        })?;

        if bot_token.is_empty() {
            return Err(BotError::Authentication("empty bot_token".into()));
        }

        let mut inner = BotClient::new(bot_token)?;
        if let Some(base) = config.get("api_base").and_then(Value::as_str) {
            let base = base.trim();
            if !base.is_empty() {
                inner = inner.with_api_base(base);
            }
        }

        Ok(Self { inner })
    }

    pub async fn send_message(
        &self,
        chat_id: &str,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<Value, TelegramError> {
        self.inner.send_message(chat_id, text, parse_mode).await
    }

    pub async fn send_photo(
        &self,
        chat_id: &str,
        photo: &str,
        caption: Option<&str>,
    ) -> Result<Value, TelegramError> {
        self.inner.send_photo(chat_id, photo, caption).await
    }

    pub async fn send_document(
        &self,
        chat_id: &str,
        document: &str,
        caption: Option<&str>,
    ) -> Result<Value, TelegramError> {
        self.inner.send_document(chat_id, document, caption).await
    }

    /// Lightweight getMe for connection test.
    pub async fn get_me(&self) -> Result<Value, TelegramError> {
        self.inner.get_me().await
    }
}
