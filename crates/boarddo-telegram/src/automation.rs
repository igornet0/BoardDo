use serde_json::Value;

use tglib::{TelegramAccountId, TelegramChatId, TelegramMessageReceived};

/// Filter for `trigger.telegram.user.message_received`.
#[derive(Clone, Debug)]
pub struct MessageTriggerFilter {
    /// `None` means any connected Telegram account.
    pub account_id: Option<TelegramAccountId>,
    /// If set, only this chat triggers (legacy single allow).
    pub chat_id: Option<TelegramChatId>,
    /// If non-empty, only these chats trigger (allow-list).
    pub only_chat_ids: Vec<TelegramChatId>,
    /// Chats that never trigger (block-list). Checked after allow filters.
    pub ignore_chat_ids: Vec<TelegramChatId>,
    pub text_contains: Option<String>,
    /// When true (default), ignore messages the connected account sent.
    /// Prevents echo / AI-reply loops. Set false for Saved Messages self-triggers
    /// (BoardDo-sent replies are still suppressed separately).
    pub ignore_outgoing: bool,
}

impl Default for MessageTriggerFilter {
    fn default() -> Self {
        Self {
            account_id: None,
            chat_id: None,
            only_chat_ids: Vec::new(),
            ignore_chat_ids: Vec::new(),
            text_contains: None,
            ignore_outgoing: true,
        }
    }
}

impl MessageTriggerFilter {
    pub fn from_node_config(config: &Value) -> Self {
        let account_id = config
            .get("account_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && *s != "any")
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .map(TelegramAccountId);
        let chat_id = config
            .get("chat_id")
            .and_then(parse_one_chat_id)
            .map(TelegramChatId);
        let only_chat_ids = parse_chat_id_list(
            config
                .get("only_chat_ids")
                .or_else(|| config.get("chat_ids")),
        );
        let ignore_chat_ids = parse_chat_id_list(
            config
                .get("ignore_chat_ids")
                .or_else(|| config.get("ignore_chats")),
        );
        let text_contains = config
            .get("text_contains")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ignore_outgoing = config
            .get("ignore_outgoing")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        Self {
            account_id,
            chat_id,
            only_chat_ids,
            ignore_chat_ids,
            text_contains,
            ignore_outgoing,
        }
    }
}

pub fn matches_message_trigger(
    filter: &MessageTriggerFilter,
    event: &TelegramMessageReceived,
    from_boarddo: bool,
) -> bool {
    // Only suppress BoardDo-originated sends. User-typed messages from the same
    // account (phone/desktop) are `is_outgoing` in TDLib but must still trigger.
    if filter.ignore_outgoing && from_boarddo {
        return false;
    }
    if let Some(account_id) = filter.account_id {
        if account_id != event.account_id {
            return false;
        }
    }
    if filter.ignore_chat_ids.iter().any(|id| *id == event.chat_id) {
        return false;
    }
    if let Some(chat_id) = filter.chat_id {
        if chat_id != event.chat_id {
            return false;
        }
    }
    if !filter.only_chat_ids.is_empty()
        && !filter.only_chat_ids.iter().any(|id| *id == event.chat_id)
    {
        return false;
    }
    if let Some(needle) = &filter.text_contains {
        match &event.text {
            Some(text) if text.contains(needle.as_str()) => {}
            _ => return false,
        }
    }
    true
}

fn parse_one_chat_id(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn parse_chat_id_list(raw: Option<&Value>) -> Vec<TelegramChatId> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match raw {
        Value::Array(items) => {
            for item in items {
                if let Some(id) = parse_one_chat_id(item) {
                    out.push(TelegramChatId(id));
                }
            }
        }
        Value::String(s) => {
            for part in s.split(|c: char| c == ',' || c == ';' || c == '\n' || c.is_whitespace()) {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if let Ok(id) = part.parse::<i64>() {
                    out.push(TelegramChatId(id));
                }
            }
        }
        Value::Number(_) => {
            if let Some(id) = parse_one_chat_id(raw) {
                out.push(TelegramChatId(id));
            }
        }
        _ => {}
    }
    out.sort_by_key(|c| c.0);
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use tglib::{TelegramMessageId, TelegramUserId};
    use uuid::Uuid;

    fn event_in(chat: i64, text: &str, outgoing: bool) -> TelegramMessageReceived {
        TelegramMessageReceived {
            account_id: TelegramAccountId(Uuid::nil()),
            chat_id: TelegramChatId(chat),
            message_id: TelegramMessageId(2),
            sender_id: Some(TelegramUserId(9)),
            text: Some(text.into()),
            is_outgoing: outgoing,
            timestamp: Utc::now(),
            reply_to: None,
        }
    }

    fn event(text: &str, outgoing: bool) -> TelegramMessageReceived {
        event_in(1, text, outgoing)
    }

    #[test]
    fn ignores_boarddo_outgoing_by_default() {
        let filter = MessageTriggerFilter::from_node_config(&json!({
            "text_contains": "@ai"
        }));
        assert!(filter.ignore_outgoing);
        assert!(matches_message_trigger(
            &filter,
            &event("@ai hi", false),
            false
        ));
        assert!(
            matches_message_trigger(&filter, &event("@ai hi", true), false),
            "user-typed from same account must still match"
        );
        assert!(!matches_message_trigger(
            &filter,
            &event("@ai hi", true),
            true
        ));
        assert!(!matches_message_trigger(
            &filter,
            &event("plain reply", true),
            true
        ));
    }

    #[test]
    fn echo_without_filter_still_skips_boarddo_outgoing() {
        let filter = MessageTriggerFilter::default();
        assert!(matches_message_trigger(
            &filter,
            &event("hello", false),
            false
        ));
        assert!(matches_message_trigger(&filter, &event("hello", true), false));
        assert!(!matches_message_trigger(
            &filter,
            &event("hello", true),
            true
        ));
    }

    #[test]
    fn allow_boarddo_outgoing_when_configured() {
        let filter = MessageTriggerFilter::from_node_config(&json!({
            "ignore_outgoing": false,
            "text_contains": "@ai"
        }));
        assert!(matches_message_trigger(
            &filter,
            &event("@ai hi", true),
            true
        ));
    }

    #[test]
    fn ignore_chat_ids_from_array_and_csv() {
        let from_array = MessageTriggerFilter::from_node_config(&json!({
            "ignore_chat_ids": [-100123, 42, "77"]
        }));
        assert_eq!(
            from_array.ignore_chat_ids,
            vec![
                TelegramChatId(-100123),
                TelegramChatId(42),
                TelegramChatId(77)
            ]
        );
        assert!(!matches_message_trigger(
            &from_array,
            &event_in(-100123, "hi", false),
            false
        ));
        assert!(matches_message_trigger(
            &from_array,
            &event_in(99, "hi", false),
            false
        ));

        let from_csv = MessageTriggerFilter::from_node_config(&json!({
            "ignore_chat_ids": "-100123, 42\n77"
        }));
        assert_eq!(from_csv.ignore_chat_ids.len(), 3);
        assert!(!matches_message_trigger(
            &from_csv,
            &event_in(42, "x", false),
            false
        ));
    }

    #[test]
    fn only_chat_ids_allow_list() {
        let filter = MessageTriggerFilter::from_node_config(&json!({
            "only_chat_ids": [-1001, 200]
        }));
        assert!(matches_message_trigger(
            &filter,
            &event_in(-1001, "ok", false),
            false
        ));
        assert!(!matches_message_trigger(
            &filter,
            &event_in(1, "no", false),
            false
        ));
    }

    #[test]
    fn ignore_chat_ids_large_channel_ids_csv_and_dedup() {
        let filter = MessageTriggerFilter::from_node_config(&json!({
            "ignore_chat_ids": "-1002364577976, -1001794093147, -1001208526041, -1001504606290, -1002611807132, -1001292964247, -1001752992242, -1002285192950, -1002611807132, -1001794093147, -1001203560567, -1001819840648, -1002285192950, -1002364577976, -1002171564020"
        }));
        assert_eq!(filter.ignore_chat_ids.len(), 11);
        assert!(!matches_message_trigger(
            &filter,
            &event_in(-1002364577976, "spam", false),
            false
        ));
        assert!(!matches_message_trigger(
            &filter,
            &event_in(-1002171564020, "spam", false),
            false
        ));
        assert!(matches_message_trigger(
            &filter,
            &event_in(-1009999999999, "ok", false),
            false
        ));
    }
}
