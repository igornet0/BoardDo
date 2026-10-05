//! Domain service for Channel / Stream / Trigger configuration lifecycle.
//!
//! Configuration only — no event processing, scheduling, or workflow firing.

#[cfg(test)]
mod tests_lifecycle;

use boarddo_shared::{
    Channel, CreateChannelRequest, CreateStreamRequest, CreateTriggerRequest, Stream, Trigger,
    UpdateChannelRequest, UpdateStreamRequest, UpdateTriggerRequest,
};
use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

use crate::storage::Storage;

#[derive(Debug, Error)]
pub enum IngressError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("invalid definition: {0}")]
    InvalidDefinition(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error(transparent)]
    Database(#[from] anyhow::Error),
}

pub struct IngressService {
    storage: Storage,
}

impl IngressService {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    // ── Channels ────────────────────────────────────────────────────────

    pub async fn create_channel(&self, req: CreateChannelRequest) -> Result<Channel, IngressError> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(IngressError::InvalidDefinition(
                "channel name is required".into(),
            ));
        }
        if self.storage.find_channel_by_name(name).await?.is_some() {
            return Err(IngressError::AlreadyExists(format!(
                "channel name `{name}`"
            )));
        }
        let id = Uuid::now_v7();
        let now = Utc::now();
        let channel = Channel {
            id,
            name: name.to_string(),
            description: req.description,
            kind: req.kind,
            enabled: req.enabled,
            config: req.config,
            created_at: now,
            updated_at: now,
        };
        self.storage.insert_channel(&channel).await?;
        Ok(channel)
    }

    pub async fn get_channel(&self, id: Uuid) -> Result<Channel, IngressError> {
        self.storage
            .get_channel(id)
            .await?
            .ok_or_else(|| IngressError::NotFound(format!("channel `{id}`")))
    }

    pub async fn list_channels(&self) -> Result<Vec<Channel>, IngressError> {
        Ok(self.storage.list_channels().await?)
    }

    pub async fn update_channel(
        &self,
        id: Uuid,
        req: UpdateChannelRequest,
    ) -> Result<Channel, IngressError> {
        let mut existing = self.get_channel(id).await?;
        let name = req.name.trim();
        if name.is_empty() {
            return Err(IngressError::InvalidDefinition(
                "channel name is required".into(),
            ));
        }
        if let Some(other) = self.storage.find_channel_by_name(name).await? {
            if other.id != id {
                return Err(IngressError::AlreadyExists(format!(
                    "channel name `{name}`"
                )));
            }
        }
        existing.name = name.to_string();
        existing.description = req.description;
        existing.kind = req.kind;
        existing.enabled = req.enabled;
        existing.config = req.config;
        existing.updated_at = Utc::now();
        self.storage.update_channel(&existing).await?;
        Ok(existing)
    }

    pub async fn delete_channel(&self, id: Uuid) -> Result<(), IngressError> {
        let _ = self.get_channel(id).await?;
        let streams = self.storage.count_streams_for_channel(id).await?;
        if streams > 0 {
            return Err(IngressError::Conflict(format!(
                "channel `{id}` has {streams} dependent stream(s); delete them first"
            )));
        }
        let deleted = self.storage.delete_channel(id).await?;
        if !deleted {
            return Err(IngressError::NotFound(format!("channel `{id}`")));
        }
        Ok(())
    }

    // ── Streams ─────────────────────────────────────────────────────────

    pub async fn create_stream(&self, req: CreateStreamRequest) -> Result<Stream, IngressError> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(IngressError::InvalidDefinition(
                "stream name is required".into(),
            ));
        }
        // Parent must exist
        let _ = self.get_channel(req.channel_id).await?;
        if self
            .storage
            .find_stream_by_name(req.channel_id, name)
            .await?
            .is_some()
        {
            return Err(IngressError::AlreadyExists(format!(
                "stream name `{name}` on channel `{}`",
                req.channel_id
            )));
        }
        let id = Uuid::now_v7();
        let now = Utc::now();
        let stream = Stream {
            id,
            channel_id: req.channel_id,
            name: name.to_string(),
            description: req.description,
            direction: req.direction,
            enabled: req.enabled,
            config: req.config,
            created_at: now,
            updated_at: now,
        };
        self.storage.insert_stream(&stream).await?;
        Ok(stream)
    }

    pub async fn get_stream(&self, id: Uuid) -> Result<Stream, IngressError> {
        self.storage
            .get_stream(id)
            .await?
            .ok_or_else(|| IngressError::NotFound(format!("stream `{id}`")))
    }

    pub async fn list_streams(
        &self,
        channel_id: Option<Uuid>,
    ) -> Result<Vec<Stream>, IngressError> {
        Ok(self.storage.list_streams(channel_id).await?)
    }

    pub async fn update_stream(
        &self,
        id: Uuid,
        req: UpdateStreamRequest,
    ) -> Result<Stream, IngressError> {
        let mut existing = self.get_stream(id).await?;
        let name = req.name.trim();
        if name.is_empty() {
            return Err(IngressError::InvalidDefinition(
                "stream name is required".into(),
            ));
        }
        // Parent must exist (including on channel move)
        let _ = self.get_channel(req.channel_id).await?;
        if let Some(other) = self
            .storage
            .find_stream_by_name(req.channel_id, name)
            .await?
        {
            if other.id != id {
                return Err(IngressError::AlreadyExists(format!(
                    "stream name `{name}` on channel `{}`",
                    req.channel_id
                )));
            }
        }
        existing.channel_id = req.channel_id;
        existing.name = name.to_string();
        existing.description = req.description;
        existing.direction = req.direction;
        existing.enabled = req.enabled;
        existing.config = req.config;
        existing.updated_at = Utc::now();
        self.storage.update_stream(&existing).await?;
        Ok(existing)
    }

    pub async fn delete_stream(&self, id: Uuid) -> Result<(), IngressError> {
        let _ = self.get_stream(id).await?;
        let triggers = self.storage.count_triggers_for_stream(id).await?;
        if triggers > 0 {
            return Err(IngressError::Conflict(format!(
                "stream `{id}` has {triggers} dependent trigger(s); delete them first"
            )));
        }
        let deleted = self.storage.delete_stream(id).await?;
        if !deleted {
            return Err(IngressError::NotFound(format!("stream `{id}`")));
        }
        Ok(())
    }

    // ── Triggers ────────────────────────────────────────────────────────

    pub async fn create_trigger(&self, req: CreateTriggerRequest) -> Result<Trigger, IngressError> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(IngressError::InvalidDefinition(
                "trigger name is required".into(),
            ));
        }
        let _ = self.get_stream(req.stream_id).await?;
        if let Some(wf) = req.workflow_id {
            if self.storage.get_workflow(wf).await?.is_none() {
                return Err(IngressError::NotFound(format!("workflow `{wf}`")));
            }
        }
        if self
            .storage
            .find_trigger_by_name(req.stream_id, name)
            .await?
            .is_some()
        {
            return Err(IngressError::AlreadyExists(format!(
                "trigger name `{name}` on stream `{}`",
                req.stream_id
            )));
        }
        let id = Uuid::now_v7();
        let now = Utc::now();
        let trigger = Trigger {
            id,
            stream_id: req.stream_id,
            name: name.to_string(),
            description: req.description,
            kind: req.kind,
            enabled: req.enabled,
            workflow_id: req.workflow_id,
            config: req.config,
            created_at: now,
            updated_at: now,
        };
        self.storage.insert_trigger(&trigger).await?;
        Ok(trigger)
    }

    pub async fn get_trigger(&self, id: Uuid) -> Result<Trigger, IngressError> {
        self.storage
            .get_trigger(id)
            .await?
            .ok_or_else(|| IngressError::NotFound(format!("trigger `{id}`")))
    }

    pub async fn list_triggers(
        &self,
        stream_id: Option<Uuid>,
    ) -> Result<Vec<Trigger>, IngressError> {
        Ok(self.storage.list_triggers(stream_id).await?)
    }

    pub async fn update_trigger(
        &self,
        id: Uuid,
        req: UpdateTriggerRequest,
    ) -> Result<Trigger, IngressError> {
        let mut existing = self.get_trigger(id).await?;
        let name = req.name.trim();
        if name.is_empty() {
            return Err(IngressError::InvalidDefinition(
                "trigger name is required".into(),
            ));
        }
        let _ = self.get_stream(req.stream_id).await?;
        if let Some(wf) = req.workflow_id {
            if self.storage.get_workflow(wf).await?.is_none() {
                return Err(IngressError::NotFound(format!("workflow `{wf}`")));
            }
        }
        if let Some(other) = self
            .storage
            .find_trigger_by_name(req.stream_id, name)
            .await?
        {
            if other.id != id {
                return Err(IngressError::AlreadyExists(format!(
                    "trigger name `{name}` on stream `{}`",
                    req.stream_id
                )));
            }
        }
        existing.stream_id = req.stream_id;
        existing.name = name.to_string();
        existing.description = req.description;
        existing.kind = req.kind;
        existing.enabled = req.enabled;
        existing.workflow_id = req.workflow_id;
        existing.config = req.config;
        existing.updated_at = Utc::now();
        self.storage.update_trigger(&existing).await?;
        Ok(existing)
    }

    pub async fn delete_trigger(&self, id: Uuid) -> Result<(), IngressError> {
        let _ = self.get_trigger(id).await?;
        let deleted = self.storage.delete_trigger(id).await?;
        if !deleted {
            return Err(IngressError::NotFound(format!("trigger `{id}`")));
        }
        Ok(())
    }
}
