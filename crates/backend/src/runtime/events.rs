use boarddo_shared::ExecutionStatus;
use serde_json::Value;
use uuid::Uuid;

/// Why a runtime is firing an execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEventSource {
    TelegramUser,
    Schedule,
    Webhook,
}

impl RuntimeEventSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TelegramUser => "telegram.user",
            Self::Schedule => "schedule",
            Self::Webhook => "webhook",
        }
    }

    pub fn allows_concurrent(self) -> bool {
        matches!(self, Self::TelegramUser)
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeEvent {
    pub workflow_id: Uuid,
    pub source: RuntimeEventSource,
    pub payload: Value,
    pub schedule_node_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionFinished {
    pub workflow_id: Uuid,
    pub execution_id: Uuid,
    pub status: ExecutionStatus,
}
