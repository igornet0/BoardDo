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

use super::builder::{self, BuildOutcome};
use super::sandbox;
use super::security;

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

    pub async fn get_detail(&self, id: Uuid) -> Result<Value, String> {
        let tool = self
            .get(id)
            .await?
            .ok_or_else(|| "tool not found".to_string())?;
        let versions = self.storage.list_custom_tool_versions(id).await.map_err(|e| e.to_string())?;
        let executions = self
            .storage
            .list_custom_tool_executions(id, 50)
            .await
            .map_err(|e| e.to_string())?;
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
    ) -> Result<Value, String> {
        if self
            .storage
            .get_custom_tool_by_name(&req.name)
            .await
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err(format!("tool `{}` already exists", req.name));
        }

        let outcome = builder::build_and_test_version(req.code.clone(), &req).await?;
        if !outcome.activated {
            return Err(format!(
                "TOOL_BUILD_FAILED: {}",
                outcome
                    .last_sandbox
                    .message
                    .unwrap_or_else(|| "validation failed".into())
            ));
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
            .map_err(|e| e.to_string())?;
        self.storage
            .insert_custom_tool_version(&ver)
            .await
            .map_err(|e| e.to_string())?;

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
    ) -> Result<Value, String> {
        let tool = self.resolve_tool(name_or_id).await?;
        if tool.origin == ToolOrigin::System {
            return Err("cannot modify SYSTEM tool".into());
        }

        let prev_active = self
            .storage
            .get_active_custom_tool_version(&tool)
            .await
            .map_err(|e| e.to_string())?;

        let version_num = self
            .storage
            .next_custom_tool_version(tool.id)
            .await
            .map_err(|e| e.to_string())?;

        let outcome = builder::build_and_test_version(req.code.clone(), &req).await?;
        let mut ver = builder::version_from_build(tool.id, version_num, &req, &outcome);

        if outcome.activated {
            self.storage
                .demote_active_versions(tool.id)
                .await
                .map_err(|e| e.to_string())?;
            ver.status = ToolVersionStatus::Active;
        } else if prev_active.is_some() {
            // keep previous active — never replace working version with broken code
            ver.status = ToolVersionStatus::Failed;
        }

        self.storage
            .insert_custom_tool_version(&ver)
            .await
            .map_err(|e| e.to_string())?;

        let mut tool = tool;
        if outcome.activated {
            tool.active_version_id = Some(ver.id);
        }
        tool.updated_at = Utc::now();
        tool.description = req.description;
        self.storage
            .update_custom_tool_meta(&tool)
            .await
            .map_err(|e| e.to_string())?;

        let _ = actor;
        Ok(json!({
            "tool": tool,
            "version": ver,
            "build": outcome_summary(&outcome),
            "rolled_back": !outcome.activated,
        }))
    }

    pub async fn test_tool(&self, name_or_id: &str, input: Value) -> Result<Value, String> {
        let (tool, ver) = self.resolve_active_version(name_or_id).await?;
        security::scan_python_source(&ver.source_code, &ver.permissions)?;
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

    pub async fn run_tool(&self, name_or_id: &str, input: Value) -> Result<Value, String> {
        let (tool, ver) = self.resolve_active_version(name_or_id).await?;
        if ver.status != ToolVersionStatus::Active {
            return Err("no ACTIVE version".into());
        }
        security::validate_requirements(&ver.requirements)?;
        security::scan_python_source(&ver.source_code, &ver.permissions)?;

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
            .map_err(|e| e.to_string())?;
        self.storage
            .record_custom_tool_stats(
                tool.id,
                result.success,
                result.duration_ms,
                result.message.clone(),
            )
            .await
            .map_err(|e| e.to_string())?;

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

    pub async fn delete_tool(&self, name_or_id: &str) -> Result<Value, String> {
        let tool = self.resolve_tool(name_or_id).await?;
        if tool.origin == ToolOrigin::System {
            return Err("cannot delete SYSTEM tool".into());
        }
        self.storage
            .delete_custom_tool(tool.id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(json!({ "deleted": tool.id }))
    }

    async fn resolve_tool(&self, name_or_id: &str) -> Result<CustomTool, String> {
        if let Ok(id) = Uuid::parse_str(name_or_id) {
            if let Some(t) = self.get(id).await? {
                return Ok(t);
            }
        }
        self.storage
            .get_custom_tool_by_name(name_or_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("tool `{name_or_id}` not found"))
    }

    async fn resolve_active_version(
        &self,
        name_or_id: &str,
    ) -> Result<(CustomTool, CustomToolVersion), String> {
        let tool = self.resolve_tool(name_or_id).await?;
        let ver = self
            .storage
            .get_active_custom_tool_version(&tool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "tool has no active version".to_string())?;
        Ok((tool, ver))
    }
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
        self.run_tool(name, input).await
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
