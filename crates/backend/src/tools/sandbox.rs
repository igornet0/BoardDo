//! Python subprocess execution for custom tools.
//!
//! Each run gets a fresh temporary working directory (removed afterwards), a
//! cleared environment and the fixed `harness.py`, which receives the source,
//! input, permissions and limits on stdin. The harness checks the AST, installs
//! an audit hook that enforces the permissions, and writes one JSON result line
//! to stdout; the tool's own prints go to stderr (logs). BoardDo enforces the
//! wall-clock timeout and the memory limit (RSS watchdog; `RLIMIT_AS` on Linux).
//!
//! This is defence in depth for a local subprocess, not a container: it stops
//! the usual escape routes, but tools still run with the user's privileges.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use boarddo_shared::v2::{ToolPermissions, ToolRuntimeConfig};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const HARNESS: &str = include_str!("harness.py");
const MEMORY_POLL: Duration = Duration::from_millis(100);
const STDERR_LIMIT: usize = 64 * 1024;

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

impl SandboxResult {
    fn error(started: Instant, kind: &str, message: impl Into<String>) -> Self {
        Self {
            success: false,
            output: None,
            error_type: Some(kind.into()),
            message: Some(message.into()),
            traceback: None,
            logs: String::new(),
            duration_ms: started.elapsed().as_millis() as u64,
        }
    }
}

/// The interpreter chosen for tools (see [`super::runtime`]).
pub fn python3_path() -> Option<PathBuf> {
    super::runtime::current().map(|i| i.path)
}

/// Static check only: syntax, forbidden constructs for these permissions, `run()` present.
pub async fn check_source(source: &str, permissions: &ToolPermissions) -> Result<(), String> {
    let result = invoke(source, Value::Null, permissions, &ToolRuntimeConfig::default(), "check").await;
    if result.success {
        Ok(())
    } else {
        Err(format!(
            "{}: {}",
            result.error_type.as_deref().unwrap_or("Error"),
            result.message.as_deref().unwrap_or("invalid tool source")
        ))
    }
}

pub async fn execute_python(
    source: &str,
    input: Value,
    permissions: &ToolPermissions,
    config: &ToolRuntimeConfig,
) -> SandboxResult {
    invoke(source, input, permissions, config, "run").await
}

async fn invoke(
    source: &str,
    input: Value,
    permissions: &ToolPermissions,
    config: &ToolRuntimeConfig,
    mode: &str,
) -> SandboxResult {
    let started = Instant::now();
    let Some(py) = python3_path() else {
        return SandboxResult::error(started, "EnvironmentError", "no Python 3 found — install Python or choose an interpreter in Tools");
    };
    let dir = match WorkDir::new() {
        Ok(d) => d,
        Err(e) => return SandboxResult::error(started, "IOError", e),
    };
    let script = dir.0.join("harness.py");
    if let Err(e) = tokio::fs::write(&script, HARNESS).await {
        return SandboxResult::error(started, "IOError", e.to_string());
    }

    let envelope = json!({
        "mode": mode,
        "source": source,
        "input": input,
        "permissions": permissions,
        "limits": { "memory_mb": config.memory_mb, "timeout_secs": config.timeout_secs },
    });
    let mut cmd = Command::new(&py);
    cmd.arg(&script)
        .current_dir(&dir.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("TMPDIR", &dir.0)
        .kill_on_drop(true);
    // HOME keeps user-installed packages importable; reads there are still gated by the hook.
    for key in ["PATH", "HOME", "LANG"] {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return SandboxResult::error(started, "SandboxError", e.to_string()),
    };
    let pid = child.id();

    // Feed stdin and drain both pipes concurrently so large inputs/outputs never deadlock.
    let mut stdin = child.stdin.take();
    let body = envelope.to_string().into_bytes();
    let feed = async move {
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(&body).await;
            let _ = stdin.shutdown().await;
        }
        drop(stdin);
    };
    let max_out = (config.max_stdout_kb.max(64) as usize) * 1024;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let read_out = async move {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(max_out as u64 + 1).read_to_end(&mut buf).await;
        // Keep draining so the child never blocks on a full pipe.
        let _ = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await;
        buf
    };
    let read_err = async move {
        let mut buf = Vec::new();
        let _ = (&mut stderr).take(STDERR_LIMIT as u64).read_to_end(&mut buf).await;
        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        buf
    };

    let memory_limit_kb = u64::from(config.memory_mb) * 1024;
    let watchdog = async move {
        let Some(pid) = pid else { return std::future::pending::<u64>().await };
        loop {
            tokio::time::sleep(MEMORY_POLL).await;
            if let Some(rss) = rss_kb(pid).await {
                if memory_limit_kb > 0 && rss > memory_limit_kb {
                    return rss;
                }
            }
        }
    };

    let timeout = Duration::from_secs(config.timeout_secs.max(1) as u64);
    let run = async {
        let (_, out, err, status) = tokio::join!(feed, read_out, read_err, child.wait());
        (out, err, status)
    };
    enum Ended {
        Done(Vec<u8>, Vec<u8>),
        Timeout,
        Memory(u64),
    }
    let ended = tokio::select! {
        r = tokio::time::timeout(timeout, run) => match r {
            Ok((out, err, _status)) => Ended::Done(out, err),
            Err(_) => Ended::Timeout,
        },
        rss = watchdog => Ended::Memory(rss),
    };
    // The child is killed on drop (timeout / memory) — make sure it is gone now.
    if !matches!(ended, Ended::Done(..)) {
        if let Some(pid) = pid {
            kill(pid);
        }
    }

    let (stdout, stderr) = match ended {
        Ended::Done(out, err) => (out, err),
        Ended::Timeout => {
            return SandboxResult::error(
                started,
                "TimeoutError",
                format!("execution exceeded {}s", config.timeout_secs),
            );
        }
        Ended::Memory(rss) => {
            return SandboxResult::error(
                started,
                "MemoryError",
                format!(
                    "the tool used {} MB, over its limit of {} MB",
                    rss / 1024,
                    config.memory_mb
                ),
            );
        }
    };

    let logs = String::from_utf8_lossy(&stderr).into_owned();
    if stdout.len() > max_out {
        return SandboxResult {
            logs,
            ..SandboxResult::error(
                started,
                "OutputError",
                format!("the result is larger than {} KB", config.max_stdout_kb.max(64)),
            )
        };
    }
    let stdout = String::from_utf8_lossy(&stdout);
    let Some(line) = stdout.lines().rev().find(|l| !l.trim().is_empty()) else {
        return SandboxResult {
            traceback: Some(logs.chars().rev().take(4000).collect::<String>().chars().rev().collect()),
            logs,
            ..SandboxResult::error(started, "SandboxError", "the tool process ended without a result (crashed or was killed)")
        };
    };
    let parsed: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(e) => {
            return SandboxResult {
                logs,
                ..SandboxResult::error(started, "ParseError", format!("invalid result from the harness: {e}"))
            };
        }
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    if parsed.get("success").and_then(Value::as_bool).unwrap_or(false) {
        SandboxResult {
            success: true,
            output: parsed.get("output").cloned(),
            error_type: None,
            message: None,
            traceback: None,
            logs,
            duration_ms,
        }
    } else {
        let err = parsed.get("error");
        let text = |k: &str| err.and_then(|e| e.get(k)).and_then(Value::as_str).map(str::to_string);
        SandboxResult {
            success: false,
            output: None,
            error_type: text("type"),
            message: text("message"),
            traceback: text("traceback"),
            logs,
            duration_ms,
        }
    }
}

/// Resident memory of a process in KB.
async fn rss_kb(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let statm = tokio::fs::read_to_string(format!("/proc/{pid}/statm")).await.ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        return Some(pages * 4);
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let out = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .await
            .ok()?;
        return String::from_utf8_lossy(&out.stdout).trim().parse().ok();
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

fn kill(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

/// Temporary working directory, removed when dropped.
struct WorkDir(PathBuf);

impl WorkDir {
    fn new() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("boarddo-tool-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self(dir))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
