use async_trait::async_trait;
use boarddo_shared::v2::{ContentChannelId, ContentLifecycle, ContentPublication};
use boarddo_telegram::TelegramUserGateway;
use chrono::Utc;
use tglib::{TelegramAccountId, TelegramChatId};
use uuid::Uuid;

use super::{ChannelAdapter, PrepareResult, PublishContext};

pub struct TelegramChannelAdapter;

#[async_trait]
impl ChannelAdapter for TelegramChannelAdapter {
    fn channel(&self) -> ContentChannelId {
        ContentChannelId::Telegram
    }

    async fn prepare(&self, ctx: &PublishContext<'_>) -> Result<PrepareResult, String> {
        let accounts = ctx
            .state
            .telegram
            .list_accounts()
            .await
            .map_err(|e| e.to_string())?;
        let account_id = ctx
            .account_id
            .clone()
            .or_else(|| {
                ctx.spec
                    .constraints
                    .get("telegram_account_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .or_else(|| accounts.first().map(|a| a.id.to_string()));
        let chat_id = ctx
            .chat_id
            .clone()
            .or_else(|| {
                ctx.spec
                    .constraints
                    .get("telegram_chat_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .or_else(|| {
                ctx.spec
                    .constraints
                    .pointer("/telegram_channels/0")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "pending-approval".into());

        let mut item = ctx.item.clone();
        item.metadata
            .insert("resolved_account_id".into(), serde_json::json!(account_id));
        item.metadata
            .insert("resolved_chat_id".into(), serde_json::json!(&chat_id));
        ctx.state
            .storage
            .update_content_item(&item)
            .await
            .map_err(|e| e.to_string())?;

        Ok(PrepareResult {
            account_id,
            chat_id: Some(chat_id),
            lifecycle_hint: ContentLifecycle::Review.as_str().into(),
        })
    }

    async fn publish(&self, ctx: &PublishContext<'_>) -> Result<ContentPublication, String> {
        if let Some(existing) = ctx
            .state
            .storage
            .get_publication_by_idempotency(&ctx.idempotency_key)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(existing);
        }

        let account_id = ctx
            .account_id
            .as_deref()
            .or_else(|| {
                ctx.item
                    .metadata
                    .get("resolved_account_id")
                    .and_then(|v| v.as_str())
            })
            .or_else(|| {
                ctx.spec
                    .constraints
                    .get("telegram_account_id")
                    .and_then(|v| v.as_str())
            });
        let chat_id = ctx
            .chat_id
            .as_deref()
            .or_else(|| ctx.item.metadata.get("resolved_chat_id").and_then(|v| v.as_str()))
            .or_else(|| {
                ctx.spec
                    .constraints
                    .get("telegram_chat_id")
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("pending-approval");

        let demo = ctx
            .spec
            .constraints
            .get("demo")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || chat_id == "pending-approval"
            || account_id.is_none();

        let pub_record = if demo {
            ContentPublication {
                id: Uuid::now_v7(),
                content_id: ctx.item.id,
                channel: ContentChannelId::Telegram,
                account_id: account_id.map(str::to_string),
                external_id: Some(format!("sim-tg-{}", Uuid::now_v7())),
                published_at: Utc::now(),
                status: "published".into(),
                simulated: true,
                metrics: Default::default(),
            }
        } else {
            let account_uuid =
                uuid::Uuid::parse_str(account_id.unwrap()).map_err(|e| e.to_string())?;
            let chat = TelegramChatId(
                chat_id
                    .parse::<i64>()
                    .map_err(|_| format!("invalid chat_id `{chat_id}`"))?,
            );
            let message_id = ctx
                .state
                .telegram
                .send_message(
                    TelegramAccountId(account_uuid),
                    chat,
                    ctx.item.body.clone(),
                    Some("markdown".into()),
                    None,
                    Some(ctx.spec.id),
                )
                .await
                .map_err(|e| e.to_string())?;
            ContentPublication {
                id: Uuid::now_v7(),
                content_id: ctx.item.id,
                channel: ContentChannelId::Telegram,
                account_id: account_id.map(str::to_string),
                external_id: Some(message_id.0.to_string()),
                published_at: Utc::now(),
                status: "published".into(),
                simulated: false,
                metrics: Default::default(),
            }
        };

        ctx.state
            .storage
            .insert_publication(&pub_record, &ctx.idempotency_key)
            .await
            .map_err(|e| e.to_string())?;
        Ok(pub_record)
    }
}
