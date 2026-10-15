//! Tool registry + execution service.

use std::sync::Arc;

use async_trait::async_trait;
use boarddo_shared::v2::{
    CreateToolRequest, CustomTool, CustomToolVersion, ToolExecutionRecord, ToolOrigin,
    ToolVersionStatus,
};
use chrono::Utc;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::storage::Storage;

use super::builder::{self, BuildOutcome, ToolRepairer};
use super::sandbox;

/// Tool operation error; the API maps the kind to an HTTP status.
#[derive(Debug, Clone)]
pub enum ToolError {
    NotFound(String),
    Conflict(String),
    Invalid(String),
    /// The code did not pass its checks / tests; `details` has the build report.
    BuildFailed { message: String, details: Value },
    Internal(String),
}

impl ToolError {
    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(m) | Self::Conflict(m) | Self::Invalid(m) | Self::Internal(m) => m,
            Self::BuildFailed { message, .. } => message,
        }
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuildFailed { message, .. } => write!(f, "TOOL_BUILD_FAILED: {message}"),
            other => f.write_str(other.message()),
        }
    }
}

impl From<ToolError> for String {
    fn from(e: ToolError) -> Self {
        e.to_string()
    }
}

fn internal(e: impl std::fmt::Display) -> ToolError {
    ToolError::Internal(e.to_string())
}

#[derive(Clone)]
pub struct ToolService {
    storage: Storage,
}

impl ToolService {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub async fn list(&self) -> Result<Vec<CustomTool>, String> {
        self.storage
            .list_custom_tools()
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn find(&self, query: &str, limit: u32) -> Result<Vec<CustomTool>, String> {
        self.storage
            .find_custom_tools(query, limit)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<CustomTool>, String> {
        self.storage
            .get_custom_tool(id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_detail(&self, id: Uuid) -> Result<Value, ToolError> {
        let tool = self
            .get(id)
            .await
            .map_err(ToolError::Internal)?
            .ok_or_else(|| ToolError::NotFound("tool not found".to_string()))?;
        let versions = self.storage.list_custom_tool_versions(id).await.map_err(internal)?;
        let executions = self
            .storage
            .list_custom_tool_executions(id, 50)
            .await
            .map_err(internal)?;
        Ok(json!({
            "tool": tool,
            "versions": versions,
            "executions": executions,
        }))
    }

    pub async fn create_tool(
        &self,
        req: CreateToolRequest,
        created_by: &str,
        origin: ToolOrigin,
    ) -> Result<Value, ToolError> {
        self.create_tool_with(req, created_by, origin, None).await
    }

    /// Create, letting `repairer` fix code that fails its checks or tests.
    pub async fn create_tool_with(
        &self,
        req: CreateToolRequest,
        created_by: &str,
        origin: ToolOrigin,
        repairer: Option<&dyn ToolRepairer>,
    ) -> Result<Value, ToolError> {
        super::security::validate_name(&req.name).map_err(ToolError::Invalid)?;
        if self
            .storage
            .get_custom_tool_by_name(&req.name)
            .await
            .map_err(internal)?
            .is_some()
        {
            return Err(ToolError::Conflict(format!("tool `{}` already exists", req.name)));
        }

        let outcome = builder::build_with_repairer(req.code.clone(), &req, repairer)
            .await
            .map_err(ToolError::Invalid)?;
        if !outcome.activated {
            return Err(ToolError::BuildFailed {
                message: outcome.failure.clone().unwrap_or_else(|| "validation failed".into()),
                details: build_report(&outcome),
            });
        }

        let now = Utc::now();
        let tool_id = Uuid::now_v7();
        let ver = builder::version_from_build(tool_id, 1, &req, &outcome);

        let tool = CustomTool {
            id: tool_id,
            name: req.name.clone(),
            description: req.description.clone(),
            language: boarddo_shared::v2::ToolLanguage::Python,
            origin,
            created_by: created_by.into(),
            active_version_id: Some(ver.id),
            tags: req.tags.clone(),
            execution_stats: Default::default(),
            created_at: now,
            updated_at: now,
        };

        self.storage
            .insert_custom_tool(&tool)
            .await
            .map_err(internal)?;
        self.storage
            .insert_custom_tool_version(&ver)
            .await
            .map_err(internal)?;

        Ok(json!({
            "tool": tool,
            "version": ver,
            "build": outcome_summary(&outcome),
        }))
    }

    pub async fn update_tool(
        &self,
        name_or_id: &str,
        req: CreateToolRequest,
        actor: &str,
    ) -> Result<Value, ToolError> {
        self.update_tool_with(name_or_id, req, actor, None).await
    }

    pub async fn update_tool_with(
        &self,
        name_or_id: &str,
        mut req: CreateToolRequest,
        actor: &str,
        repairer: Option<&dyn ToolRepairer>,
    ) -> Result<Value, ToolError> {
        let tool = self.resolve_tool(name_or_id).await?;
        if tool.origin == ToolOrigin::System {
            return Err(ToolError::Invalid("cannot modify SYSTEM tool".into()));
        }
        // The name is the identity; a new version keeps it.
        req.name = tool.name.clone();

        let prev_active = self
            .storage
            .get_active_custom_tool_version(&tool)
            .await
            .map_err(internal)?;

        let version_num = self
            .storage
            .next_custom_tool_version(tool.id)
            .await
            .map_err(internal)?;

        let outcome = builder::build_with_repairer(req.code.clone(), &req, repairer)
            .await
            .map_err(ToolError::Invalid)?;
        let mut ver = builder::version_from_build(tool.id, version_num, &req, &outcome);

        if outcome.activated {
            self.storage
                .demote_active_versions(tool.id)
                .await
                .map_err(internal)?;
            ver.status = ToolVersionStatus::Active;
        } else if prev_active.is_some() {
            // keep previous active — never replace working version with broken code
            ver.status = ToolVersionStatus::Failed;
        }

        self.storage
            .insert_custom_tool_version(&ver)
            .await
            .map_err(internal)?;

        let mut tool = tool;
        if outcome.activated {
            tool.active_version_id = Some(ver.id);
        }
        tool.updated_at = Utc::now();
        tool.description = req.description;
        self.storage
            .update_custom_tool_meta(&tool)
            .await
            .map_err(internal)?;

        let _ = actor;
        Ok(json!({
            "tool": tool,
            "version": ver,
            "build": outcome_summary(&outcome),
            "report": build_report(&outcome),
            "rolled_back": !outcome.activated,
        }))
    }

    pub async fn test_tool(&self, name_or_id: &str, input: Value) -> Result<Value, ToolError> {
        let (tool, ver) = self.resolve_active_version(name_or_id).await?;
        let result = sandbox::execute_python(
            &ver.source_code,
            input,
            &ver.permissions,
            &ver.runtime_config,
        )
        .await;
        Ok(json!({
            "tool_id": tool.id,
            "version": ver.version,
            "result": sandbox_to_json(&result),
        }))
    }

    pub async fn run_tool(&self, name_or_id: &str, input: Value) -> Result<Value, ToolError> {
        let (tool, ver) = self.resolve_active_version(name_or_id).await?;
        if ver.status != ToolVersionStatus::Active {
            return Err(ToolError::Invalid("no ACTIVE version".into()));
        }
        super::security::validate_requirements(&ver.requirements).map_err(ToolError::Invalid)?;

        let result = sandbox::execute_python(
            &ver.source_code,
            input.clone(),
            &ver.permissions,
            &ver.runtime_config,
        )
        .await;

        let exec_id = Uuid::now_v7();
        let rec = ToolExecutionRecord {
            id: exec_id,
            tool_id: tool.id,
            version: ver.version,
            status: if result.success {
                "success".into()
            } else {
                "failed".into()
            },
            duration_ms: result.duration_ms,
            output: result.output.clone(),
            error_type: result.error_type.clone(),
            message: result.message.clone(),
            traceback: result.traceback.clone(),
            logs: result.logs.clone(),
            resource_usage: Map::from_iter([(
                "memory_mb".into(),
                json!(ver.runtime_config.memory_mb),
            )]),
            created_at: Utc::now(),
        };
        self.storage
            .insert_custom_tool_execution(&rec)
            .await
            .map_err(internal)?;
        self.storage
            .record_custom_tool_stats(
                tool.id,
                result.success,
                result.duration_ms,
                result.message.clone(),
            )
            .await
            .map_err(internal)?;

        if result.success {
            Ok(json!({
                "execution_id": exec_id,
                "tool_id": tool.id,
                "name": tool.name,
                "version": ver.version,
                "status": "success",
                "duration_ms": result.duration_ms,
                "output": result.output,
                "logs": result.logs,
            }))
        } else {
            Ok(json!({
                "execution_id": exec_id,
                "tool_id": tool.id,
                "version": ver.version,
                "status": "failed",
                "error_type": result.error_type,
                "message": result.message,
                "traceback": result.traceback,
                "logs": result.logs,
            }))
        }
    }

    pub async fn delete_tool(&self, name_or_id: &str) -> Result<Value, ToolError> {
        let tool = self.resolve_tool(name_or_id).await?;
        if tool.origin == ToolOrigin::System {
            return Err(ToolError::Invalid("cannot delete SYSTEM tool".into()));
        }
        self.storage
            .delete_custom_tool(tool.id)
            .await
            .map_err(internal)?;
        Ok(json!({ "deleted": tool.id }))
    }

    pub async fn resolve_tool(&self, name_or_id: &str) -> Result<CustomTool, ToolError> {
        if let Ok(id) = Uuid::parse_str(name_or_id) {
            if let Some(t) = self.get(id).await.map_err(ToolError::Internal)? {
                return Ok(t);
            }
        }
        self.storage
            .get_custom_tool_by_name(name_or_id)
            .await
            .map_err(internal)?
            .ok_or_else(|| ToolError::NotFound(format!("tool `{name_or_id}` not found")))
    }

    pub async fn resolve_active_version(
        &self,
        name_or_id: &str,
    ) -> Result<(CustomTool, CustomToolVersion), ToolError> {
        let tool = self.resolve_tool(name_or_id).await?;
        let ver = self
            .storage
            .get_active_custom_tool_version(&tool)
            .await
            .map_err(internal)?
            .ok_or_else(|| ToolError::Invalid("tool has no active version".to_string()))?;
        Ok((tool, ver))
    }
}

/// What the build did, for the API and the UI.
fn build_report(outcome: &BuildOutcome) -> Value {
    let last = &outcome.last_sandbox;
    json!({
        "activated": outcome.activated,
        "failure": outcome.failure,
        "error_type": last.error_type,
        "message": last.message,
        "traceback": last.traceback,
        "logs": last.logs.chars().take(4000).collect::<String>(),
        "repairs": outcome.repair_history,
        "tests": outcome.test_cases.len(),
        "code": outcome.code,
    })
}

fn outcome_summary(outcome: &BuildOutcome) -> Value {
    json!({
        "activated": outcome.activated,
        "repair_attempts": outcome.repair_history.len(),
        "duration_ms": outcome.last_sandbox.duration_ms,
    })
}

fn sandbox_to_json(r: &sandbox::SandboxResult) -> Value {
    json!({
        "success": r.success,
        "output": r.output,
        "error_type": r.error_type,
        "message": r.message,
        "traceback": r.traceback,
        "duration_ms": r.duration_ms,
        "logs": r.logs,
    })
}

#[async_trait]
pub trait CustomToolGateway: Send + Sync {
    async fn run_by_name(&self, name: &str, input: Value) -> Result<Value, String>;
}

#[async_trait]
impl CustomToolGateway for ToolService {
    async fn run_by_name(&self, name: &str, input: Value) -> Result<Value, String> {
        Ok(self.run_tool(name, input).await?)
    }
}

pub struct NullCustomToolGateway;

#[async_trait]
impl CustomToolGateway for NullCustomToolGateway {
    async fn run_by_name(&self, _name: &str, _input: Value) -> Result<Value, String> {
        Err("custom tools not configured".into())
    }
}

pub type SharedToolService = Arc<ToolService>;
