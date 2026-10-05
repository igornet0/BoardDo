//! Isolated Python subprocess execution for custom tools.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use boarddo_shared::v2::{ToolPermissions, ToolRuntimeConfig};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

const HARNESS: &str = r#"
import json
import sys
import traceback

# --- user tool ---
USER_CODE_PLACEHOLDER
# --- end user tool ---

if __name__ == "__main__":
    try:
        raw = sys.stdin.read()
        envelope = json.loads(raw) if raw.strip() else {}
        input_data = envelope.get("input", envelope)
        result = run(input_data)
        print(json.dumps({"success": True, "output": result}), flush=True)
    except Exception as e:
        print(
            json.dumps({
                "success": False,
                "error": {
                    "type": type(e).__name__,
                    "message": str(e),
                    "traceback": traceback.format_exc(),
                },
            }),
            flush=True,
        )
"#;

#[derive(Debug, Clone)]
pub struct SandboxResult {
    pub success: bool,
    pub output: Option<Value>,
    pub error_type: Option<String>,
    pub message: Option<String>,
    pub traceback: Option<String>,
    pub logs: String,
    pub duration_ms: u64,
}

pub fn python3_path() -> Option<PathBuf> {
    which_python()
}

fn which_python() -> Option<PathBuf> {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| PathBuf::from("python3"))
}

pub async fn validate_syntax(source: &str) -> Result<(), String> {
    let py = python3_path().ok_or("python3 not found")?;
    let script = HARNESS.replace("USER_CODE_PLACEHOLDER", source);
    let dir = tempfile_dir()?;
    let path = dir.join("check_tool.py");
    tokio::fs::write(&path, &script)
        .await
        .map_err(|e| e.to_string())?;
    let out = Command::new(&py)
        .arg("-m")
        .arg("py_compile")
        .arg(&path)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into())
    }
}

pub async fn execute_python(
    source: &str,
    input: Value,
    _permissions: &ToolPermissions,
    config: &ToolRuntimeConfig,
) -> SandboxResult {
    let started = Instant::now();
    let py = match python3_path() {
        Some(p) => p,
        None => {
            return SandboxResult {
                success: false,
                output: None,
                error_type: Some("EnvironmentError".into()),
                message: Some("python3 not found".into()),
                traceback: None,
                logs: String::new(),
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };

    let dir = match tempfile_dir() {
        Ok(d) => d,
        Err(e) => {
            return SandboxResult {
                success: false,
                output: None,
                error_type: Some("IOError".into()),
                message: Some(e),
                traceback: None,
                logs: String::new(),
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };

    let script = HARNESS.replace("USER_CODE_PLACEHOLDER", source);
    let path = dir.join("tool_run.py");
    if tokio::fs::write(&path, &script).await.is_err() {
        return fail(started, "failed to write sandbox script");
    }

    let timeout_d = Duration::from_secs(config.timeout_secs.max(1) as u64);
    let max_out = (config.max_stdout_kb.max(64) as usize) * 1024;
    let stdin_payload = json!({ "input": input });

    let mut child = match Command::new(&py)
        .arg(&path)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return fail(started, &e.to_string()),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let body = stdin_payload.to_string();
        let _ = stdin.write_all(body.as_bytes()).await;
    }

    let wait = timeout(timeout_d, async move { child.wait_with_output().await });
    let output = match wait.await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return fail(started, &e.to_string()),
        Err(_) => {
            return SandboxResult {
                success: false,
                output: None,
                error_type: Some("TimeoutError".into()),
                message: Some(format!("execution exceeded {}s", config.timeout_secs)),
                traceback: None,
                logs: String::new(),
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };

    let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if stdout.len() > max_out {
        stdout.truncate(max_out);
        stdout.push_str("\n…[truncated]");
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let logs = if stderr.is_empty() {
        stdout.clone()
    } else {
        format!("{stdout}\n--- stderr ---\n{stderr}")
    };

    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or(stdout.as_str());

    let parsed: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(_) => {
            return SandboxResult {
                success: false,
                output: None,
                error_type: Some("ParseError".into()),
                message: Some("invalid JSON from sandbox".into()),
                traceback: Some(stdout),
                logs,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };

    if parsed
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        SandboxResult {
            success: true,
            output: parsed.get("output").cloned(),
            error_type: None,
            message: None,
            traceback: None,
            logs,
            duration_ms: started.elapsed().as_millis() as u64,
        }
    } else {
        let err = parsed.get("error");
        SandboxResult {
            success: false,
            output: None,
            error_type: err
                .and_then(|e| e.get("type"))
                .and_then(Value::as_str)
                .map(str::to_string),
            message: err
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string),
            traceback: err
                .and_then(|e| e.get("traceback"))
                .and_then(Value::as_str)
                .map(str::to_string),
            logs,
            duration_ms: started.elapsed().as_millis() as u64,
        }
    }
}

fn fail(started: Instant, msg: &str) -> SandboxResult {
    SandboxResult {
        success: false,
        output: None,
        error_type: Some("SandboxError".into()),
        message: Some(msg.into()),
        traceback: None,
        logs: String::new(),
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

fn tempfile_dir() -> Result<PathBuf, String> {
    let base = std::env::temp_dir().join(format!("boarddo-tool-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    Ok(base)
}
