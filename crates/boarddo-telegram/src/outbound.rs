//! Track messages BoardDo itself sent so triggers can ignore them without
//! also suppressing user-typed messages from the same account (phone/desktop).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::Mutex;

use tglib::{TelegramAccountId, TelegramChatId, TelegramMessageId};

const SENT_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_SENT: usize = 4_096;

#[derive(Default)]
pub struct OutboundTracker {
    inflight: DashMap<TelegramAccountId, AtomicUsize>,
    sent: DashMap<(uuid::Uuid, i64, i64), Instant>,
    prune_lock: Mutex<()>,
}

impl OutboundTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_send(&self, account_id: TelegramAccountId) -> OutboundSendGuard<'_> {
        self.inflight
            .entry(account_id)
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::SeqCst);
        OutboundSendGuard {
            tracker: self,
            account_id,
        }
    }

    pub fn note_sent(
        &self,
        account_id: TelegramAccountId,
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
    ) {
        self.sent
            .insert((account_id.0, chat_id.0, message_id.0), Instant::now());
        self.maybe_prune();
    }

    pub fn is_boarddo_originated(
        &self,
        account_id: TelegramAccountId,
        chat_id: TelegramChatId,
        message_id: TelegramMessageId,
        is_outgoing: bool,
    ) -> bool {
        if self
            .sent
            .contains_key(&(account_id.0, chat_id.0, message_id.0))
        {
            return true;
        }
        if !is_outgoing {
            return false;
        }
        self.inflight
            .get(&account_id)
            .is_some_and(|n| n.load(Ordering::SeqCst) > 0)
    }

    fn end_send(&self, account_id: TelegramAccountId) {
        if let Some(n) = self.inflight.get(&account_id) {
            let _ = n.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(1))
            });
        }
    }

    fn maybe_prune(&self) {
        if self.sent.len() < MAX_SENT / 2 {
            return;
        }
        let Some(_guard) = self.prune_lock.try_lock() else {
            return;
        };
        let now = Instant::now();
        self.sent
            .retain(|_, at| now.duration_since(*at) < SENT_TTL);
        if self.sent.len() > MAX_SENT {
            // Drop oldest half if still oversized.
            let mut entries: Vec<_> = self
                .sent
                .iter()
                .map(|e| (*e.key(), *e.value()))
                .collect();
            entries.sort_by_key(|(_, at)| *at);
            let drop_n = entries.len().saturating_sub(MAX_SENT / 2);
            for (key, _) in entries.into_iter().take(drop_n) {
                self.sent.remove(&key);
            }
        }
    }
}

pub struct OutboundSendGuard<'a> {
    tracker: &'a OutboundTracker,
    account_id: TelegramAccountId,
}

impl Drop for OutboundSendGuard<'_> {
    fn drop(&mut self) {
        self.tracker.end_send(self.account_id);
    }
}
