//! In-memory cache of chat member lists. Membership changes rarely, so
//! scenarios that need the list on every message can reuse a recent copy
//! instead of hitting Telegram each time. Lost on restart by design.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tglib::{ChatMembersResponse, TelegramAccountId, TelegramChatId};
use uuid::Uuid;

/// Member list plus where it came from.
#[derive(Clone, Debug)]
pub struct ChatMembersResult {
    pub response: ChatMembersResponse,
    pub fetched_at: DateTime<Utc>,
    pub from_cache: bool,
}

struct Entry {
    stored: Instant,
    fetched_at: DateTime<Utc>,
    limit: u32,
    response: ChatMembersResponse,
}

#[derive(Default)]
pub struct MembersCache {
    entries: DashMap<(Uuid, i64), Entry>,
}

impl MembersCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// A copy no older than `max_age` that covers `limit` members.
    pub fn get(
        &self,
        account_id: TelegramAccountId,
        chat_id: TelegramChatId,
        limit: u32,
        max_age: Duration,
    ) -> Option<ChatMembersResult> {
        let entry = self.entries.get(&(account_id.0, chat_id.0))?;
        if entry.stored.elapsed() > max_age {
            return None;
        }
        let complete = entry.response.members.len() as u32 >= entry.response.total_count;
        if entry.limit < limit && !complete {
            return None;
        }
        let mut response = entry.response.clone();
        response.members.truncate(limit as usize);
        Some(ChatMembersResult {
            response,
            fetched_at: entry.fetched_at,
            from_cache: true,
        })
    }

    pub fn put(
        &self,
        account_id: TelegramAccountId,
        chat_id: TelegramChatId,
        limit: u32,
        response: &ChatMembersResponse,
    ) -> DateTime<Utc> {
        let fetched_at = Utc::now();
        self.entries.insert(
            (account_id.0, chat_id.0),
            Entry {
                stored: Instant::now(),
                fetched_at,
                limit,
                response: response.clone(),
            },
        );
        fetched_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tglib::{TelegramChatMember, TelegramUser, TelegramUserId};

    fn response(n: usize, total: u32) -> ChatMembersResponse {
        ChatMembersResponse {
            total_count: total,
            members: (0..n)
                .map(|i| TelegramChatMember {
                    user: TelegramUser {
                        id: TelegramUserId(i as i64),
                        first_name: format!("u{i}"),
                        last_name: None,
                        username: None,
                        is_bot: false,
                    },
                    status: "member".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn hit_within_ttl_and_truncates() {
        let cache = MembersCache::new();
        let (acc, chat) = (TelegramAccountId::new(), TelegramChatId(-1));
        cache.put(acc, chat, 200, &response(5, 5));
        let hit = cache.get(acc, chat, 3, Duration::from_secs(60)).unwrap();
        assert!(hit.from_cache);
        assert_eq!(hit.response.members.len(), 3);
        assert!(cache.get(acc, TelegramChatId(-2), 3, Duration::from_secs(60)).is_none());
        assert!(cache.get(TelegramAccountId::new(), chat, 3, Duration::from_secs(60)).is_none());
    }

    #[test]
    fn miss_when_expired() {
        let cache = MembersCache::new();
        let (acc, chat) = (TelegramAccountId::new(), TelegramChatId(-1));
        cache.put(acc, chat, 200, &response(2, 2));
        std::thread::sleep(Duration::from_millis(5));
        assert!(cache.get(acc, chat, 200, Duration::from_millis(1)).is_none());
    }

    #[test]
    fn larger_limit_needs_refetch_unless_complete() {
        let cache = MembersCache::new();
        let (acc, chat) = (TelegramAccountId::new(), TelegramChatId(-1));
        cache.put(acc, chat, 10, &response(10, 500));
        assert!(cache.get(acc, chat, 200, Duration::from_secs(60)).is_none());
        assert!(cache.get(acc, chat, 10, Duration::from_secs(60)).is_some());

        cache.put(acc, chat, 10, &response(4, 4));
        assert!(cache.get(acc, chat, 200, Duration::from_secs(60)).is_some());
    }
}
