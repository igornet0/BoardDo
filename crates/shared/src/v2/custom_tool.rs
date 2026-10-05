//! AI / user custom tools — Python sandbox execution with versioning.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLanguage {
    Python,
}

impl ToolLanguage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Python => "python",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            _ => Self::Python,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ToolOrigin {
    System,
    User,
    Agent,
}

impl ToolOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "SYSTEM",
            Self::User => "USER",
            Self::Agent => "AGENT",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "SYSTEM" => Self::System,
            "USER" => Self::User,
            _ => Self::Agent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ToolVersionStatus {
    Draft,
    Testing,
    Active,
    Failed,
    Disabled,
    Archived,
}

impl ToolVersionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Testing => "TESTING",
            Self::Active => "ACTIVE",
            Self::Failed => "FAILED",
            Self::Disabled => "DISABLED",
            Self::Archived => "ARCHIVED",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "TESTING" => Self::Testing,
            "ACTIVE" => Self::Active,
            "FAILED" => Self::Failed,
            "DISABLED" => Self::Disabled,
            "ARCHIVED" => Self::Archived,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolPermissions {
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub filesystem_read: bool,
    #[serde(default)]
    pub filesystem_write: bool,
    #[serde(default)]
    pub shell: bool,
    #[serde(default)]
    pub database: bool,
}

impl ToolPermissions {
    pub fn locked_down() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolRuntimeConfig {
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u32,
    #[serde(default = "default_cpu_cores")]
    pub cpu_cores: u32,
    #[serde(default = "default_max_stdout_kb")]
    pub max_stdout_kb: u32,
}

fn default_timeout_secs() -> u32 {
    30
}
fn default_memory_mb() -> u32 {
    256
}
fn default_cpu_cores() -> u32 {
    1
}
fn default_max_stdout_kb() -> u32 {
    512
}

impl Default for ToolRuntimeConfig {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout_secs(),
            memory_mb: default_memory_mb(),
            cpu_cores: default_cpu_cores(),
            max_stdout_kb: default_max_stdout_kb(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolExecutionStats {
    #[serde(default)]
    pub execution_count: u64,
    #[serde(default)]
    pub success_count: u64,
    #[serde(default)]
    pub failure_count: u64,
    #[serde(default)]
    pub average_duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<DateTime<Utc>>,
}

impl Default for ToolExecutionStats {
    fn default() -> Self {
        Self {
            execution_count: 0,
            success_count: 0,
            failure_count: 0,
            average_duration_ms: 0,
            last_error: None,
            last_success_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolTestCase {
    pub input: Value,
    #[serde(default)]
    pub expected_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolRepairRecord {
    pub attempt: u32,
    pub error_message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceback: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTool {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub language: ToolLanguage,
    pub origin: ToolOrigin,
    pub created_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_version_id: Option<Uuid>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub execution_stats: ToolExecutionStats,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomToolVersion {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub version: u32,
    pub source_code: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub permissions: ToolPermissions,
    #[serde(default)]
    pub requirements: Vec<String>,
    pub runtime_config: ToolRuntimeConfig,
    pub status: ToolVersionStatus,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub specification: Value,
    #[serde(default)]
    pub test_cases: Vec<ToolTestCase>,
    #[serde(default)]
    pub repair_history: Vec<ToolRepairRecord>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolExecutionRecord {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub version: u32,
    pub status: String,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceback: Option<String>,
    #[serde(default)]
    pub logs: String,
    #[serde(default)]
    pub resource_usage: Map<String, Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateToolRequest {
    pub name: String,
    pub description: String,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub output_schema: Value,
    #[serde(default)]
    pub requirements: Vec<String>,
    pub code: String,
    #[serde(default)]
    pub permissions: ToolPermissions,
    #[serde(default)]
    pub test_input: Option<Value>,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_lang() -> String {
    "python".into()
}

pub fn custom_tool_capability(type_id: &str) -> Option<&'static str> {
    use super::capability::caps;
    match type_id {
        "tool.create"
        | "create_tool"
        | "tool.update"
        | "update_tool"
        | "tool.test"
        | "test_tool"
        | "tool.delete"
        | "delete_tool"
        | "tool.find"
        | "find_tools" => Some(caps::TOOL_BUILD),
        "tool.run" | "run_tool" | "tool.list" | "list_tools" | "tool.get" | "get_tool" => {
            Some(caps::TOOL_EXECUTE)
        }
        _ => None,
    }
}

pub const MAX_REPAIR_ATTEMPTS: u32 = 3;

pub const DEFAULT_PY_ALLOWLIST: &[&str] = &[
    "json",
    "math",
    "statistics",
    "re",
    "datetime",
    "collections",
    "itertools",
    "functools",
    "typing",
    "decimal",
    "pandas",
    "numpy",
    "requests",
    "beautifulsoup4",
    "bs4",
];
