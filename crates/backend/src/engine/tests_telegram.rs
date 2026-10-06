//! Telegram user-account nodes + mock gateway.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use boarddo_shared::{Edge, Node, Position, WorkflowDefinition, type_ids};
    use tglib::{
        ChatMembersResponse, ForwardMessagesRequest, GetChatMembersRequest, TelegramChatMember,
        TelegramUser, TelegramUserId,
    };
    use tglib::{
        TelegramAccountId, TelegramChatId, TelegramError, TelegramMessageId,
    };
    use boarddo_telegram::{ChatMembersResult, TelegramUserGateway};
    use serde_json::json;
    use uuid::Uuid;

    use crate::engine::Engine;

    struct RecordingGateway {
        sends: std::sync::Mutex<Vec<(TelegramAccountId, TelegramChatId, String)>>,
        replies: std::sync::Mutex<Vec<Option<TelegramMessageId>>>,
        member_ttls: std::sync::Mutex<Vec<Option<std::time::Duration>>>,
    }

    impl RecordingGateway {
        fn new() -> Self {
            Self {
                sends: std::sync::Mutex::new(Vec::new()),
                replies: std::sync::Mutex::new(Vec::new()),
                member_ttls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    fn tg_user(id: i64, name: &str, username: Option<&str>) -> TelegramUser {
        TelegramUser {
            id: TelegramUserId(id),
            first_name: name.into(),
            last_name: None,
            username: username.map(str::to_string),
            is_bot: false,
        }
    }

    #[async_trait::async_trait]
    impl TelegramUserGateway for RecordingGateway {
        async fn send_message(
            &self,
            account_id: TelegramAccountId,
            chat_id: TelegramChatId,
            text: String,
            _parse_mode: Option<String>,
            reply_to: Option<TelegramMessageId>,
            _execution_id: Option<Uuid>,
            _scenario_id: Option<Uuid>,
        ) -> Result<TelegramMessageId, TelegramError> {
            self.sends.lock().unwrap().push((account_id, chat_id, text));
            self.replies.lock().unwrap().push(reply_to);
            Ok(TelegramMessageId(1))
        }

        async fn get_user(
            &self,
            _account_id: TelegramAccountId,
            user_id: TelegramUserId,
        ) -> Result<TelegramUser, TelegramError> {
            Ok(tg_user(user_id.0, "User", None))
        }

        async fn get_chat_members(
            &self,
            _account_id: TelegramAccountId,
            request: GetChatMembersRequest,
            max_age: Option<std::time::Duration>,
        ) -> Result<ChatMembersResult, TelegramError> {
            self.member_ttls.lock().unwrap().push(max_age);
            if request.chat_id.0 == 404 {
                return Err(TelegramError::InvalidRequest("member list hidden".into()));
            }
            let response = ChatMembersResponse {
                total_count: 3,
                members: vec![
                    TelegramChatMember {
                        user: tg_user(1, "Igor", Some("igornet0")),
                        status: "creator".into(),
                    },
                    TelegramChatMember {
                        user: tg_user(2, "Anna", Some("anna_k")),
                        status: "member".into(),
                    },
                ],
            };
            Ok(ChatMembersResult {
                response,
                fetched_at: chrono::Utc::now(),
                from_cache: false,
            })
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
        let gw = Arc::new(RecordingGateway::new());
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
        let gw = Arc::new(RecordingGateway::new());
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

    #[tokio::test]
    async fn user_send_message_replies_to_trigger_message() {
        let account = TelegramAccountId::new();
        let gw = Arc::new(RecordingGateway::new());
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
                        "account_id": "{{trigger.account_id}}",
                        "chat_id": "{{trigger.chat_id}}",
                        "text": "pong",
                        "reply_to_message_id": "{{trigger.message_id}}"
                    }),
                ),
                node(
                    "s2",
                    type_ids::TELEGRAM_USER_SEND_MESSAGE,
                    json!({
                        "account_id": "{{trigger.account_id}}",
                        "chat_id": "{{trigger.chat_id}}",
                        "text": "no reply",
                        "reply_to_message_id": "{{trigger.missing}}"
                    }),
                ),
            ],
            edges: vec![edge("e1", "t1", "s1"), edge("e2", "s1", "s2")],
        };
        let result = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({
                    "account_id": account.to_string(),
                    "chat_id": 5,
                    "message_id": 389040570368i64,
                    "text": "@ai ping"
                }),
                Some("telegram.user"),
                None,
            )
            .await;
        assert!(result.error.is_none(), "{:?}", result.error);
        let replies = gw.replies.lock().unwrap();
        assert_eq!(*replies, vec![Some(TelegramMessageId(389040570368)), None]);
    }

    #[tokio::test]
    async fn get_chat_members_outputs_list_and_soft_fails() {
        let account = TelegramAccountId::new();
        let gw = Arc::new(RecordingGateway::new());
        let engine = Engine::new().with_telegram_user(gw.clone());
        let definition = WorkflowDefinition {
            nodes: vec![
                node(
                    "t1",
                    type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
                    json!({"account_id": "any"}),
                ),
                node(
                    "members",
                    type_ids::TELEGRAM_USER_GET_CHAT_MEMBERS,
                    json!({
                        "account_id": "{{trigger.account_id}}",
                        "chat_id": "{{trigger.chat_id}}"
                    }),
                ),
                node(
                    "hidden",
                    type_ids::TELEGRAM_USER_GET_CHAT_MEMBERS,
                    json!({
                        "account_id": "{{trigger.account_id}}",
                        "chat_id": "404",
                        "continue_on_error": true,
                        "cache_ttl_hours": 0
                    }),
                ),
                node(
                    "s1",
                    type_ids::TELEGRAM_USER_SEND_MESSAGE,
                    json!({
                        "account_id": "{{trigger.account_id}}",
                        "chat_id": "{{trigger.chat_id}}",
                        "text": "{{nodes.members.output.context}}|{{nodes.hidden.output.context}}"
                    }),
                ),
            ],
            edges: vec![
                edge("e1", "t1", "members"),
                edge("e2", "members", "hidden"),
                edge("e3", "hidden", "s1"),
            ],
        };
        let result = engine
            .execute_with_source(
                Uuid::now_v7(),
                Uuid::now_v7(),
                1,
                &definition,
                json!({ "account_id": account.to_string(), "chat_id": -100, "text": "@ai" }),
                Some("telegram.user"),
                None,
            )
            .await;
        assert!(result.error.is_none(), "{:?}", result.error);
        let sends = gw.sends.lock().unwrap();
        assert_eq!(
            sends[0].2,
            "Chat members (2 of 3):\n- Igor (@igornet0) — creator\n- Anna (@anna_k)|"
        );
        assert_eq!(
            *gw.member_ttls.lock().unwrap(),
            vec![Some(std::time::Duration::from_secs(24 * 3600)), None]
        );
    }
}
