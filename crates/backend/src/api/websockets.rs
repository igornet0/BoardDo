use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use tglib::TelegramEvent;
use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;

use crate::state::SharedState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = BroadcastStream::new(state.subscribe());

    let mut send_task = tokio::spawn(async move {
        while let Some(item) = events.next().await {
            match item {
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(payload) => {
                        if sender.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to serialize execution event");
                    }
                },
                Err(err) => {
                    tracing::debug!(error = %err, "broadcast lag");
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
            // Client messages ignored in Phase 1 (ping handled by axum).
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

pub async fn telegram_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_telegram_socket(socket, state))
}

pub async fn goals_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_goals_socket(socket, state))
}

/// UI envelope. Auth states are pushed as `auth_state_changed` so the panel
/// does not poll REST.
pub fn telegram_ws_json(event: &TelegramEvent) -> serde_json::Value {
    match event {
        TelegramEvent::AuthorizationChanged(payload) => {
            let mut body = json!({
                "type": "auth_state_changed",
                "account_id": payload.account_id,
                "state": payload.status.as_str(),
            });
            if let Some(error) = &payload.error {
                body["error"] = json!(error);
            }
            body
        }
        other => serde_json::to_value(other).unwrap_or_else(|_| json!({"type": "error"})),
    }
}

async fn handle_telegram_socket(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = BroadcastStream::new(state.telegram.subscribe_events());

    let mut send_task = tokio::spawn(async move {
        while let Some(item) = events.next().await {
            match item {
                Ok(event) => {
                    let payload = telegram_ws_json(&event).to_string();
                    if sender.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                Err(err) => {
                    tracing::debug!(error = %err, "telegram broadcast lag");
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

async fn handle_goals_socket(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = BroadcastStream::new(state.subscribe_goal());

    let mut send_task = tokio::spawn(async move {
        while let Some(item) = events.next().await {
            match item {
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(payload) => {
                        if sender.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to serialize goal event");
                    }
                },
                Err(err) => tracing::debug!(error = %err, "goal broadcast lag"),
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if matches!(msg, Message::Close(_)) {
                break;
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tglib::{
        TelegramAccountId, TelegramAccountStatus, TelegramAuthorizationChanged,
    };
    use chrono::Utc;

    #[test]
    fn auth_state_changed_envelope() {
        let event = TelegramEvent::AuthorizationChanged(TelegramAuthorizationChanged {
            account_id: TelegramAccountId::new(),
            status: TelegramAccountStatus::WaitCode,
            error: None,
            timestamp: Utc::now(),
        });
        let value = telegram_ws_json(&event);
        assert_eq!(value["type"], "auth_state_changed");
        assert_eq!(value["state"], "wait_code");
        assert!(value.get("account_id").is_some());
    }
}
