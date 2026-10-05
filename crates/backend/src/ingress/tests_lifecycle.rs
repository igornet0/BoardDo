//! P7.4 — Channel / Stream / Trigger configuration lifecycle.

#[cfg(test)]
mod tests {
    use boarddo_shared::{
        ChannelKind, CreateChannelRequest, CreateStreamRequest, CreateTriggerRequest,
        StreamDirection, TriggerKind, UpdateChannelRequest, UpdateStreamRequest,
        UpdateTriggerRequest,
    };
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    use crate::ingress::{IngressError, IngressService};
    use crate::storage::Storage;

    async fn setup() -> IngressService {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let storage = Storage::from_pool(pool);
        storage.migrate().await.unwrap();
        IngressService::new(storage)
    }

    #[tokio::test]
    async fn full_lifecycle_and_dependency_guards() {
        let svc = setup().await;

        // Create Channel
        let ch = svc
            .create_channel(CreateChannelRequest {
                name: "main".into(),
                description: "primary".into(),
                kind: ChannelKind::Webhook,
                enabled: true,
                config: json!({"path": "/hooks"}),
            })
            .await
            .unwrap();
        assert_eq!(ch.name, "main");

        // Get / List / Update Channel
        let got = svc.get_channel(ch.id).await.unwrap();
        assert_eq!(got.id, ch.id);
        let listed = svc.list_channels().await.unwrap();
        assert_eq!(listed.len(), 1);
        let ch = svc
            .update_channel(
                ch.id,
                UpdateChannelRequest {
                    name: "main".into(),
                    description: "updated".into(),
                    kind: ChannelKind::Http,
                    enabled: false,
                    config: json!({"path": "/v2"}),
                },
            )
            .await
            .unwrap();
        assert_eq!(ch.description, "updated");
        assert_eq!(ch.kind, ChannelKind::Http);
        assert!(!ch.enabled);

        // Create Stream
        let stream = svc
            .create_stream(CreateStreamRequest {
                channel_id: ch.id,
                name: "orders".into(),
                description: String::new(),
                direction: StreamDirection::Inbound,
                enabled: true,
                config: json!({}),
            })
            .await
            .unwrap();
        assert_eq!(stream.channel_id, ch.id);

        let got_s = svc.get_stream(stream.id).await.unwrap();
        assert_eq!(got_s.name, "orders");
        assert_eq!(svc.list_streams(Some(ch.id)).await.unwrap().len(), 1);

        let stream = svc
            .update_stream(
                stream.id,
                UpdateStreamRequest {
                    channel_id: ch.id,
                    name: "orders".into(),
                    description: "inbound orders".into(),
                    direction: StreamDirection::Outbound,
                    enabled: true,
                    config: json!({"topic": "orders"}),
                },
            )
            .await
            .unwrap();
        assert_eq!(stream.direction, StreamDirection::Outbound);
        assert_eq!(stream.description, "inbound orders");

        // Create Trigger
        let trigger = svc
            .create_trigger(CreateTriggerRequest {
                stream_id: stream.id,
                name: "on-create".into(),
                description: String::new(),
                kind: TriggerKind::Event,
                enabled: true,
                workflow_id: None,
                config: json!({"event": "order.created"}),
            })
            .await
            .unwrap();

        let got_t = svc.get_trigger(trigger.id).await.unwrap();
        assert_eq!(got_t.name, "on-create");
        assert_eq!(svc.list_triggers(Some(stream.id)).await.unwrap().len(), 1);

        let trigger = svc
            .update_trigger(
                trigger.id,
                UpdateTriggerRequest {
                    stream_id: stream.id,
                    name: "on-create".into(),
                    description: "fires on create".into(),
                    kind: TriggerKind::Webhook,
                    enabled: false,
                    workflow_id: None,
                    config: json!({"event": "order.created", "v": 2}),
                },
            )
            .await
            .unwrap();
        assert_eq!(trigger.kind, TriggerKind::Webhook);
        assert!(!trigger.enabled);
        assert_eq!(trigger.description, "fires on create");

        // Dependency: cannot delete channel with streams
        let err = svc.delete_channel(ch.id).await.unwrap_err();
        assert!(matches!(err, IngressError::Conflict(_)));

        // Dependency: cannot delete stream with triggers
        let err = svc.delete_stream(stream.id).await.unwrap_err();
        assert!(matches!(err, IngressError::Conflict(_)));

        // Tear down leaf → root
        svc.delete_trigger(trigger.id).await.unwrap();
        svc.delete_stream(stream.id).await.unwrap();
        svc.delete_channel(ch.id).await.unwrap();
        assert!(svc.list_channels().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn not_found_and_invalid_parent() {
        let svc = setup().await;
        let missing = Uuid::now_v7();

        assert!(matches!(
            svc.get_channel(missing).await.unwrap_err(),
            IngressError::NotFound(_)
        ));
        assert!(matches!(
            svc.update_channel(
                missing,
                UpdateChannelRequest {
                    name: "x".into(),
                    description: String::new(),
                    kind: ChannelKind::Internal,
                    enabled: true,
                    config: json!({}),
                }
            )
            .await
            .unwrap_err(),
            IngressError::NotFound(_)
        ));
        assert!(matches!(
            svc.delete_channel(missing).await.unwrap_err(),
            IngressError::NotFound(_)
        ));

        // Stream with missing channel
        let err = svc
            .create_stream(CreateStreamRequest {
                channel_id: missing,
                name: "s".into(),
                description: String::new(),
                direction: StreamDirection::Inbound,
                enabled: true,
                config: json!({}),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, IngressError::NotFound(_)));

        // Trigger with missing stream
        let err = svc
            .create_trigger(CreateTriggerRequest {
                stream_id: missing,
                name: "t".into(),
                description: String::new(),
                kind: TriggerKind::Event,
                enabled: true,
                workflow_id: None,
                config: json!({}),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, IngressError::NotFound(_)));

        assert!(matches!(
            svc.get_stream(missing).await.unwrap_err(),
            IngressError::NotFound(_)
        ));
        assert!(matches!(
            svc.delete_stream(missing).await.unwrap_err(),
            IngressError::NotFound(_)
        ));
        assert!(matches!(
            svc.get_trigger(missing).await.unwrap_err(),
            IngressError::NotFound(_)
        ));
        assert!(matches!(
            svc.delete_trigger(missing).await.unwrap_err(),
            IngressError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn duplicate_name_conflict() {
        let svc = setup().await;
        let ch = svc
            .create_channel(CreateChannelRequest {
                name: "dup".into(),
                description: String::new(),
                kind: ChannelKind::Internal,
                enabled: true,
                config: json!({}),
            })
            .await
            .unwrap();
        let err = svc
            .create_channel(CreateChannelRequest {
                name: "dup".into(),
                description: String::new(),
                kind: ChannelKind::Internal,
                enabled: true,
                config: json!({}),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, IngressError::AlreadyExists(_)));

        let s = svc
            .create_stream(CreateStreamRequest {
                channel_id: ch.id,
                name: "s1".into(),
                description: String::new(),
                direction: StreamDirection::Inbound,
                enabled: true,
                config: json!({}),
            })
            .await
            .unwrap();
        let err = svc
            .create_stream(CreateStreamRequest {
                channel_id: ch.id,
                name: "s1".into(),
                description: String::new(),
                direction: StreamDirection::Inbound,
                enabled: true,
                config: json!({}),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, IngressError::AlreadyExists(_)));

        let _ = svc
            .create_trigger(CreateTriggerRequest {
                stream_id: s.id,
                name: "t1".into(),
                description: String::new(),
                kind: TriggerKind::Manual,
                enabled: true,
                workflow_id: None,
                config: json!({}),
            })
            .await
            .unwrap();
        let err = svc
            .create_trigger(CreateTriggerRequest {
                stream_id: s.id,
                name: "t1".into(),
                description: String::new(),
                kind: TriggerKind::Manual,
                enabled: true,
                workflow_id: None,
                config: json!({}),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, IngressError::AlreadyExists(_)));
    }
}
