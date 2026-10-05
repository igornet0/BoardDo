use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use boarddo_shared::Node;
use serde_json::Value;

use super::context::ExecutionContext;

/// Result produced by a node handler.
#[derive(Debug, Clone)]
pub struct NodeOutput {
    pub data: Value,
    /// For branching nodes (condition): which source_port to follow next.
    pub branch: Option<String>,
}

impl NodeOutput {
    pub fn data(data: Value) -> Self {
        Self { data, branch: None }
    }

    pub fn branch(data: Value, port: impl Into<String>) -> Self {
        Self {
            data,
            branch: Some(port.into()),
        }
    }
}

#[async_trait]
pub trait NodeHandler: Send + Sync {
    fn type_id(&self) -> &'static str;
    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String>;
}

/// Registry of node handlers keyed by `type_id`.
#[derive(Default, Clone)]
pub struct NodeRegistry {
    handlers: HashMap<String, Arc<dyn NodeHandler>>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, handler: Arc<dyn NodeHandler>) {
        self.handlers.insert(handler.type_id().to_string(), handler);
    }

    pub fn get(&self, type_id: &str) -> Option<Arc<dyn NodeHandler>> {
        self.handlers.get(type_id).cloned()
    }

    pub fn contains(&self, type_id: &str) -> bool {
        self.handlers.contains_key(type_id)
    }

    pub fn known_types(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }

    pub fn with_defaults() -> Self {
        use crate::engine::nodes::{
            AiAnalyzeNode, AiAudioNode, AiChatNode, AiClassifyNode, AiImageNode, AiVideoNode,
            ConditionNode, CustomToolRunNode, DelayNode,
            GitHubGetLatestRelease, HttpRequestNode, LogNode, ManualTrigger, ScheduleTrigger,
            SetDataNode, TelegramSendDocument, TelegramSendMessage, TelegramSendPhoto,
            TelegramUserDeleteMessages, TelegramUserEditMessage, TelegramUserForwardMessage,
            TelegramUserMessageReceived, TelegramUserSendMessage, TransformNode, WebExtractNode, WebFetchNode, WebOpenNode,
            WebSearchNode, WebhookTrigger,
        };

        let mut registry = Self::new();
        registry.register(Arc::new(ManualTrigger));
        registry.register(Arc::new(WebhookTrigger));
        registry.register(Arc::new(ScheduleTrigger));
        registry.register(Arc::new(SetDataNode));
        registry.register(Arc::new(TransformNode));
        registry.register(Arc::new(ConditionNode));
        registry.register(Arc::new(CustomToolRunNode));
        registry.register(Arc::new(DelayNode));
        registry.register(Arc::new(LogNode));
        registry.register(Arc::new(HttpRequestNode));
        registry.register(Arc::new(AiChatNode));
        registry.register(Arc::new(AiClassifyNode));
        registry.register(Arc::new(AiAnalyzeNode));
        registry.register(Arc::new(AiImageNode));
        registry.register(Arc::new(AiAudioNode));
        registry.register(Arc::new(AiVideoNode));
        registry.register(Arc::new(WebSearchNode));
        registry.register(Arc::new(WebFetchNode));
        registry.register(Arc::new(WebOpenNode));
        registry.register(Arc::new(WebExtractNode));
        registry.register(Arc::new(GitHubGetLatestRelease));
        registry.register(Arc::new(TelegramSendMessage));
        registry.register(Arc::new(TelegramSendPhoto));
        registry.register(Arc::new(TelegramSendDocument));
        registry.register(Arc::new(TelegramUserMessageReceived));
        registry.register(Arc::new(TelegramUserSendMessage));
        registry.register(Arc::new(TelegramUserForwardMessage));
        registry.register(Arc::new(TelegramUserEditMessage));
        registry.register(Arc::new(TelegramUserDeleteMessages));
        registry
    }
}
