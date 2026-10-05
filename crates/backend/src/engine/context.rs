use std::sync::Arc;

use serde_json::{Map, Value};
use uuid::Uuid;

use boarddo_shared::WorkflowDefinition;

use crate::connections::ConnectionProvider;
use crate::engine::node_state::{NodeStateStore, NullNodeStateStore};
use crate::integrations::media::MediaStore;
use crate::integrations::browser::{BrowserRenderer, NullBrowserRenderer};
use crate::integrations::web::{NullWebGateway, WebGateway};
use crate::tools::service::{CustomToolGateway, NullCustomToolGateway};
use boarddo_telegram::{NullTelegramUserGateway, TelegramUserGateway};

use super::expr;

/// Runtime context available to every node during an execution.
#[derive(Clone)]
pub struct ExecutionContext {
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version: u32,
    pub trigger: Value,
    /// How this run was started: `manual`, `webhook`, …
    pub trigger_source: Option<String>,
    pub variables: Map<String, Value>,
    pub nodes: Map<String, Value>,
    pub connections: Arc<dyn ConnectionProvider>,
    pub telegram_user: Arc<dyn TelegramUserGateway>,
    pub web: Arc<dyn WebGateway>,
    /// Headless browser for JavaScript-rendered pages (RustBrowser automation server).
    pub browser: Arc<dyn BrowserRenderer>,
    pub node_state: Arc<dyn NodeStateStore>,
    pub custom_tools: Arc<dyn CustomToolGateway>,
    pub media: MediaStore,
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("execution_id", &self.execution_id)
            .field("workflow_id", &self.workflow_id)
            .field("workflow_version", &self.workflow_version)
            .field("trigger", &self.trigger)
            .field("trigger_source", &self.trigger_source)
            .field("variables", &self.variables)
            .field("nodes", &self.nodes)
            .finish_non_exhaustive()
    }
}

impl ExecutionContext {
    pub fn new(
        execution_id: Uuid,
        workflow_id: Uuid,
        workflow_version: u32,
        trigger: Value,
    ) -> Self {
        Self::with_source(execution_id, workflow_id, workflow_version, trigger, None)
    }

    pub fn with_source(
        execution_id: Uuid,
        workflow_id: Uuid,
        workflow_version: u32,
        trigger: Value,
        trigger_source: Option<String>,
    ) -> Self {
        Self {
            execution_id,
            workflow_id,
            workflow_version,
            trigger,
            trigger_source,
            variables: Map::new(),
            nodes: Map::new(),
            connections: Arc::new(crate::connections::NullConnectionProvider),
            telegram_user: Arc::new(NullTelegramUserGateway),
            web: Arc::new(NullWebGateway),
            browser: Arc::new(NullBrowserRenderer),
            node_state: Arc::new(NullNodeStateStore),
            custom_tools: Arc::new(NullCustomToolGateway),
            media: MediaStore::new(
                std::env::var("BOARDDO_DATA_DIR").unwrap_or_else(|_| "data".into()),
            ),
        }
    }

    pub fn with_custom_tools(mut self, custom_tools: Arc<dyn CustomToolGateway>) -> Self {
        self.custom_tools = custom_tools;
        self
    }

    pub fn with_connections(mut self, connections: Arc<dyn ConnectionProvider>) -> Self {
        self.connections = connections;
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

    pub fn with_media(mut self, media: MediaStore) -> Self {
        self.media = media;
        self
    }

    pub fn is_sandbox(&self) -> bool {
        self.trigger
            .get("sandbox")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn set_variable(&mut self, name: impl Into<String>, value: Value) {
        self.variables.insert(name.into(), value);
    }

    pub fn set_node_output(&mut self, node_id: impl Into<String>, output: Value) {
        self.nodes.insert(node_id.into(), output);
    }

    /// Resolve `{{…}}` templates and expressions against this context.
    pub fn resolve_template(&self, template: &str) -> Value {
        expr::resolve(self, template).unwrap_or(Value::Null)
    }

    pub fn resolve_template_result(&self, template: &str) -> Result<Value, String> {
        expr::resolve(self, template)
    }

    pub fn resolve_value(&self, raw: Value) -> Result<Value, String> {
        expr::resolve_value(self, raw)
    }

    pub fn resolve_path(&self, path: &str) -> Option<Value> {
        let path = path.trim();
        if path.is_empty() {
            return None;
        }

        let mut parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return None;
        }

        let root = parts.remove(0);
        let mut current = match root {
            "trigger" => self.trigger.clone(),
            "variables" => Value::Object(self.variables.clone()),
            "nodes" => {
                if parts.is_empty() {
                    return Some(Value::Object(self.nodes.clone()));
                }
                let node_id = parts.remove(0);
                let output = self.nodes.get(node_id)?.clone();
                if parts.first().copied() == Some("output") {
                    parts.remove(0);
                }
                let mut cur = output;
                for part in parts {
                    cur = descend(cur, part)?;
                }
                return Some(cur);
            }
            other => self.variables.get(other)?.clone(),
        };

        for part in parts {
            current = descend(current, part)?;
        }
        Some(current)
    }

    pub fn snapshot(&self) -> Value {
        serde_json::json!({
            "trigger": self.trigger,
            "variables": self.variables,
            "nodes": self.nodes,
        })
    }

    pub fn from_definition(
        execution_id: Uuid,
        workflow_id: Uuid,
        workflow_version: u32,
        trigger: Value,
        _definition: &WorkflowDefinition,
    ) -> Self {
        Self::new(execution_id, workflow_id, workflow_version, trigger)
    }
}

fn descend(current: Value, part: &str) -> Option<Value> {
    match current {
        Value::Object(map) => map.get(part).cloned(),
        Value::Array(arr) => {
            let idx: usize = part.parse().ok()?;
            arr.get(idx).cloned()
        }
        _ => None,
    }
}
