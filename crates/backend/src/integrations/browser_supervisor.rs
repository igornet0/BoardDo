//! Runs the bundled RustBrowser (`vendor/RustBrowser`, built by `make browser`)
//! on demand: started on the first render, stopped after an idle period,
//! restarted if it dies. One process serves every board and agent.
//!
//! Environment:
//! - `BROWSER_BIN` — explicit path to `rust-browser`;
//! - `BROWSER_IDLE_SECS` (600) — stop after this long without renders;
//! - `BROWSER_MAX_TABS` (4) — parallel pages inside the browser;
//! - `BROWSER_LOG` (`warn`) — `RUST_LOG` for the browser process.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use boarddo_shared::v2::{WebError, WebPolicy};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde_json::{Value, json};
use tokio::process::{Child, ChildStdin, Command};

use super::browser::{BrowserRenderer, HttpBrowserRenderer, RenderRequest, RenderResult};

const DEFAULT_IDLE_SECS: u64 = 600;
const DEFAULT_MAX_TABS: usize = 4;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const REAPER_EVERY: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Paths tried in order; the first existing file is used (checked on every start,
    /// so `make browser` works without restarting BoardDo).
    pub candidates: Vec<PathBuf>,
    pub idle_timeout: Duration,
    pub max_tabs: usize,
    pub log_level: String,
}

struct Running {
    child: Child,
    /// Held open on purpose: the browser exits when BoardDo's end of the pipe closes.
    _stdin: Option<ChildStdin>,
    client: HttpBrowserRenderer,
    binary: PathBuf,
    port: u16,
    pid: Option<u32>,
    started_at: DateTime<Utc>,
}

pub struct BrowserSupervisor {
    config: SupervisorConfig,
    me: Weak<BrowserSupervisor>,
    running: tokio::sync::Mutex<Option<Running>>,
    active: AtomicUsize,
    last_used: Mutex<Instant>,
    starts: AtomicU64,
    reaper: Mutex<bool>,
}

impl BrowserSupervisor {
    pub fn new(config: SupervisorConfig) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            config,
            me: me.clone(),
            running: tokio::sync::Mutex::new(None),
            active: AtomicUsize::new(0),
            last_used: Mutex::new(Instant::now()),
            starts: AtomicU64::new(0),
            reaper: Mutex::new(false),
        })
    }

    pub fn from_env() -> Arc<Self> {
        let num = |key: &str, default: u64| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(default)
        };
        Self::new(SupervisorConfig {
            candidates: default_candidates(),
            idle_timeout: Duration::from_secs(num("BROWSER_IDLE_SECS", DEFAULT_IDLE_SECS).max(30)),
            max_tabs: num("BROWSER_MAX_TABS", DEFAULT_MAX_TABS as u64).clamp(1, 16) as usize,
            log_level: std::env::var("BROWSER_LOG").unwrap_or_else(|_| "warn".into()),
        })
    }

    /// The `rust-browser` binary, if it has been built.
    pub fn binary(&self) -> Option<PathBuf> {
        self.config.candidates.iter().find(|p| p.is_file()).cloned()
    }

    fn touch(&self) {
        if let Ok(mut t) = self.last_used.lock() {
            *t = Instant::now();
        }
    }

    fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or_default()
    }

    /// The client of a live browser process, starting one if needed.
    async fn ensure_running(&self) -> Result<HttpBrowserRenderer, WebError> {
        let mut guard = self.running.lock().await;
        if let Some(running) = guard.as_mut() {
            match running.child.try_wait() {
                Ok(None) => return Ok(running.client.clone()),
                Ok(Some(status)) => {
                    tracing::warn!(%status, "browser process exited — restarting");
                }
                Err(e) => tracing::warn!(error = %e, "browser process state unknown — restarting"),
            }
            *guard = None;
        }
        let binary = self.binary().ok_or_else(|| {
            WebError::Upstream(
                "RustBrowser is not built: run `make browser` (or set BROWSER_BIN / \
                 BROWSER_AUTOMATION_URL)"
                    .into(),
            )
        })?;
        let running = self.spawn(binary).await?;
        let client = running.client.clone();
        *guard = Some(running);
        drop(guard);
        self.start_reaper();
        Ok(client)
    }

    async fn spawn(&self, binary: PathBuf) -> Result<Running, WebError> {
        let port = free_port()?;
        let token = random_token();
        let addr = format!("127.0.0.1:{port}");
        let mut child = Command::new(&binary)
            .arg("--automation-server")
            .arg(&addr)
            .arg("--automation-max-tabs")
            .arg(self.config.max_tabs.to_string())
            .arg("--automation-exit-on-stdin-eof")
            // Secrets go through the environment, not argv (visible in `ps`).
            .env("RUST_BROWSER_AUTOMATION_TOKEN", &token)
            .env("RUST_LOG", &self.config.log_level)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| WebError::Upstream(format!("browser: cannot start {}: {e}", binary.display())))?;
        let pid = child.id();
        let stdin = child.stdin.take();
        let client = HttpBrowserRenderer::new(format!("http://{addr}"), Some(token));
        tracing::info!(pid, %addr, binary = %binary.display(), "browser process starting");

        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(WebError::Upstream(format!(
                    "browser: process exited during startup ({status})"
                )));
            }
            if client.health().await.is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.start_kill();
                return Err(WebError::Upstream("browser: did not become healthy in time".into()));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        self.starts.fetch_add(1, Ordering::Relaxed);
        tracing::info!(pid, %addr, "browser process ready");
        Ok(Running {
            child,
            _stdin: stdin,
            client,
            binary,
            port,
            pid,
            started_at: Utc::now(),
        })
    }

    /// Background task that stops the browser after `idle_timeout` without renders.
    fn start_reaper(&self) {
        {
            let Ok(mut started) = self.reaper.lock() else {
                return;
            };
            if *started {
                return;
            }
            *started = true;
        }
        let me = self.me.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(REAPER_EVERY).await;
                let Some(sup) = me.upgrade() else {
                    return;
                };
                if sup.active.load(Ordering::SeqCst) == 0 && sup.idle_for() >= sup.config.idle_timeout {
                    if sup.stop("idle").await {
                        tracing::info!(idle_secs = sup.config.idle_timeout.as_secs(), "browser stopped (idle)");
                    }
                }
            }
        });
    }

    /// Stop the browser process; `true` if one was running.
    pub async fn stop(&self, reason: &str) -> bool {
        let Some(mut running) = self.running.lock().await.take() else {
            return false;
        };
        tracing::info!(pid = running.pid, reason, "stopping browser process");
        // Content processes exit on their own when the IPC socket closes.
        let _ = running.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), running.child.wait()).await;
        true
    }

    async fn process_alive(&self) -> bool {
        let mut guard = self.running.lock().await;
        match guard.as_mut() {
            Some(r) => matches!(r.child.try_wait(), Ok(None)),
            None => false,
        }
    }
}

/// Keeps `active` / `last_used` right even if a render future is dropped.
struct ActiveGuard<'a>(&'a BrowserSupervisor);

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
        self.0.touch();
    }
}

#[async_trait]
impl BrowserRenderer for BrowserSupervisor {
    fn is_configured(&self) -> bool {
        self.binary().is_some()
    }

    async fn render(
        &self,
        request: RenderRequest,
        policy: &WebPolicy,
    ) -> Result<RenderResult, WebError> {
        self.active.fetch_add(1, Ordering::SeqCst);
        self.touch();
        let _guard = ActiveGuard(self);
        let mut attempt = 0;
        loop {
            let client = self.ensure_running().await?;
            match client.render(request.clone(), policy).await {
                // The process died under us: start a fresh one and retry once.
                Err(e) if attempt == 0 && !self.process_alive().await => {
                    tracing::warn!(error = %e, "browser died during render — restarting once");
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    async fn status(&self) -> Value {
        let binary = self.binary();
        let mut guard = self.running.lock().await;
        let alive = guard
            .as_mut()
            .is_some_and(|r| matches!(r.child.try_wait(), Ok(None)));
        let running = guard.as_ref().filter(|_| alive);
        let mut out = json!({
            "mode": "managed",
            "available": binary.is_some(),
            "binary": binary.map(|p| p.display().to_string()),
            "state": if running.is_some() { "running" } else { "stopped" },
            "active_renders": self.active.load(Ordering::SeqCst),
            "idle_secs": self.idle_for().as_secs(),
            "idle_timeout_secs": self.config.idle_timeout.as_secs(),
            "max_tabs": self.config.max_tabs,
            "starts": self.starts.load(Ordering::Relaxed),
        });
        if let Some(r) = running {
            out["pid"] = json!(r.pid);
            out["port"] = json!(r.port);
            out["started_at"] = json!(r.started_at.to_rfc3339());
            out["running_binary"] = json!(r.binary.display().to_string());
            out["health"] = r.client.health().await.unwrap_or(Value::Null);
        }
        if out["available"] == false {
            out["hint"] = json!("run `make browser` to build vendor/RustBrowser");
        }
        out
    }

    async fn shutdown(&self) {
        self.stop("shutdown").await;
    }
}

fn free_port() -> Result<u16, WebError> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| WebError::Upstream(format!("browser: no free port: {e}")))
}

fn random_token() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// `BROWSER_BIN`, then `make browser`'s output, then a manual build in the submodule.
fn default_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(bin) = std::env::var("BROWSER_BIN") {
        if !bin.trim().is_empty() {
            out.push(PathBuf::from(bin.trim()));
        }
    }
    let target = std::env::var("BROWSER_TARGET_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".cache/boarddo/rust-browser")));
    if let Some(target) = target {
        out.push(target.join("release/rust-browser"));
        out.push(target.join("debug/rust-browser"));
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.extend(cwd.ancestors().take(4).map(PathBuf::from));
    }
    if let Ok(exe) = std::env::current_exe() {
        roots.extend(exe.ancestors().skip(1).take(5).map(PathBuf::from));
    }
    for root in roots {
        let base = root.join("vendor/RustBrowser/target");
        for profile in ["release", "debug"] {
            let p = base.join(profile).join("rust-browser");
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supervisor(candidates: Vec<PathBuf>) -> Arc<BrowserSupervisor> {
        BrowserSupervisor::new(SupervisorConfig {
            candidates,
            idle_timeout: Duration::from_secs(60),
            max_tabs: 2,
            log_level: "warn".into(),
        })
    }

    #[tokio::test]
    async fn not_built_reports_hint_and_errors_on_render() {
        let sup = supervisor(vec![PathBuf::from("/nonexistent/rust-browser")]);
        assert!(!sup.is_configured());
        let status = sup.status().await;
        assert_eq!(status["mode"], "managed");
        assert_eq!(status["state"], "stopped");
        assert!(status["hint"].as_str().unwrap().contains("make browser"));
        let err = sup
            .render(RenderRequest::default(), &WebPolicy::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("make browser"), "{err}");
        assert_eq!(sup.active.load(Ordering::SeqCst), 0, "guard released");
    }

    #[test]
    fn first_existing_candidate_wins() {
        let dir = std::env::temp_dir().join(format!("boarddo-sup-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("rust-browser");
        std::fs::write(&bin, b"").unwrap();
        let sup = supervisor(vec![dir.join("missing"), bin.clone()]);
        assert_eq!(sup.binary(), Some(bin));
        assert!(sup.is_configured());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn broken_binary_fails_cleanly() {
        let dir = std::env::temp_dir().join(format!("boarddo-sup-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("rust-browser");
        // Exists but is not runnable: spawn must fail with a clear error, not hang.
        std::fs::write(&bin, b"not a program").unwrap();
        let sup = supervisor(vec![bin]);
        let err = sup
            .render(RenderRequest::default(), &WebPolicy::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cannot start"), "{err}");
        assert_eq!(sup.status().await["state"], "stopped");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn defaults_include_make_browser_output() {
        let c = default_candidates();
        assert!(c.iter().any(|p| p.ends_with(".cache/boarddo/rust-browser/release/rust-browser")));
        assert!(c.iter().any(|p| p.ends_with("vendor/RustBrowser/target/release/rust-browser")));
    }

    #[test]
    fn tokens_are_random_hex() {
        let (a, b) = (random_token(), random_token());
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
