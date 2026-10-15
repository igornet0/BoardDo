//! ProjectHub extension: projects, code, `.brdd` analysis, tasks and GitHub
//! issues from [Project Hub](https://github.com/igornet0/ProjectShit).
//!
//! Not installed by default. Installing clones (or uses a local checkout of)
//! the repository, builds the headless `project-hub-server`, runs its
//! `--self-test`, and only then lets the user enable it. Enabled, BoardDo runs
//! the server as a sidecar on a random loopback port with a random token and
//! exposes it through `/api/projecthub/*`, `projecthub.*` nodes and goals.
//!
//! A Project Hub that is already on the machine (the desktop app ships the
//! server inside its bundle, or a binary on `PATH`) is detected and adopted
//! after the same self-test — no build needed.
//!
//! Environment: `PROJECTHUB_DIR` (install root, default
//! `~/.cache/boarddo/extensions/projecthub`), `PROJECTHUB_REPO`,
//! `PROJECTHUB_BRANCH` (install defaults), `PROJECTHUB_BIN` (use a prebuilt binary).

pub mod client;
pub mod detect;
mod install;
pub mod tracker;

#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

pub use client::PhClient;
pub use detect::Detection;

use crate::storage::Storage;

pub const EXTENSION_ID: &str = "projecthub";
pub const DEFAULT_REPO: &str = "https://github.com/igornet0/ProjectShit.git";
pub const DEFAULT_BRANCH: &str = "main";
const CONFIG_KEY: &str = "extension:projecthub";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const LOG_LINES: usize = 400;
const DETECT_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionConfig {
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub enabled: bool,
    /// Git URL or a local checkout path.
    #[serde(default = "default_repo")]
    pub repo: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub source_dir: Option<PathBuf>,
    #[serde(default)]
    pub binary: Option<PathBuf>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub installed_at: Option<DateTime<Utc>>,
    /// Last `--self-test` report.
    #[serde(default)]
    pub self_test: Option<Value>,
    /// `off` | `detected` | `any` — project commands the API may run.
    #[serde(default = "default_commands")]
    pub commands: String,
    /// Database override; `None` shares the Project Hub desktop app's database.
    #[serde(default)]
    pub db_path: Option<String>,
    /// Where the binary came from: `build` (BoardDo built it) | `app` | `env` | `path` | `managed`.
    #[serde(default)]
    pub source: Option<String>,
    /// Set by uninstall: do not adopt a detected installation automatically again.
    #[serde(default)]
    pub ignore_detected: bool,
}

fn default_repo() -> String {
    std::env::var("PROJECTHUB_REPO").unwrap_or_else(|_| DEFAULT_REPO.into())
}
fn default_branch() -> String {
    std::env::var("PROJECTHUB_BRANCH").unwrap_or_else(|_| DEFAULT_BRANCH.into())
}
fn default_commands() -> String {
    "detected".into()
}

impl Default for ExtensionConfig {
    fn default() -> Self {
        Self {
            installed: false,
            enabled: false,
            repo: default_repo(),
            branch: default_branch(),
            source_dir: None,
            binary: None,
            version: None,
            installed_at: None,
            self_test: None,
            commands: default_commands(),
            db_path: None,
            source: None,
            ignore_detected: false,
        }
    }
}

impl ExtensionConfig {
    pub fn self_test_ok(&self) -> bool {
        self.self_test
            .as_ref()
            .and_then(|r| r.get("ok"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Installed by BoardDo itself (so uninstall may delete the build).
    pub fn is_managed_build(&self) -> bool {
        matches!(self.source.as_deref(), None | Some("build"))
    }

    pub fn binary_present(&self) -> bool {
        self.binary.as_ref().is_some_and(|b| b.is_file())
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InstallJob {
    pub running: bool,
    /// `prepare` | `fetch` | `build` | `self_test` | `done` | `failed`
    pub phase: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub log: VecDeque<String>,
}

impl InstallJob {
    pub fn line(&mut self, line: impl Into<String>) {
        let line = line.into();
        tracing::debug!(target: "projecthub.install", "{line}");
        self.log.push_back(line);
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }
}

struct Running {
    child: Child,
    /// Held open: the server exits when BoardDo's end of the pipe closes.
    _stdin: Option<ChildStdin>,
    client: PhClient,
    url: String,
    pid: Option<u32>,
    started_at: DateTime<Utc>,
}

pub struct ProjectHub {
    storage: Storage,
    config: tokio::sync::RwLock<ExtensionConfig>,
    install: std::sync::Mutex<InstallJob>,
    running: tokio::sync::Mutex<Option<Running>>,
    last_error: std::sync::Mutex<Option<String>>,
    detection: tokio::sync::Mutex<Option<(Instant, Detection)>>,
}

static HUB: OnceLock<Arc<ProjectHub>> = OnceLock::new();

/// The extension manager (after [`init`]).
pub fn hub() -> Option<Arc<ProjectHub>> {
    HUB.get().cloned()
}

#[cfg(test)]
static TEST_CLIENT: std::sync::RwLock<Option<PhClient>> = std::sync::RwLock::new(None);

/// Tests talk to a mock server instead of a sidecar.
#[cfg(test)]
pub fn set_test_client(client: Option<PhClient>) {
    *TEST_CLIENT.write().unwrap() = client;
}

/// Client of the running server, starting it when enabled but stopped.
pub async fn client() -> Result<PhClient, String> {
    #[cfg(test)]
    if let Some(c) = TEST_CLIENT.read().unwrap().clone() {
        return Ok(c);
    }
    let hub = hub().ok_or("ProjectHub extension is not initialised")?;
    hub.ensure_client().await
}

/// Load the config and start the sidecar when the extension is enabled.
pub async fn init(storage: Storage) -> Arc<ProjectHub> {
    let config = match storage.get_node_state(CONFIG_KEY).await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => ExtensionConfig::default(),
    };
    let enabled = config.enabled && config.installed;
    let hub = Arc::new(ProjectHub {
        storage,
        config: tokio::sync::RwLock::new(config),
        install: std::sync::Mutex::new(InstallJob::default()),
        running: tokio::sync::Mutex::new(None),
        last_error: std::sync::Mutex::new(None),
        detection: tokio::sync::Mutex::new(None),
    });
    let _ = HUB.set(hub.clone());
    let h = hub.clone();
    tokio::spawn(async move {
        // Notice an existing / updated / removed installation before autostart.
        if let Err(err) = h.reconcile().await {
            tracing::warn!(error = %err, "projecthub.detect_failed");
        }
        if enabled {
            if let Err(err) = h.start().await {
                tracing::warn!(error = %err, "projecthub.autostart_failed");
            }
        }
    });
    hub.spawn_watchdog();
    hub
}

/// Install root (`~/.cache/boarddo/extensions/projecthub`): outside the
/// BoardDo tree because vendored C deps dislike paths with spaces.
pub fn install_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("PROJECTHUB_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".cache/boarddo/extensions/projecthub")
}

impl ProjectHub {
    pub async fn config(&self) -> ExtensionConfig {
        self.config.read().await.clone()
    }

    async fn save(&self, config: ExtensionConfig) -> Result<(), String> {
        let raw = serde_json::to_string(&config).map_err(|e| e.to_string())?;
        self.storage
            .set_node_state(CONFIG_KEY, &raw, Utc::now())
            .await
            .map_err(|e| e.to_string())?;
        *self.config.write().await = config;
        Ok(())
    }

    pub async fn update<F: FnOnce(&mut ExtensionConfig)>(&self, f: F) -> Result<ExtensionConfig, String> {
        let mut cfg = self.config().await;
        f(&mut cfg);
        self.save(cfg.clone()).await?;
        Ok(cfg)
    }

    fn set_error(&self, err: Option<String>) {
        if let Ok(mut e) = self.last_error.lock() {
            *e = err;
        }
    }

    pub fn install_job(&self) -> InstallJob {
        self.install.lock().map(|j| j.clone()).unwrap_or_default()
    }

    pub(crate) fn with_job<R>(&self, f: impl FnOnce(&mut InstallJob) -> R) -> Option<R> {
        self.install.lock().ok().map(|mut j| f(&mut j))
    }

    /// Public status for the UI.
    pub async fn status(&self) -> Value {
        let cfg = self.config().await;
        let job = self.install_job();
        let detection = self.detection(false).await;
        let (running, url, pid, started_at) = {
            let mut guard = self.running.lock().await;
            match guard.as_mut() {
                Some(r) => {
                    if matches!(r.child.try_wait(), Ok(None)) {
                        (true, Some(r.url.clone()), r.pid, Some(r.started_at))
                    } else {
                        (false, None, None, None)
                    }
                }
                None => (false, None, None, None),
            }
        };
        let state = if job.running {
            "installing"
        } else if !cfg.installed {
            if job.phase == "failed" {
                "error"
            } else if detection.best().is_some() {
                "detected"
            } else {
                "not_installed"
            }
        } else if !cfg.binary_present() {
            "missing"
        } else if running {
            "running"
        } else if cfg.enabled {
            "starting"
        } else {
            "installed"
        };
        json!({
            "id": EXTENSION_ID,
            "name": "ProjectHub",
            "description": "Projects on this machine: code, .brdd analysis and version tracking, tasks, GitHub issues",
            "homepage": "https://github.com/igornet0/ProjectShit",
            "state": state,
            "installed": cfg.installed,
            "enabled": cfg.enabled,
            "running": running,
            "url": url,
            "pid": pid,
            "started_at": started_at,
            "error": self.last_error.lock().ok().and_then(|e| e.clone()),
            "config": cfg,
            "self_test_ok": cfg.self_test_ok(),
            "install_root": install_root(),
            "install": job,
            "detected": detection,
        })
    }

    /// Kick off install / reinstall in the background.
    pub fn start_install(self: &Arc<Self>, repo: Option<String>, branch: Option<String>) -> Result<(), String> {
        {
            let mut job = self.install.lock().map_err(|_| "install state poisoned")?;
            if job.running {
                return Err("installation is already running".into());
            }
            *job = InstallJob {
                running: true,
                phase: "prepare".into(),
                started_at: Some(Utc::now()),
                ..Default::default()
            };
        }
        let hub = self.clone();
        tokio::spawn(async move {
            let result = install::run(&hub, repo, branch).await;
            let failed = result.is_err();
            hub.with_job(|j| {
                j.running = false;
                j.finished_at = Some(Utc::now());
                match &result {
                    Ok(()) => {
                        j.phase = "done".into();
                        j.line("✓ ProjectHub is installed and passed the self-test");
                    }
                    Err(err) => {
                        j.phase = "failed".into();
                        j.error = Some(err.clone());
                        j.line(format!("✕ {err}"));
                    }
                }
            });
            if failed {
                tracing::warn!("projecthub.install_failed");
            }
        });
        Ok(())
    }

    /// Re-run `--self-test` of the installed binary.
    pub async fn self_test(&self) -> Result<Value, String> {
        let cfg = self.config().await;
        let binary = cfg.binary.clone().filter(|b| b.is_file()).ok_or("ProjectHub is not installed")?;
        let report = install::self_test(&binary).await?;
        self.update(|c| c.self_test = Some(report.clone())).await?;
        Ok(report)
    }

    pub async fn enable(&self) -> Result<(), String> {
        let cfg = self.config().await;
        if !cfg.installed {
            return Err("install ProjectHub first".into());
        }
        if !cfg.self_test_ok() {
            return Err("the self-test has not passed — run it again or reinstall".into());
        }
        self.update(|c| c.enabled = true).await?;
        self.start().await
    }

    pub async fn disable(&self) -> Result<(), String> {
        self.update(|c| c.enabled = false).await?;
        self.stop().await;
        Ok(())
    }

    /// Stop and forget the install. A BoardDo build is deleted; a Project Hub
    /// installed separately (desktop app, PATH) is left alone and not re-adopted.
    pub async fn uninstall(&self) -> Result<(), String> {
        self.stop().await;
        let root = install_root();
        let managed = self.config().await.is_managed_build();
        self.save(ExtensionConfig { ignore_detected: true, ..ExtensionConfig::default() }).await?;
        self.with_job(|j| *j = InstallJob::default());
        *self.detection.lock().await = None;
        if managed
            && root.starts_with(std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".cache"))
            && root.is_dir()
        {
            tokio::fs::remove_dir_all(&root).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// What is installed on this machine (cached for a minute; `refresh` re-scans).
    pub async fn detection(&self, refresh: bool) -> Detection {
        let mut guard = self.detection.lock().await;
        if !refresh {
            if let Some((at, d)) = guard.as_ref() {
                if at.elapsed() < DETECT_TTL {
                    return d.clone();
                }
            }
        }
        let db = self.config().await.db_path;
        let d = detect::detect(db.as_deref()).await;
        *guard = Some((Instant::now(), d.clone()));
        d
    }

    /// Adopt an installed `project-hub-server` (the detected best one, or
    /// `binary` if it is among the detected) after a passing self-test.
    pub async fn use_installed(&self, binary: Option<PathBuf>) -> Result<Value, String> {
        if self.install_job().running {
            return Err("installation is running".into());
        }
        let detection = self.detection(true).await;
        let candidate = match &binary {
            Some(b) => detection
                .find(b)
                .ok_or_else(|| format!("{} is not a detected ProjectHub server", b.display()))?,
            None => detection.best().ok_or("no installed ProjectHub found on this machine")?,
        }
        .clone();
        let report = install::self_test(&candidate.binary).await?;
        let ok = report["ok"].as_bool().unwrap_or(false);
        if !ok {
            return Err(format!(
                "self-test of {} failed: {} of {} steps",
                candidate.binary.display(),
                report["failed"],
                report["steps"].as_array().map(|a| a.len()).unwrap_or(0)
            ));
        }
        let was_running = self.is_running().await;
        let cfg = self
            .update(|c| {
                c.installed = true;
                c.ignore_detected = false;
                c.source = Some(candidate.source.clone());
                c.binary = Some(candidate.binary.clone());
                c.source_dir = None;
                c.version = candidate.version.clone().or_else(|| report["version"].as_str().map(str::to_string));
                c.installed_at = Some(Utc::now());
                c.self_test = Some(report.clone());
            })
            .await?;
        tracing::info!(binary = %candidate.binary.display(), source = %candidate.source, "projecthub.adopted");
        if was_running {
            self.restart().await?;
        }
        Ok(json!({ "candidate": candidate, "config": cfg }))
    }

    /// Startup check: adopt a Project Hub that is already installed, follow an
    /// app update (new version) and recover when the configured binary is gone.
    pub async fn reconcile(&self) -> Result<(), String> {
        let cfg = self.config().await;
        if cfg.installed && cfg.binary_present() {
            let binary = cfg.binary.clone().unwrap_or_default();
            if let Some(v) = detect::server_version(&binary).await {
                if cfg.version.as_deref() != Some(v.as_str()) {
                    tracing::info!(from = ?cfg.version, to = %v, "projecthub.updated");
                    self.update(|c| c.version = Some(v)).await?;
                }
            }
            return Ok(());
        }
        if !cfg.installed && cfg.ignore_detected {
            return Ok(());
        }
        let detection = self.detection(true).await;
        if detection.best().is_none() {
            if cfg.installed {
                self.set_error(Some("ProjectHub binary is missing — reinstall or install the desktop app".into()));
            }
            return Ok(());
        }
        // Keep `enabled` as it was: adopting never turns the extension on by itself.
        self.use_installed(None).await.map(|_| ())
    }

    pub async fn start(&self) -> Result<(), String> {
        let mut guard = self.running.lock().await;
        if let Some(r) = guard.as_mut() {
            if matches!(r.child.try_wait(), Ok(None)) {
                return Ok(());
            }
        }
        let cfg = self.config().await;
        let binary = cfg
            .binary
            .clone()
            .filter(|b| b.is_file())
            .ok_or("ProjectHub binary is missing — reinstall the extension")?;
        let running = match spawn(&binary, &cfg).await {
            Ok(r) => r,
            Err(err) => {
                self.set_error(Some(err.clone()));
                return Err(err);
            }
        };
        tracing::info!(url = %running.url, pid = running.pid, "projecthub.started");
        *guard = Some(running);
        self.set_error(None);
        Ok(())
    }

    pub async fn stop(&self) {
        if let Some(mut r) = self.running.lock().await.take() {
            let _ = r.child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(5), r.child.wait()).await;
            tracing::info!("projecthub.stopped");
        }
    }

    pub async fn restart(&self) -> Result<(), String> {
        self.stop().await;
        self.start().await
    }

    async fn ensure_client(&self) -> Result<PhClient, String> {
        {
            let mut guard = self.running.lock().await;
            if let Some(r) = guard.as_mut() {
                if matches!(r.child.try_wait(), Ok(None)) {
                    return Ok(r.client.clone());
                }
            }
        }
        let cfg = self.config().await;
        if !cfg.installed {
            return Err("ProjectHub is not installed — open ProjectHub in BoardDo and install it".into());
        }
        if !cfg.enabled {
            return Err("ProjectHub is installed but disabled — enable it in the ProjectHub panel".into());
        }
        self.start().await?;
        let guard = self.running.lock().await;
        guard.as_ref().map(|r| r.client.clone()).ok_or_else(|| "ProjectHub did not start".into())
    }

    pub async fn is_running(&self) -> bool {
        let mut guard = self.running.lock().await;
        guard.as_mut().is_some_and(|r| matches!(r.child.try_wait(), Ok(None)))
    }

    /// Restart a crashed sidecar while the extension is enabled.
    fn spawn_watchdog(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut backoff = 10u64;
            loop {
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                let Some(hub) = weak.upgrade() else { return };
                let cfg = hub.config().await;
                if !(cfg.enabled && cfg.installed) || hub.install_job().running {
                    backoff = 10;
                    continue;
                }
                if hub.is_running().await {
                    backoff = 10;
                    continue;
                }
                tracing::warn!("projecthub.not_running — restarting");
                backoff = match hub.start().await {
                    Ok(()) => 10,
                    Err(_) => (backoff * 2).min(300),
                };
            }
        });
    }
}

async fn spawn(binary: &std::path::Path, cfg: &ExtensionConfig) -> Result<Running, String> {
    let token = random_token();
    let mut cmd = Command::new(binary);
    cmd.arg("--listen")
        .arg("127.0.0.1:0")
        .arg("--exit-on-stdin-eof")
        .arg("--commands")
        .arg(&cfg.commands)
        // Secrets via env, not argv (visible in `ps`).
        .env("PROJECT_HUB_TOKEN", &token)
        .env("RUST_LOG", std::env::var("PROJECTHUB_LOG").unwrap_or_else(|_| "info".into()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    if let Some(db) = cfg.db_path.as_deref().filter(|p| !p.trim().is_empty()) {
        cmd.arg("--db").arg(db);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", binary.display()))?;
    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut lines = BufReader::new(stdout).lines();

    let ready = tokio::time::timeout(STARTUP_TIMEOUT, async {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(rest) = line.strip_prefix("PROJECT_HUB_READY ") {
                return serde_json::from_str::<Value>(rest).ok();
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    let Some(ready) = ready else {
        let _ = child.start_kill();
        return Err("project-hub-server did not report readiness".into());
    };
    let url = ready["url"].as_str().unwrap_or_default().to_string();
    // Keep draining stdout so the pipe never fills.
    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

    let client = PhClient::new(&url, Some(token));
    client.health().await?;
    Ok(Running {
        child,
        _stdin: stdin,
        client,
        url,
        pid,
        started_at: Utc::now(),
    })
}

fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
