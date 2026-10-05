use std::sync::Arc;

use boarddo_shared::{ExecutionStatus, WorkflowDefinition};
use serde_json::Value;
use uuid::Uuid;

use crate::connections::{ConnectionProvider, NullConnectionProvider};
use crate::engine::node_state::{NodeStateStore, NullNodeStateStore};
use crate::integrations::media::MediaStore;
use crate::integrations::browser::{self, BrowserRenderer};
use crate::integrations::web::{HttpWebGateway, WebGateway};
use crate::tools::service::CustomToolGateway;
use boarddo_telegram::{NullTelegramUserGateway, TelegramUserGateway};

use super::context::ExecutionContext;
use super::executor::{EventSink, ExecutionResult, Executor};
use super::registry::NodeRegistry;
use super::validate::validate_workflow;

/// High-level SmartDo engine: validate → execute → result.
#[derive(Clone)]
pub struct Engine {
    registry: NodeRegistry,
    executor: Arc<Executor>,
    connections: Arc<dyn ConnectionProvider>,
    telegram_user: Arc<dyn TelegramUserGateway>,
    web: Arc<dyn WebGateway>,
    browser: Arc<dyn BrowserRenderer>,
    node_state: Arc<dyn NodeStateStore>,
    custom_tools: Arc<dyn CustomToolGateway>,
    media: MediaStore,
}

impl Engine {
    pub fn new() -> Self {
        Self::with_connections(Arc::new(NullConnectionProvider))
    }

    pub fn with_connections(connections: Arc<dyn ConnectionProvider>) -> Self {
        let registry = NodeRegistry::with_defaults();
        let executor = Arc::new(Executor::new(registry.clone()));
        Self {
            registry,
            executor,
            connections,
            telegram_user: Arc::new(NullTelegramUserGateway),
            web: Arc::new(HttpWebGateway::from_env()),
            browser: browser::shared(),
            node_state: Arc::new(NullNodeStateStore),
            custom_tools: Arc::new(crate::tools::service::NullCustomToolGateway),
            media: MediaStore::new(
                std::env::var("BOARDDO_DATA_DIR").unwrap_or_else(|_| "data".into()),
            ),
        }
    }

    pub fn with_custom_tools(mut self, custom_tools: Arc<dyn CustomToolGateway>) -> Self {
        self.custom_tools = custom_tools;
        self
    }

    pub fn with_media(mut self, media: MediaStore) -> Self {
        self.media = media;
        self
    }

    pub fn with_telegram_user(mut self, telegram_user: Arc<dyn TelegramUserGateway>) -> Self {
        self.telegram_user = telegram_user;
        self
    }

    pub fn with_web(mut self, web: Arc<dyn WebGateway>) -> Self {
        self.web = web;
        self
    }

    pub fn with_browser(mut self, browser: Arc<dyn BrowserRenderer>) -> Self {
        self.browser = browser;
        self
    }

    pub fn with_node_state(mut self, node_state: Arc<dyn NodeStateStore>) -> Self {
        self.node_state = node_state;
        self
    }

    pub fn registry(&self) -> &NodeRegistry {
        &self.registry
    }

    pub fn validate(&self, definition: &WorkflowDefinition) -> Result<(), Vec<String>> {
        validate_workflow(definition, &self.registry)
    }

    pub async fn execute(
        &self,
        execution_id: Uuid,
        workflow_id: Uuid,
        workflow_version: u32,
        definition: &WorkflowDefinition,
        trigger: Value,
        on_event: Option<&EventSink>,
    ) -> ExecutionResult {
        self.execute_with_source(
            execution_id,
            workflow_id,
            workflow_version,
            definition,
            trigger,
            None,
            on_event,
        )
        .await
    }

    pub async fn execute_with_source(
        &self,
        execution_id: Uuid,
        workflow_id: Uuid,
        workflow_version: u32,
        definition: &WorkflowDefinition,
        trigger: Value,
        trigger_source: Option<&str>,
        on_event: Option<&EventSink>,
    ) -> ExecutionResult {
        let ctx = ExecutionContext::with_source(
            execution_id,
            workflow_id,
            workflow_version,
            trigger,
            trigger_source.map(str::to_string),
        )
        .with_connections(self.connections.clone())
        .with_telegram_user(self.telegram_user.clone())
        .with_web(self.web.clone())
        .with_browser(self.browser.clone())
        .with_node_state(self.node_state.clone())
        .with_custom_tools(self.custom_tools.clone())
        .with_media(self.media.clone());

        if let Err(errors) = self.validate(definition) {
            let msg = format!("Workflow validation failed: {}", errors.join("; "));
            return ExecutionResult {
                status: ExecutionStatus::Failed,
                error: Some(msg),
                node_executions: vec![],
                context: ctx,
            };
        }

        self.executor.run(definition, ctx, on_event).await
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
