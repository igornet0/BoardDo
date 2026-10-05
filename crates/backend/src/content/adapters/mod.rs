//! Channel adapters — Content Engine never branches on `if telegram`.

use async_trait::async_trait;
use boarddo_shared::v2::{ContentChannelId, ContentPublication, ContentItem, GoalSpec, GoalRun};
use serde_json::Value;
use uuid::Uuid;

use crate::state::SharedState;

mod telegram;

pub struct PublishContext<'a> {
    pub state: &'a SharedState,
    pub spec: &'a GoalSpec,
    pub run: &'a GoalRun,
    pub item: &'a ContentItem,
    pub account_id: Option<String>,
    pub chat_id: Option<String>,
    pub idempotency_key: String,
}

#[derive(serde::Serialize)]
pub struct PrepareResult {
    pub account_id: Option<String>,
    pub chat_id: Option<String>,
    pub lifecycle_hint: String,
}

#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn channel(&self) -> ContentChannelId;

    async fn prepare(&self, ctx: &PublishContext<'_>) -> Result<PrepareResult, String>;

    async fn publish(&self, ctx: &PublishContext<'_>) -> Result<ContentPublication, String>;

    async fn metrics(&self, _publication: &ContentPublication) -> Result<Value, String> {
        Ok(Value::Null)
    }
}

pub fn adapter_for(channel: ContentChannelId) -> Box<dyn ChannelAdapter> {
    match channel {
        ContentChannelId::Telegram => Box::new(telegram::TelegramChannelAdapter),
        _ => Box::new(stub::StubChannelAdapter { channel }),
    }
}

mod stub {
    use super::*;

    pub struct StubChannelAdapter {
        pub channel: ContentChannelId,
    }

    #[async_trait]
    impl ChannelAdapter for StubChannelAdapter {
        fn channel(&self) -> ContentChannelId {
            self.channel
        }

        async fn prepare(&self, ctx: &PublishContext<'_>) -> Result<PrepareResult, String> {
            Ok(PrepareResult {
                account_id: ctx.account_id.clone(),
                chat_id: ctx.chat_id.clone(),
                lifecycle_hint: "review".into(),
            })
        }

        async fn publish(&self, ctx: &PublishContext<'_>) -> Result<ContentPublication, String> {
            let demo = ctx
                .spec
                .constraints
                .get("demo")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            Ok(ContentPublication {
                id: Uuid::now_v7(),
                content_id: ctx.item.id,
                channel: self.channel,
                account_id: ctx.account_id.clone(),
                external_id: Some(format!("sim-{}-{}", self.channel.as_str(), Uuid::now_v7())),
                published_at: chrono::Utc::now(),
                status: if demo { "simulated".into() } else { "unsupported".into() },
                simulated: demo,
                metrics: Default::default(),
            })
        }
    }
}
