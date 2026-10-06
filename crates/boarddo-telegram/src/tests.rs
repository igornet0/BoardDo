use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use uuid::Uuid;

use tglib::{
    DeleteMessagesRequest, EditMessageRequest, ForwardMessagesRequest,
    TelegramChat,
};
use tglib::{
    TelegramAccountId, TelegramAccountPermissions, TelegramAccountStatus, TelegramActionKind,
    TelegramChatId, TelegramError, TelegramEvent, TelegramMessageId, TelegramMessageReceived,
    TELEGRAM_ENGINE_STATUS,
};

use crate::automation::{matches_message_trigger, MessageTriggerFilter};
use crate::events::TelegramEventRouter;
use crate::reconnect::ReconnectBackoff;
use crate::store::TelegramSessionStore;
use crate::{TelegramEngine, TelegramUserGateway};

async fn test_engine() -> (TelegramEngine, PathBuf) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let store = Arc::new(TelegramSessionStore::new(pool));
    store.migrate().await.unwrap();
    let data_dir = std::env::temp_dir().join(format!("boarddo-tg-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&data_dir).unwrap();
    let (engine, _) = TelegramEngine::with_mock(store, data_dir.clone());
    (engine, data_dir)
}

async fn wait_status(engine: &TelegramEngine, id: TelegramAccountId, want: TelegramAccountStatus) {
    for _ in 0..80 {
        if let Some(account) = engine.get_account(id).await.unwrap() {
            if account.status == want {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let got = engine.get_account(id).await.unwrap().map(|a| a.status);
    panic!("timed out waiting for {want:?}, last={got:?}");
}

async fn authorize(engine: &TelegramEngine, id: TelegramAccountId) {
    wait_status(engine, id, TelegramAccountStatus::WaitPhoneNumber).await;
    engine
        .submit_phone(id, "+35840111222".into())
        .await
        .unwrap();
    wait_status(engine, id, TelegramAccountStatus::WaitCode).await;
    engine.submit_code(id, "12345".into()).await.unwrap();
    wait_status(engine, id, TelegramAccountStatus::Ready).await;
}

#[tokio::test]
async fn account_manager_create_start_stop_delete() {
    let (engine, data_dir) = test_engine().await;
    let created = engine.create_account().await.unwrap();
    assert!(!created.consent.is_empty());
    let id = created.account.id;
    assert_eq!(created.account.status, TelegramAccountStatus::WaitPhoneNumber);

    engine.stop_account(id).await.unwrap();
    wait_status(&engine, id, TelegramAccountStatus::Disconnected).await;

    engine.start_account(id).await.unwrap();
    wait_status(&engine, id, TelegramAccountStatus::WaitPhoneNumber).await;

    engine.delete_account(id).await.unwrap();
    assert!(engine.get_account(id).await.unwrap().is_none());
    let session = data_dir.join("telegram").join("accounts").join(id.to_string());
    assert!(!session.exists());
}

#[tokio::test]
async fn authorization_phone_code_password_ready_closed() {
    let (engine, _) = test_engine().await;
    let id = engine.create_account().await.unwrap().account.id;
    wait_status(&engine, id, TelegramAccountStatus::WaitPhoneNumber).await;

    engine
        .submit_phone(id, "+491511234567".into())
        .await
        .unwrap();
    wait_status(&engine, id, TelegramAccountStatus::WaitCode).await;

    let mock = engine.manager.mock_client(id).unwrap();
    mock.set_require_password(true);
    engine.submit_code(id, "11111".into()).await.unwrap();
    wait_status(&engine, id, TelegramAccountStatus::WaitPassword).await;

    engine
        .submit_password(id, "not-logged".into())
        .await
        .unwrap();
    wait_status(&engine, id, TelegramAccountStatus::Ready).await;

    let account = engine.get_account(id).await.unwrap().unwrap();
    let masked = account.phone_masked.clone().unwrap_or_default();
    assert!(masked.contains("***"));
    assert!(!masked.contains("1511234567"));

    engine.disconnect_account(id).await.unwrap();
    wait_status(&engine, id, TelegramAccountStatus::Disconnected).await;
}

#[tokio::test]
async fn authorization_error_on_empty_code() {
    let (engine, _) = test_engine().await;
    let id = engine.create_account().await.unwrap().account.id;
    wait_status(&engine, id, TelegramAccountStatus::WaitPhoneNumber).await;
    engine.submit_phone(id, "+358000".into()).await.unwrap();
    wait_status(&engine, id, TelegramAccountStatus::WaitCode).await;
    let err = engine.submit_code(id, "".into()).await.unwrap_err();
    assert!(matches!(err, TelegramError::InvalidRequest(_)));
}

#[tokio::test]
async fn persistent_session_restore_without_reauth() {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let store = Arc::new(TelegramSessionStore::new(pool));
    store.migrate().await.unwrap();
    let data_dir = std::env::temp_dir().join(format!("boarddo-tg-restore-{}", Uuid::new_v4()));
    let (engine, _) = TelegramEngine::with_mock(store.clone(), data_dir.clone());
    let id = engine.create_account().await.unwrap().account.id;
    authorize(&engine, id).await;
    engine.shutdown_all().await;

    let (engine2, _) = TelegramEngine::with_mock(store, data_dir);
    engine2.restore().await.unwrap();
    wait_status(&engine2, id, TelegramAccountStatus::Ready).await;
    engine2.shutdown_all().await;
}

#[tokio::test]
async fn multi_account_isolation_and_failure() {
    let (engine, _) = test_engine().await;
    let a = engine.create_account().await.unwrap().account.id;
    let b = engine.create_account().await.unwrap().account.id;
    let c = engine.create_account().await.unwrap().account.id;
    authorize(&engine, a).await;
    authorize(&engine, b).await;
    authorize(&engine, c).await;

    engine.manager.mock_client(a).unwrap().force_error("boom").await;
    wait_status(&engine, a, TelegramAccountStatus::Error).await;

    let b_acc = engine.get_account(b).await.unwrap().unwrap();
    let c_acc = engine.get_account(c).await.unwrap().unwrap();
    assert_eq!(b_acc.status, TelegramAccountStatus::Ready);
    assert_eq!(c_acc.status, TelegramAccountStatus::Ready);

    engine
        .manager
        .mock_client(b)
        .unwrap()
        .inject_incoming_message(TelegramChatId(1), TelegramMessageId(9), "hello", None)
        .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        engine.get_account(c).await.unwrap().unwrap().status,
        TelegramAccountStatus::Ready
    );
}

#[tokio::test]
async fn events_carry_account_id() {
    let (engine, _) = test_engine().await;
    let mut rx = engine.subscribe_events();
    let id = engine.create_account().await.unwrap().account.id;
    authorize(&engine, id).await;
    engine
        .manager
        .mock_client(id)
        .unwrap()
        .inject_incoming_message(TelegramChatId(42), TelegramMessageId(7), "BTC LONG", Some(5))
        .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("event timeout")
            .unwrap();
        assert_eq!(event.account_id(), id);
        if let TelegramEvent::MessageReceived(payload) = event {
            assert_eq!(payload.chat_id, TelegramChatId(42));
            assert_eq!(payload.text.as_deref(), Some("BTC LONG"));
            assert!(TelegramEventRouter::ensure_account_id(
                &TelegramEvent::MessageReceived(payload),
                id
            ));
            break;
        }
    }
}

#[tokio::test]
async fn permissions_allow_and_deny() {
    let (engine, _) = test_engine().await;
    let id = engine.create_account().await.unwrap().account.id;
    authorize(&engine, id).await;

    let msg_id = engine
        .send_message(id, TelegramChatId(1), "hi".into(), None, None, None, None)
        .await
        .unwrap();
    assert!(msg_id.0 >= 1);

    engine
        .set_permissions(
            id,
            TelegramAccountPermissions {
                send_messages: false,
                ..TelegramAccountPermissions::default()
            },
        )
        .await
        .unwrap();

    let err = engine
        .send_message(id, TelegramChatId(1), "nope".into(), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, TelegramError::PermissionDenied(_)));
    let audit = engine.list_audit(Some(id), 10).await.unwrap();
    assert!(audit.iter().any(|a| a.error.as_deref() == Some("permission_denied")));
}

#[tokio::test]
async fn actions_forward_edit_delete() {
    let (engine, _) = test_engine().await;
    let id = engine.create_account().await.unwrap().account.id;
    authorize(&engine, id).await;
    engine
        .forward_messages(
            id,
            ForwardMessagesRequest {
                from_chat_id: TelegramChatId(1),
                to_chat_id: TelegramChatId(2),
                message_ids: vec![TelegramMessageId(1)],
            },
            None,
            None,
        )
        .await
        .unwrap();
    engine
        .edit_message(
            id,
            EditMessageRequest {
                chat_id: TelegramChatId(1),
                message_id: TelegramMessageId(1),
                text: "edited".into(),
                parse_mode: Default::default(),
            },
            None,
            None,
        )
        .await
        .unwrap();
    engine
        .delete_messages(
            id,
            DeleteMessagesRequest {
                chat_id: TelegramChatId(1),
                message_ids: vec![TelegramMessageId(1)],
            },
            None,
            None,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn automation_cross_account_and_idempotency() {
    let (engine, _) = test_engine().await;
    let a = engine.create_account().await.unwrap().account.id;
    let b = engine.create_account().await.unwrap().account.id;
    authorize(&engine, a).await;
    authorize(&engine, b).await;

    let payload = TelegramMessageReceived {
        account_id: a,
        chat_id: TelegramChatId(10),
        message_id: TelegramMessageId(3),
        sender_id: None,
        text: Some("signal BTC".into()),
        is_outgoing: false,
        timestamp: Utc::now(),
        reply_to: None,
    };
    let event = TelegramEvent::MessageReceived(payload.clone());
    assert!(engine.claim_event(&event).await.unwrap());
    assert!(!engine.claim_event(&event).await.unwrap());

    let filter_a = MessageTriggerFilter {
        account_id: Some(a),
        text_contains: Some("signal".into()),
        ignore_outgoing: true,
        ..Default::default()
    };
    let filter_b = MessageTriggerFilter {
        account_id: Some(b),
        ignore_outgoing: true,
        ..Default::default()
    };
    assert!(matches_message_trigger(&filter_a, &payload, false));
    assert!(!matches_message_trigger(&filter_b, &payload, false));
    assert!(matches_message_trigger(
        &MessageTriggerFilter::from_node_config(&json!({"account_id": "any", "text_contains": "BTC"})),
        &payload,
        false
    ));

    engine
        .send_message(b, TelegramChatId(99), payload.text.clone().unwrap(), None, None, None, Some(Uuid::now_v7()))
        .await
        .unwrap();
    let sends = engine.manager.mock_client(b).unwrap().recorded_sends();
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].chat_id, TelegramChatId(99));
}

#[tokio::test]
async fn chats_and_messages_list() {
    let (engine, _) = test_engine().await;
    let id = engine.create_account().await.unwrap().account.id;
    authorize(&engine, id).await;
    engine.manager.mock_client(id).unwrap().seed_chat(TelegramChat {
        id: TelegramChatId(5),
        title: "Crypto Signals".into(),
        chat_type: "supergroup".into(),
        username: Some("crypto_signals".into()),
    });
    let chats = engine
        .list_chats(id, tglib::ListChatsRequest { limit: None })
        .await
        .unwrap();
    assert_eq!(chats.chats.len(), 1);
}

#[tokio::test]
async fn shutdown_closes_all_clients() {
    let (engine, _) = test_engine().await;
    let a = engine.create_account().await.unwrap().account.id;
    let b = engine.create_account().await.unwrap().account.id;
    authorize(&engine, a).await;
    authorize(&engine, b).await;
    engine.shutdown_all().await;
    wait_status(&engine, a, TelegramAccountStatus::Disconnected).await;
    wait_status(&engine, b, TelegramAccountStatus::Disconnected).await;
}

#[tokio::test]
async fn reconnect_backoff_is_finite() {
    let mut backoff = ReconnectBackoff::new();
    let mut n = 0;
    while backoff.next_delay().is_some() {
        n += 1;
    }
    assert_eq!(n, 6);
    assert!(backoff.next_delay().is_none());
}

#[tokio::test]
async fn permission_service_direct() {
    let (engine, _) = test_engine().await;
    let id = engine.create_account().await.unwrap().account.id;
    engine
        .permissions
        .check(id, TelegramActionKind::SendMessage)
        .await
        .unwrap();
}

#[test]
fn telegram_engine_status_ready_marker() {
    assert_eq!(TELEGRAM_ENGINE_STATUS, "READY");
    assert_eq!(crate::TELEGRAM_ENGINE_STATUS, "READY");
}

#[tokio::test]
async fn definition_of_done_vertical_slice() {
    let (engine, data_dir) = test_engine().await;
    let a = engine.create_account().await.unwrap().account.id;
    let b = engine.create_account().await.unwrap().account.id;
    authorize(&engine, a).await;
    authorize(&engine, b).await;

    engine.shutdown_all().await;
    let store = engine.store.clone();
    let (engine2, _) = TelegramEngine::with_mock(store, data_dir);
    engine2.restore().await.unwrap();
    wait_status(&engine2, a, TelegramAccountStatus::Ready).await;
    wait_status(&engine2, b, TelegramAccountStatus::Ready).await;

    let mut rx = engine2.subscribe_events();
    engine2
        .manager
        .mock_client(a)
        .unwrap()
        .inject_incoming_message(TelegramChatId(1), TelegramMessageId(1), "BTC", None)
        .await;
    let mut saw = false;
    for _ in 0..20 {
        if let Ok(Ok(TelegramEvent::MessageReceived(p))) =
            tokio::time::timeout(Duration::from_millis(100), rx.recv()).await
        {
            assert_eq!(p.account_id, a);
            saw = true;
            break;
        }
    }
    assert!(saw, "expected MessageReceived");
    engine2
        .send_message(b, TelegramChatId(2), "forwarded".into(), None, None, None, None)
        .await
        .unwrap();
    engine2.manager.mock_client(a).unwrap().force_error("down").await;
    wait_status(&engine2, a, TelegramAccountStatus::Error).await;
    assert_eq!(
        engine2.get_account(b).await.unwrap().unwrap().status,
        TelegramAccountStatus::Ready
    );
    println!("TELEGRAM_ENGINE_STATUS=READY");
    engine2.shutdown_all().await;
}

/// Sandbox: Echo / @ai reply must not re-fire on the account's own send.
#[tokio::test]
async fn sandbox_echo_and_ai_do_not_loop_on_outgoing() {
    let (engine, _) = test_engine().await;
    let account = engine.create_account().await.unwrap().account.id;
    authorize(&engine, account).await;
    let mock = engine.manager.mock_client(account).unwrap();

    let echo_filter = MessageTriggerFilter::from_node_config(&json!({
        "account_id": "any",
        "text_contains": "",
        "ignore_outgoing": true
    }));
    let ai_filter = MessageTriggerFilter::from_node_config(&json!({
        "account_id": "any",
        "text_contains": "@ai",
        "ignore_outgoing": true
    }));

    let mut rx = engine.subscribe_events();
    mock.inject_incoming_message(
        TelegramChatId(42),
        TelegramMessageId(100),
        "@ai how many people live in Russia?",
        Some(777),
    )
    .await;

    let incoming = wait_message(&mut rx).await;
    assert!(!incoming.is_outgoing);
    assert!(matches_message_trigger(&echo_filter, &incoming, false));
    assert!(matches_message_trigger(&ai_filter, &incoming, false));

    // Scenario reply (echo or AI) — mock mirrors TDLib and emits outgoing update.
    engine
        .send_message(
            account,
            TelegramChatId(42),
            "About **146 million** people.".into(),
            Some("markdown".into()),
            None,
            None,
            Some(Uuid::now_v7()),
        )
        .await
        .unwrap();

    let outgoing = wait_message(&mut rx).await;
    assert!(outgoing.is_outgoing);
    assert_eq!(
        outgoing.text.as_deref(),
        Some("About **146 million** people.")
    );
    let from_boarddo = engine.is_boarddo_originated(&outgoing);
    assert!(from_boarddo, "BoardDo send must be tracked as outbound");
    assert!(
        !matches_message_trigger(&echo_filter, &outgoing, from_boarddo),
        "echo must not re-trigger on BoardDo reply"
    );
    assert!(
        !matches_message_trigger(&ai_filter, &outgoing, from_boarddo),
        "@ai must not re-trigger on BoardDo reply"
    );

    // User-typed outgoing (same account, phone/desktop) must still match.
    let user_typed = tglib::TelegramMessageReceived {
        account_id: account,
        chat_id: TelegramChatId(42),
        message_id: TelegramMessageId(999_001),
        sender_id: None,
        text: Some("@ai from my phone".into()),
        is_outgoing: true,
        timestamp: chrono::Utc::now(),
        reply_to: None,
    };
    assert!(
        matches_message_trigger(&ai_filter, &user_typed, engine.is_boarddo_originated(&user_typed)),
        "user-typed @ai from same account must trigger"
    );

    // Even an outgoing reply that still contains @ai must be ignored by default.
    engine
        .send_message(
            account,
            TelegramChatId(42),
            "@ai accidental echo".into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let outgoing_ai = wait_message(&mut rx).await;
    assert!(outgoing_ai.is_outgoing);
    assert!(!matches_message_trigger(
        &ai_filter,
        &outgoing_ai,
        engine.is_boarddo_originated(&outgoing_ai)
    ));

    let sends = mock.recorded_sends();
    assert_eq!(sends.len(), 2);
    engine.shutdown_all().await;
}

async fn wait_message(
    rx: &mut tokio::sync::broadcast::Receiver<TelegramEvent>,
) -> TelegramMessageReceived {
    for _ in 0..40 {
        if let Ok(Ok(TelegramEvent::MessageReceived(p))) =
            tokio::time::timeout(Duration::from_millis(100), rx.recv()).await
        {
            return p;
        }
    }
    panic!("expected MessageReceived event");
}

#[tokio::test]
async fn reply_event_carries_target_and_message_is_fetchable() {
    let (engine, _) = test_engine().await;
    let mut rx = engine.subscribe_events();
    let id = engine.create_account().await.unwrap().account.id;
    authorize(&engine, id).await;
    let mock = engine.manager.mock_client(id).unwrap();
    mock.seed_message(tglib::TelegramMessage {
        chat_id: TelegramChatId(42),
        message_id: TelegramMessageId(100),
        sender_id: Some(tglib::TelegramUserId(5)),
        text: Some("Минфин увеличит покупку валюты".into()),
        content_kind: "text".into(),
        timestamp: Utc::now(),
    });
    mock.inject_reply(
        TelegramChatId(42),
        TelegramMessageId(101),
        "@ai проанализируй его",
        Some(9),
        tglib::TelegramReplyTo {
            chat_id: TelegramChatId(42),
            message_id: TelegramMessageId(100),
            quote: Some("покупку валюты".into()),
        },
    )
    .await;

    let payload = wait_message(&mut rx).await;
    let reply = payload.reply_to.expect("reply_to");
    assert_eq!(reply.message_id, TelegramMessageId(100));
    assert_eq!(reply.quote.as_deref(), Some("покупку валюты"));

    let original = engine
        .get_message(id, reply.chat_id, reply.message_id)
        .await
        .unwrap();
    assert_eq!(original.text.as_deref(), Some("Минфин увеличит покупку валюты"));
    assert!(engine
        .get_message(id, TelegramChatId(42), TelegramMessageId(999))
        .await
        .is_err());
    engine.shutdown_all().await;
}

#[tokio::test]
async fn chat_members_are_cached_and_permission_still_checked() {
    let (engine, _) = test_engine().await;
    let id = engine.create_account().await.unwrap().account.id;
    authorize(&engine, id).await;
    let mock = engine.manager.mock_client(id).unwrap();
    let member = |uid: i64, name: &str| tglib::TelegramChatMember {
        user: tglib::TelegramUser {
            id: tglib::TelegramUserId(uid),
            first_name: name.into(),
            last_name: None,
            username: None,
            is_bot: false,
        },
        status: "member".into(),
    };
    let chat = TelegramChatId(-77);
    mock.seed_chat_members(chat, vec![member(1, "Ann")]);
    let req = || tglib::GetChatMembersRequest { chat_id: chat, limit: None };
    let day = Some(Duration::from_secs(24 * 3600));

    let first = engine.get_chat_members(id, req(), day).await.unwrap();
    assert!(!first.from_cache);

    // Membership changes, but the cached copy is still within the TTL.
    mock.seed_chat_members(chat, vec![member(1, "Ann"), member(2, "Bob")]);
    let cached = engine.get_chat_members(id, req(), day).await.unwrap();
    assert!(cached.from_cache);
    assert_eq!(cached.response.members.len(), 1);

    // No TTL → fresh fetch, which also refreshes the cache.
    let fresh = engine.get_chat_members(id, req(), None).await.unwrap();
    assert!(!fresh.from_cache);
    assert_eq!(fresh.response.members.len(), 2);

    engine
        .set_permissions(
            id,
            TelegramAccountPermissions {
                read_messages: false,
                ..TelegramAccountPermissions::default()
            },
        )
        .await
        .unwrap();
    assert!(engine.get_chat_members(id, req(), day).await.is_err());
    engine.shutdown_all().await;
}
