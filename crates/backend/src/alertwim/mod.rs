//! AlertWim extension: a local log window ([AlertWim](https://github.com/igornet0/AlertWim))
//! that scenarios write to through `alertwim.alert` nodes.
//!
//! AlertWim listens on loopback and announces itself in
//! `~/.alertwim/endpoint.json` (`{url, token, pid, version, exe}`). BoardDo reads
//! that file, keeps an `alertwim` connection in sync with it automatically and
//! starts the app when a scenario needs the window. Not on the machine →
//! the extension offers to install it from GitHub (`cargo build --release`).
//!
//! Environment: `ALERTWIM_HOME` (discovery dir, default `~/.alertwim`),
//! `ALERTWIM_BIN` (binary or `.app` to launch), `ALERTWIM_REPO`, `ALERTWIM_BRANCH`,
//! `ALERTWIM_DIR` (install root, default `~/.cache/boarddo/extensions/alertwim`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use boarddo_shared::{CreateConnectionRequest, UpdateConnectionRequest};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

use crate::state::SharedState;

pub const EXTENSION_ID: &str = "alertwim";
pub const CONNECTION_TYPE: &str = "alertwim";
pub const DEFAULT_REPO: &str = "https://github.com/igornet0/AlertWim.git";
pub const DEFAULT_BRANCH: &str = "main";
const CONFIG_KEY: &str = "extension:alertwim";
const BIN_NAME: &str = "alertwim";
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);
const BUILD_TIMEOUT: Duration = Duration::from_secs(60 * 30);
const LOG_LINES: usize = 300;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Binary or `.app` bundle BoardDo launches (detected or built).
    #[serde(default)]
    pub binary: Option<PathBuf>,
    /// `build` | `app` | `env` | `path` | `running`.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub connection_id: Option<Uuid>,
    #[serde(default)]
    pub installed_at: Option<DateTime<Utc>>,
}

/// What AlertWim writes to `endpoint.json` while it runs.
#[derive(Debug, Clone, Deserialize)]
pub struct Endpoint {
    pub url: String,
    pub token: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub exe: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InstallJob {
    pub running: bool,
    pub phase: String,
    pub error: Option<String>,
    pub log: VecDeque<String>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub struct AlertWim {
    state: SharedState,
    job: Mutex<InstallJob>,
    /// Serialises launches so parallel scenario runs start one window.
    launch_lock: tokio::sync::Mutex<()>,
}

static EXT: OnceLock<Arc<AlertWim>> = OnceLock::new();

pub fn get() -> Option<Arc<AlertWim>> {
    EXT.get().cloned()
}

/// Called once at startup: registers the extension and adopts a running or
/// installed AlertWim (creating its connection).
pub async fn init(state: SharedState) -> Arc<AlertWim> {
    let ext = EXT
        .get_or_init(|| {
            Arc::new(AlertWim {
                state,
                job: Mutex::new(InstallJob::default()),
                launch_lock: tokio::sync::Mutex::new(()),
            })
        })
        .clone();
    let bg = ext.clone();
    tokio::spawn(async move {
        if let Err(err) = bg.sync().await {
            tracing::debug!(error = %err, "alertwim.sync");
        }
    });
    ext
}

fn repo() -> String {
    std::env::var("ALERTWIM_REPO").unwrap_or_else(|_| DEFAULT_REPO.into())
}
fn branch() -> String {
    std::env::var("ALERTWIM_BRANCH").unwrap_or_else(|_| DEFAULT_BRANCH.into())
}

pub fn home_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("ALERTWIM_HOME") {
        return PathBuf::from(p);
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".alertwim"))
        .unwrap_or_else(|| PathBuf::from(".alertwim"))
}

pub fn install_root() -> PathBuf {
    if let Some(p) = std::env::var_os("ALERTWIM_DIR") {
        return PathBuf::from(p);
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".cache/boarddo/extensions/alertwim"))
        .unwrap_or_else(|| PathBuf::from(".boarddo/extensions/alertwim"))
}

pub fn read_endpoint() -> Option<Endpoint> {
    let raw = std::fs::read(home_dir().join("endpoint.json")).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(concat!("BoardDo/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

async fn alive(url: &str) -> bool {
    match http().get(format!("{}/health", url.trim_end_matches('/'))).send().await {
        Ok(r) => r.json::<Value>().await.is_ok_and(|v| v["app"] == "alertwim"),
        Err(_) => false,
    }
}

/// The running instance, if its discovery file points at a live server.
pub async fn running_endpoint() -> Option<Endpoint> {
    let ep = read_endpoint()?;
    alive(&ep.url).await.then_some(ep)
}

fn is_launchable(p: &Path) -> bool {
    if p.extension().is_some_and(|e| e == "app") {
        p.is_dir()
    } else {
        p.is_file()
    }
}

/// Installed copies, best first: `ALERTWIM_BIN`, macOS app bundles, BoardDo's
/// own build, `PATH`, the binary the last running instance came from.
pub fn detect_binaries() -> Vec<(PathBuf, &'static str)> {
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    let mut push = |p: PathBuf, src: &'static str| {
        if is_launchable(&p) && !out.iter().any(|(q, _)| q == &p) {
            out.push((p, src));
        }
    };
    if let Some(p) = std::env::var_os("ALERTWIM_BIN") {
        push(PathBuf::from(p), "env");
    }
    push(PathBuf::from("/Applications/AlertWim.app"), "app");
    if let Some(h) = std::env::var_os("HOME") {
        push(PathBuf::from(h).join("Applications/AlertWim.app"), "app");
    }
    push(install_root().join("target/release").join(BIN_NAME), "build");
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            push(dir.join(BIN_NAME), "path");
        }
    }
    if let Some(exe) = read_endpoint().and_then(|e| e.exe) {
        push(exe, "running");
    }
    out
}

impl AlertWim {
    pub async fn config(&self) -> Config {
        match self.state.storage.get_node_state(CONFIG_KEY).await {
            Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
            _ => Config::default(),
        }
    }

    async fn save(&self, cfg: &Config) -> Result<(), String> {
        let raw = serde_json::to_string(cfg).map_err(|e| e.to_string())?;
        self.state
            .storage
            .set_node_state(CONFIG_KEY, &raw, Utc::now())
            .await
            .map_err(|e| e.to_string())
    }

    fn with_job<R>(&self, f: impl FnOnce(&mut InstallJob) -> R) -> R {
        f(&mut self.job.lock().unwrap())
    }

    fn log(&self, line: impl Into<String>) {
        self.with_job(|j| {
            j.log.push_back(line.into());
            while j.log.len() > LOG_LINES {
                j.log.pop_front();
            }
        });
    }

    /// Binary to launch: the configured one if still present, else the best detected.
    pub async fn binary(&self) -> Option<PathBuf> {
        let cfg = self.config().await;
        if let Some(b) = cfg.binary.filter(|b| is_launchable(b)) {
            return Some(b);
        }
        detect_binaries().into_iter().next().map(|(p, _)| p)
    }

    /// Adopt what is on the machine and keep the connection in sync with the
    /// running instance. Cheap; called at startup, on status and before sends.
    pub async fn sync(&self) -> Result<Option<Uuid>, String> {
        let mut cfg = self.config().await;
        if cfg.binary.as_ref().is_none_or(|b| !is_launchable(b)) {
            if let Some((bin, src)) = detect_binaries().into_iter().next() {
                cfg.binary = Some(bin);
                cfg.source = Some(src.into());
                cfg.installed_at.get_or_insert_with(Utc::now);
                self.save(&cfg).await?;
            }
        }
        let Some(ep) = read_endpoint() else {
            return Ok(cfg.connection_id);
        };
        let id = self.ensure_connection(&mut cfg, &ep).await?;
        Ok(Some(id))
    }

    /// Create (or refresh) the `AlertWim` connection: base_url + token secret.
    async fn ensure_connection(&self, cfg: &mut Config, ep: &Endpoint) -> Result<Uuid, String> {
        let conns = &self.state.connections;
        let existing = match cfg.connection_id {
            Some(id) => conns.get(id).await.map_err(|e| e.to_string())?,
            None => None,
        };
        let existing = match existing {
            Some(c) => Some(c),
            None => conns
                .list()
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|c| c.connection_type == CONNECTION_TYPE),
        };
        let config = json!({ "base_url": ep.url, "managed": true, "version": ep.version });
        let secret = json!({ "token": ep.token });
        let id = match existing {
            Some(c) if c.config.get("base_url") == config.get("base_url") && c.has_secret => {
                // Token is stable per machine; refresh only when the URL moved.
                c.id
            }
            Some(c) => {
                let req = UpdateConnectionRequest {
                    name: c.name.clone(),
                    connection_type: CONNECTION_TYPE.into(),
                    config,
                    secret: Some(secret),
                    enabled: c.enabled,
                };
                conns.update(c.id, req).await.map_err(|e| e.to_string())?;
                c.id
            }
            None => {
                let req = CreateConnectionRequest {
                    name: "AlertWim".into(),
                    connection_type: CONNECTION_TYPE.into(),
                    config,
                    secret: Some(secret),
                    enabled: true,
                };
                let c = conns.create(req).await.map_err(|e| e.to_string())?;
                tracing::info!(id = %c.id, "alertwim.connection_created");
                c.id
            }
        };
        if cfg.connection_id != Some(id) {
            cfg.connection_id = Some(id);
            self.save(cfg).await?;
        }
        Ok(id)
    }

    /// Start AlertWim unless it already runs; waits until it answers.
    pub async fn launch(&self) -> Result<Endpoint, String> {
        if let Some(ep) = running_endpoint().await {
            return Ok(ep);
        }
        let _guard = self.launch_lock.lock().await;
        if let Some(ep) = running_endpoint().await {
            return Ok(ep);
        }
        let bin = self.binary().await.ok_or(
            "AlertWim is not installed — install it in Extensions (github.com/igornet0/AlertWim)",
        )?;
        let mut cmd = if bin.extension().is_some_and(|e| e == "app") {
            let mut c = Command::new("open");
            c.arg("-g").arg("-a").arg(&bin);
            c
        } else {
            Command::new(&bin)
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // Own process group: Ctrl+C on `make dev` must not close the window.
        #[cfg(unix)]
        cmd.process_group(0);
        cmd.spawn().map_err(|e| format!("cannot start {}: {e}", bin.display()))?;

        let deadline = tokio::time::Instant::now() + LAUNCH_TIMEOUT;
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Some(ep) = running_endpoint().await {
                drop(_guard);
                let _ = self.sync().await;
                return Ok(ep);
            }
        }
        Err(format!("AlertWim ({}) did not answer within {}s", bin.display(), LAUNCH_TIMEOUT.as_secs()))
    }

    pub async fn status(&self) -> Value {
        let _ = self.sync().await;
        let cfg = self.config().await;
        let running = running_endpoint().await;
        let detected: Vec<Value> = detect_binaries()
            .into_iter()
            .map(|(p, s)| json!({ "path": p, "source": s }))
            .collect();
        let job = self.with_job(|j| j.clone());
        json!({
            "id": EXTENSION_ID,
            "name": "AlertWim",
            "description": "Local log window: scenarios send alerts to it with the AlertWim node",
            "repo": repo(),
            "installed": cfg.binary.as_ref().is_some_and(|b| is_launchable(b)) || running.is_some(),
            "enabled": true,
            "running": running.is_some(),
            "url": running.as_ref().map(|e| e.url.clone()),
            "version": running.as_ref().and_then(|e| e.version.clone()),
            "binary": cfg.binary,
            "source": cfg.source,
            "connection_id": cfg.connection_id,
            "installed_at": cfg.installed_at,
            "detected": detected,
            "install_job": job,
        })
    }

    /// Clone/pull the repository and `cargo build --release` in the background.
    pub fn start_install(self: &Arc<Self>) -> Result<(), String> {
        {
            let mut j = self.job.lock().unwrap();
            if j.running {
                return Err("AlertWim install is already running".into());
            }
            *j = InstallJob { running: true, phase: "prepare".into(), ..InstallJob::default() };
        }
        let me = self.clone();
        tokio::spawn(async move {
            let result = me.install().await;
            me.with_job(|j| {
                j.running = false;
                j.finished_at = Some(Utc::now());
                match &result {
                    Ok(()) => j.phase = "done".into(),
                    Err(e) => {
                        j.phase = "failed".into();
                        j.error = Some(e.clone());
                    }
                }
            });
            if let Err(e) = result {
                me.log(format!("✗ {e}"));
            }
        });
        Ok(())
    }

    async fn install(self: &Arc<Self>) -> Result<(), String> {
        let root = install_root();
        let src = root.join("src");
        tokio::fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
        let (repo, branch) = (repo(), branch());

        self.with_job(|j| j.phase = "fetch".into());
        let fetch = if src.join(".git").is_dir() {
            self.log(format!("git pull {repo} ({branch})"));
            let mut c = Command::new("git");
            c.args(["pull", "--ff-only", "origin", &branch]).current_dir(&src);
            c
        } else {
            self.log(format!("git clone {repo} ({branch})"));
            let mut c = Command::new("git");
            c.args(["clone", "--depth", "1", "--branch", &branch, &repo]).arg(&src);
            c
        };
        self.run_logged(fetch, Duration::from_secs(300)).await.map_err(|e| format!("fetch failed: {e}"))?;
        if !src.join("Cargo.toml").is_file() {
            return Err(format!("{repo} ({branch}) has no Cargo.toml — nothing to build yet"));
        }

        self.with_job(|j| j.phase = "build".into());
        self.log("cargo build --release (first build takes a few minutes)…");
        let mut build = Command::new("cargo");
        build
            .args(["build", "--release"])
            .current_dir(&src)
            .env("CARGO_TARGET_DIR", root.join("target"))
            .env("CARGO_TERM_COLOR", "never");
        self.run_logged(build, BUILD_TIMEOUT).await.map_err(|e| format!("build failed: {e}"))?;

        let bin = root.join("target/release").join(BIN_NAME);
        if !bin.is_file() {
            return Err(format!("build finished but {} is missing", bin.display()));
        }
        let mut cfg = self.config().await;
        cfg.binary = Some(bin.clone());
        cfg.source = Some("build".into());
        cfg.installed_at = Some(Utc::now());
        self.save(&cfg).await?;
        self.log(format!("✓ installed {}", bin.display()));

        self.with_job(|j| j.phase = "launch".into());
        let ep = self.launch().await?;
        self.log(format!("✓ AlertWim is running at {}", ep.url));
        Ok(())
    }

    async fn run_logged(&self, mut cmd: Command, timeout: Duration) -> Result<(), String> {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let out = child.stdout.take();
        let err = child.stderr.take();
        let pump = |r: Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>| async move {
            let Some(r) = r else { return };
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(me) = get() {
                    me.log(line);
                }
            }
        };
        let a = tokio::spawn(pump(out.map(|o| Box::new(o) as _)));
        let b = tokio::spawn(pump(err.map(|e| Box::new(e) as _)));
        let status = tokio::time::timeout(timeout, child.wait())
            .await
            .map_err(|_| format!("timed out after {}s", timeout.as_secs()))?
            .map_err(|e| e.to_string())?;
        let _ = tokio::join!(a, b);
        if status.success() { Ok(()) } else { Err(format!("exit status {status}")) }
    }
}

/// Alert payload accepted by AlertWim's `POST /v1/alerts`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AlertMessage {
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    pub show: bool,
    pub focus: bool,
}

/// Deliver one alert to `url` with `token`.
pub async fn send(url: &str, token: &str, alert: &AlertMessage) -> Result<Value, String> {
    let resp = http()
        .post(format!("{}/v1/alerts", url.trim_end_matches('/')))
        .bearer_auth(token)
        .header("X-AlertWim-Client", "boarddo")
        .json(alert)
        .send()
        .await
        .map_err(|e| format!("AlertWim unreachable at {url}: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        Ok(body)
    } else {
        Err(format!("AlertWim answered {status}: {}", body["error"].as_str().unwrap_or("")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};

    async fn mock() -> String {
        async fn alerts(headers: HeaderMap, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
            let auth = headers.get("authorization").and_then(|v| v.to_str().ok());
            let client = headers.get("x-alertwim-client").and_then(|v| v.to_str().ok());
            if auth != Some("Bearer secret") {
                return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" })));
            }
            (StatusCode::OK, Json(json!({ "ok": true, "id": "a1", "echo": body, "client": client })))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/v1/alerts", post(alerts))).await.unwrap();
        });
        url
    }

    #[tokio::test]
    async fn send_delivers_alert_with_token() {
        let url = mock().await;
        let alert = AlertMessage {
            level: "warn".into(),
            title: Some("t".into()),
            message: "hello".into(),
            show: true,
            ..Default::default()
        };
        let resp = send(&url, "secret", &alert).await.unwrap();
        assert_eq!(resp["id"], "a1");
        assert_eq!(resp["client"], "boarddo");
        assert_eq!(resp["echo"]["level"], "warn");
        assert_eq!(resp["echo"]["message"], "hello");
        assert!(resp["echo"].get("source").is_none());
    }

    #[tokio::test]
    async fn send_reports_rejected_token() {
        let url = mock().await;
        let err = send(&url, "wrong", &AlertMessage { message: "x".into(), ..Default::default() })
            .await
            .unwrap_err();
        assert!(err.contains("401"), "{err}");
    }
}
