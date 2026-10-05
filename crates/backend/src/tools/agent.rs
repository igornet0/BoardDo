//! Agent-facing tool.* operations.

use boarddo_shared::v2::{CreateToolRequest, ToolOrigin, ToolPermissions};
use serde_json::{Value, json};

use crate::state::SharedState;
use crate::tools::service::ToolService;

pub async fn execute(
    state: &SharedState,
    type_id: &str,
    config: &Value,
) -> Result<Value, String> {
    let svc = ToolService::new(state.storage.clone());
    match normalize(type_id) {
        "tool.create" => {
            let req = parse_create(config)?;
            svc.create_tool(req, "agent", ToolOrigin::Agent).await
        }
        "tool.update" => {
            let name = str_field(config, "name").or_else(|_| str_field(config, "tool_id"))?;
            let req = parse_create(config)?;
            svc.update_tool(&name, req, "agent").await
        }
        "tool.test" | "tool.test_tool" => {
            let name = tool_ref(config)?;
            let input = config.get("input").cloned().unwrap_or(json!({}));
            svc.test_tool(&name, input).await
        }
        "tool.run" => {
            let name = tool_ref(config)?;
            let input = config.get("input").cloned().unwrap_or(json!({}));
            svc.run_tool(&name, input).await
        }
        "tool.list" => {
            let tools = svc.list().await?;
            Ok(json!({ "tools": tools }))
        }
        "tool.get" => {
            let name = tool_ref(config)?;
            let tool = if let Ok(id) = uuid::Uuid::parse_str(&name) {
                svc.get(id).await?
            } else {
                state
                    .storage
                    .get_custom_tool_by_name(&name)
                    .await
                    .map_err(|e| e.to_string())?
            };
            tool.map(|t| json!({ "tool": t }))
                .ok_or_else(|| "not found".into())
        }
        "tool.find" => {
            let q = config
                .get("query")
                .or_else(|| config.get("capability"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let limit = config.get("limit").and_then(Value::as_u64).unwrap_or(20) as u32;
            let tools = svc.find(q, limit).await?;
            Ok(json!({ "tools": tools }))
        }
        "tool.delete" => {
            let name = tool_ref(config)?;
            svc.delete_tool(&name).await
        }
        other => Err(format!("unknown tool op `{other}`")),
    }
}

fn normalize(type_id: &str) -> &str {
    match type_id {
        "create_tool" => "tool.create",
        "update_tool" => "tool.update",
        "test_tool" => "tool.test",
        "run_tool" => "tool.run",
        "list_tools" => "tool.list",
        "get_tool" => "tool.get",
        "find_tools" => "tool.find",
        "delete_tool" => "tool.delete",
        other => other,
    }
}

fn tool_ref(config: &Value) -> Result<String, String> {
    str_field(config, "name")
        .or_else(|_| str_field(config, "tool_name"))
        .or_else(|_| str_field(config, "tool_id"))
}

fn str_field(config: &Value, key: &str) -> Result<String, String> {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing `{key}`"))
}

fn parse_create(config: &Value) -> Result<CreateToolRequest, String> {
    let code = config
        .get("code")
        .and_then(Value::as_str)
        .ok_or("missing `code`")?
        .to_string();
    Ok(CreateToolRequest {
        name: str_field(config, "name")?,
        description: config
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        language: config
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("python")
            .into(),
        input_schema: config.get("input_schema").cloned().unwrap_or(json!({})),
        output_schema: config.get("output_schema").cloned().unwrap_or(json!({})),
        requirements: config
            .get("requirements")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        code,
        permissions: config
            .get("permissions")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_else(ToolPermissions::locked_down),
        test_input: config.get("test_input").cloned(),
        purpose: config
            .get("purpose")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        tags: config
            .get("tags")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
    })
}
