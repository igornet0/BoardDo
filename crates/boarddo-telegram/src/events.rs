use chrono::Utc;

use tglib::TelegramClientUpdate;
use tglib::{
    TelegramAccountId, TelegramAuthorizationChanged, TelegramChatId, TelegramChatUpdated,
    TelegramConnectionChanged, TelegramEvent, TelegramMessageDeleted, TelegramMessageEdited,
    TelegramMessageId, TelegramMessageReceived, TelegramUserId, TelegramUserUpdated,
};

pub struct TelegramEventRouter;

impl TelegramEventRouter {
    pub fn map(account_id: TelegramAccountId, update: TelegramClientUpdate) -> Option<TelegramEvent> {
        let timestamp = Utc::now();
        match update {
            TelegramClientUpdate::MessageReceived {
                chat_id,
                message_id,
                sender_id,
                text,
                is_outgoing,
                timestamp: ts,
            } => Some(TelegramEvent::MessageReceived(TelegramMessageReceived {
                account_id,
                chat_id,
                message_id,
                sender_id: sender_id.map(TelegramUserId),
                text,
                is_outgoing,
                timestamp: ts,
            })),
            TelegramClientUpdate::MessageEdited {
                chat_id,
                message_id,
                text,
                timestamp: ts,
            } => Some(TelegramEvent::MessageEdited(TelegramMessageEdited {
                account_id,
                chat_id,
                message_id,
                text,
                timestamp: ts,
            })),
            TelegramClientUpdate::MessageDeleted {
                chat_id,
                message_id,
                timestamp: ts,
            } => Some(TelegramEvent::MessageDeleted(TelegramMessageDeleted {
                account_id,
                chat_id,
                message_id,
                timestamp: ts,
            })),
            TelegramClientUpdate::ChatUpdated {
                chat_id,
                title,
                timestamp: ts,
            } => Some(TelegramEvent::ChatUpdated(TelegramChatUpdated {
                account_id,
                chat_id,
                title,
                timestamp: ts,
            })),
            TelegramClientUpdate::AuthState { status, error } => {
                Some(TelegramEvent::AuthorizationChanged(
                    TelegramAuthorizationChanged {
                        account_id,
                        status,
                        error,
                        timestamp,
                    },
                ))
            }
            TelegramClientUpdate::ConnectionChanged { connected } => {
                Some(TelegramEvent::ConnectionChanged(TelegramConnectionChanged {
                    account_id,
                    connected,
                    timestamp,
                }))
            }
            TelegramClientUpdate::Closed => Some(TelegramEvent::AuthorizationChanged(
                TelegramAuthorizationChanged {
                    account_id,
                    status: tglib::TelegramAccountStatus::Disconnected,
                    error: None,
                    timestamp,
                },
            )),
            TelegramClientUpdate::Error { .. } | TelegramClientUpdate::Profile { .. } => None,
        }
    }

    pub fn ensure_account_id(event: &TelegramEvent, expected: TelegramAccountId) -> bool {
        event.account_id() == expected
    }

    pub fn chat_updated_placeholder(account_id: TelegramAccountId, chat_id: TelegramChatId) -> TelegramEvent {
        TelegramEvent::ChatUpdated(TelegramChatUpdated {
            account_id,
            chat_id,
            title: None,
            timestamp: Utc::now(),
        })
    }

    pub fn user_updated(
        account_id: TelegramAccountId,
        user_id: TelegramUserId,
        username: Option<String>,
    ) -> TelegramEvent {
        TelegramEvent::UserUpdated(TelegramUserUpdated {
            account_id,
            user_id,
            username,
            timestamp: Utc::now(),
        })
    }

    pub fn message_id_key(account_id: TelegramAccountId, chat_id: TelegramChatId, message_id: TelegramMessageId) -> String {
        format!("{account_id}|{chat_id}|{message_id}|message_received")
    }
}
