//! Telegram user-account nodes + mock gateway.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use boarddo_shared::{Edge, Node, Position, WorkflowDefinition, type_ids};
    use tglib::ForwardMessagesRequest;
    use tglib::{
        TelegramAccountId, TelegramChatId, TelegramError, TelegramMessageId,
    };
    use boarddo_telegram::TelegramUserGateway;
    use serde_json::json;
    use uuid::Uuid;

    use crate::engine::Engine;

    struct RecordingGateway {
        sends: std::sync::Mutex<Vec<(TelegramAccountId, TelegramChatId, String)>>,
    }

    #[async_trait::async_trait]
    impl TelegramUserGateway for RecordingGateway {
        async fn send_message(
            &self,
            account_id: TelegramAccountId,
            chat_id: TelegramChatId,
            text: String,
            _parse_mode: Option<String>,
            _execution_id: Option<Uuid>,
            _scenario_id: Option<Uuid>,
        ) -> Result<TelegramMessageId, TelegramError> {
            self.sends.lock().unwrap().push((account_id, chat_id, text));
            Ok(TelegramMessageId(1))
        }

        async fn forward_messages(
            &self,
            _account_id: TelegramAccountId,
            _request: ForwardMessagesRequest,
            _execution_id: Option<Uuid>,
            _scenario_id: Option<Uuid>,
        ) -> Result<(), TelegramError> {
            Ok(())
        }

        async fn edit_message(
            &self,
            _account_id: TelegramAccountId,
            _request: tglib::EditMessageRequest,
            _execution_id: Option<Uuid>,
            _scenario_id: Option<Uuid>,
        ) -> Result<(), TelegramError> {
            Ok(())
        }

        async fn delete_messages(
            &self,
            _account_id: TelegramAccountId,
            _request: tglib::DeleteMessagesRequest,
            _execution_id: Option<Uuid>,
            _scenario_id: Option<Uuid>,
        ) -> Result<(), TelegramError> {
            Ok(())
        }
    }

    fn node(id: &str, type_id: &str, config: serde_json::Value) -> Node {
        Node {
            id: id.into(),
            type_id: type_id.into(),
            category: None,
            position: Position::default(),
            config,
        }
    }

    fn edge(id: &str, source: &str, target: &str) -> Edge {
        Edge {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            source_port: None,
            target_port: None,
        }
    }

    #[tokio::test]
    async fn user_send_message_uses_explicit_account() {
        let account = TelegramAccountId::new();
        let gw = Arc::new(RecordingGateway {
            sends: std::sync::Mutex::new(Vec::new()),
        });
        let engine = Engine::new().with_telegram_user(gw.clone());
        let definition = WorkflowDefinition {
            nodes: vec![
                node(
                    "t1",
                    type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
                    json!({"account_id": "any"}),
                ),
                node(
                    "s1",
                    type_ids::TELEGRAM_USER_SEND_MESSAGE,
                    json!({
                        "account_id": account.to_string(),
                        "chat_id": "99",
                        "text": "hello"
                    }),
                ),
            ],
            edges: vec![edge("e1", "t1", "s1")],
        };

        let result = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({
                    "account_id": account.to_string(),
                    "text": "BTC",
                    "chat_id": 1
                }),
                Some("telegram.user"),
                None,
            )
            .await;

        assert!(result.error.is_none(), "{:?}", result.error);
        let sends = gw.sends.lock().unwrap();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0, account);
        assert_eq!(sends[0].1, TelegramChatId(99));
    }

    #[tokio::test]
    async fn sandbox_run_skips_telegram_send() {
        let gw = Arc::new(RecordingGateway {
            sends: std::sync::Mutex::new(Vec::new()),
        });
        let engine = Engine::new().with_telegram_user(gw.clone());
        let definition = WorkflowDefinition {
            nodes: vec![
                node(
                    "incoming",
                    type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
                    json!({"account_id": "any"}),
                ),
                node(
                    "reply",
                    type_ids::TELEGRAM_USER_SEND_MESSAGE,
                    json!({
                        "account_id": "{{trigger.account_id}}",
                        "chat_id": "{{trigger.chat_id}}",
                        "text": "echo {{trigger.text}}"
                    }),
                ),
            ],
            edges: vec![edge("e1", "incoming", "reply")],
        };

        let result = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({
                    "sandbox": true,
                    "text": "новости ИИ на сегодня",
                    "chat_id": "sandbox",
                    "account_id": "sandbox"
                }),
                Some("manual"),
                None,
            )
            .await;

        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(gw.sends.lock().unwrap().is_empty());
        let reply = result.context.nodes.get("reply").expect("reply output");
        assert_eq!(reply["sandbox"], true);
        assert!(reply["text"].as_str().unwrap().contains("новости ИИ"));
    }
}
