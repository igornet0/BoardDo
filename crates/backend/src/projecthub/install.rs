//! Install job: fetch source → build `project-hub-server` → `--self-test`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::{ProjectHub, install_root};

const BUILD_TIMEOUT: Duration = Duration::from_secs(60 * 45);
const SELF_TEST_TIMEOUT: Duration = Duration::from_secs(180);
use super::detect::BIN_NAME;

pub(super) async fn run(hub: &Arc<ProjectHub>, repo: Option<String>, branch: Option<String>) -> Result<(), String> {
    let cfg = hub.config().await;
    let repo = repo.filter(|r| !r.trim().is_empty()).unwrap_or(cfg.repo.clone()).trim().to_string();
    let branch = branch.filter(|b| !b.trim().is_empty()).unwrap_or(cfg.branch.clone()).trim().to_string();
    let log = |line: String| {
        hub.with_job(|j| j.line(line));
    };
    let phase = |p: &str| {
        hub.with_job(|j| j.phase = p.to_string());
    };

    // A prebuilt binary skips fetch + build.
    let prebuilt = std::env::var_os("PROJECTHUB_BIN").map(PathBuf::from).filter(|p| p.is_file());
    let (source_dir, binary) = if let Some(bin) = prebuilt {
        log(format!("Using prebuilt binary from PROJECTHUB_BIN: {}", bin.display()));
        (None, bin)
    } else {
        phase("prepare");
        let cargo = tool_version("cargo", &["--version"]).await
            .ok_or("`cargo` was not found — install Rust (https://rustup.rs) to build ProjectHub")?;
        log(format!("cargo: {cargo}"));

        phase("fetch");
        let source = fetch(hub, &repo, &branch).await?;
        let manifest = tokio::fs::read_to_string(source.join("Cargo.toml")).await.unwrap_or_default();
        if !manifest.contains("apps/server") {
            return Err(format!(
                "{repo} ({branch}) has no headless server (apps/server). Use a ProjectHub revision with the HTTP API \
                 or point the installer at a local checkout"
            ));
        }

        phase("build");
        let target = install_root().join("target");
        log(format!("Building project-hub-server (release) → {}", target.display()));
        log("First build compiles libgit2/OpenSSL/SQLite and takes a few minutes…".into());
        let mut cmd = Command::new("cargo");
        cmd.args(["build", "--release", "-p", "project-hub-server"])
            .current_dir(&source)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_TERM_COLOR", "never");
        run_logged(hub, cmd, BUILD_TIMEOUT).await.map_err(|e| format!("build failed: {e}"))?;
        let bin = target.join("release").join(BIN_NAME);
        if !bin.is_file() {
            return Err(format!("build finished but {} is missing", bin.display()));
        }
        (Some(source), bin)
    };

    phase("self_test");
    log("Running project-hub-server --self-test on a temporary database…".into());
    let report = self_test(&binary).await?;
    for step in report["steps"].as_array().cloned().unwrap_or_default() {
        let ok = step["ok"].as_bool().unwrap_or(false);
        log(format!(
            "  {} {}{}",
            if ok { "✓" } else { "✕" },
            step["name"].as_str().unwrap_or("?"),
            step["detail"].as_str().map(|d| format!(" — {d}")).unwrap_or_default()
        ));
    }
    let ok = report["ok"].as_bool().unwrap_or(false);
    let version = report["version"].as_str().map(str::to_string);
    hub.update(|c| {
        c.repo = repo.clone();
        c.branch = branch.clone();
        c.source = Some(if source_dir.is_some() { "build" } else { "env" }.into());
        c.ignore_detected = false;
        c.source_dir = source_dir.clone();
        c.binary = Some(binary.clone());
        c.version = version.clone();
        c.installed = true;
        c.installed_at = Some(Utc::now());
        c.self_test = Some(report.clone());
    })
    .await?;
    if !ok {
        return Err(format!(
            "self-test failed: {} of {} steps",
            report["failed"],
            report["steps"].as_array().map(|a| a.len()).unwrap_or(0)
        ));
    }
    log(format!(
        "Self-test passed: {} steps in {} ms",
        report["passed"], report["duration_ms"]
    ));
    // A reinstall picks up the new binary.
    if hub.is_running().await {
        hub.restart().await?;
    }
    Ok(())
}

/// Local checkout → used in place; URL → shallow clone / update under the install root.
async fn fetch(hub: &Arc<ProjectHub>, repo: &str, branch: &str) -> Result<PathBuf, String> {
    let log = |line: String| {
        hub.with_job(|j| j.line(line));
    };
    let local = Path::new(repo);
    if local.is_dir() {
        if !local.join("Cargo.toml").is_file() {
            return Err(format!("{repo} is not a Cargo workspace"));
        }
        log(format!("Using local checkout {repo}"));
        return Ok(local.to_path_buf());
    }
    tool_version("git", &["--version"]).await.ok_or("`git` was not found")?;
    let src = install_root().join("src");
    if src.join(".git").is_dir() {
        log(format!("Updating {} ({branch})", src.display()));
        let mut fetch = Command::new("git");
        fetch.args(["fetch", "--depth", "1", "origin", branch]).current_dir(&src);
        run_logged(hub, fetch, Duration::from_secs(600)).await?;
        let mut reset = Command::new("git");
        reset.args(["reset", "--hard", "FETCH_HEAD"]).current_dir(&src);
        run_logged(hub, reset, Duration::from_secs(60)).await?;
    } else {
        tokio::fs::create_dir_all(install_root()).await.map_err(|e| e.to_string())?;
        if src.exists() {
            tokio::fs::remove_dir_all(&src).await.map_err(|e| e.to_string())?;
        }
        log(format!("Cloning {repo} ({branch})"));
        let mut clone = Command::new("git");
        clone
            .args(["clone", "--depth", "1", "--branch", branch, repo])
            .arg(&src);
        run_logged(hub, clone, Duration::from_secs(900)).await?;
    }
    Ok(src)
}

pub(super) async fn self_test(binary: &Path) -> Result<Value, String> {
    let out = tokio::time::timeout(
        SELF_TEST_TIMEOUT,
        Command::new(binary)
            .arg("--self-test")
            .env("RUST_LOG", "warn")
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "self-test timed out".to_string())?
    .map_err(|e| format!("cannot run {}: {e}", binary.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let start = stdout.find('{').ok_or_else(|| {
        format!(
            "self-test printed no report (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).chars().take(400).collect::<String>()
        )
    })?;
    serde_json::from_str(&stdout[start..]).map_err(|e| format!("self-test report is not JSON: {e}"))
}

async fn tool_version(tool: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(tool).args(args).output().await.ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run a command, streaming stdout + stderr lines into the install log.
async fn run_logged(hub: &Arc<ProjectHub>, mut cmd: Command, timeout: Duration) -> Result<(), String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let pump = |stream: Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>| {
        let hub = hub.clone();
        tokio::spawn(async move {
            let Some(stream) = stream else { return };
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim_end().to_string();
                if !line.is_empty() {
                    hub.with_job(|j| j.line(line));
                }
            }
        })
    };
    let a = pump(stdout.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>));
    let b = pump(stderr.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>));
    let status = tokio::time::timeout(timeout, child.wait())
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let _ = tokio::join!(a, b);
    if status.success() {
        Ok(())
    } else {
        Err(format!("exit status {}", status.code().unwrap_or(-1)))
    }
}
