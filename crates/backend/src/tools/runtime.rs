//! Which Python interpreter runs tools.
//!
//! Order: the user's choice (stored setting) → `BOARDDO_PYTHON` → the newest
//! working `python3.x` on this machine (a BoardDo tools venv counts too).
//! Broken shims (e.g. pyenv without that version) are skipped: every
//! candidate is probed by actually running it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;

use crate::storage::Storage;

/// Third-party packages tools may use (pip name → import name).
pub const PACKAGES: &[(&str, &str)] = &[
    ("numpy", "numpy"),
    ("pandas", "pandas"),
    ("requests", "requests"),
    ("beautifulsoup4", "bs4"),
];
const SETTING_KEY: &str = "tools:python";
/// Standard modules a healthy interpreter must import (builds with a broken
/// libexpat / OpenSSL / sqlite exist — e.g. pyexpat failing breaks xml and pip).
const HEALTH_MODULES: &[&str] = &["pyexpat", "ssl", "sqlite3", "decimal", "zlib", "hashlib", "json", "csv"];
const MIN_VERSION: (u32, u32) = (3, 8);
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const PIP_TIMEOUT: Duration = Duration::from_secs(900);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Interpreter {
    /// What BoardDo executes.
    pub path: PathBuf,
    /// `sys.executable` (the real binary behind shims / symlinks).
    pub executable: PathBuf,
    pub version: String,
    #[serde(skip)]
    pub version_tuple: (u32, u32, u32),
    /// import name → installed.
    pub packages: BTreeMap<String, bool>,
    /// `sys.prefix`: the environment (a venv shares the binary with its base).
    pub prefix: PathBuf,
    /// Inside the BoardDo tools venv.
    pub venv: bool,
    /// Standard modules that fail to import (`module: error`); empty = healthy.
    pub broken: Vec<String>,
}

impl Interpreter {
    pub fn healthy(&self) -> bool {
        self.broken.is_empty()
    }

    pub fn label(&self) -> String {
        let installed: Vec<&str> = self.packages.iter().filter(|(_, ok)| **ok).map(|(k, _)| k.as_str()).collect();
        if installed.is_empty() {
            format!("Python {} (standard library only)", self.version)
        } else {
            format!("Python {} with {}", self.version, installed.join(", "))
        }
    }
}

static CURRENT: RwLock<Option<Interpreter>> = RwLock::new(None);
static VENV_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);
/// The user picked an interpreter (otherwise it is chosen automatically).
static MANUAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The interpreter in use (auto-detected on first call when nothing was chosen).
pub fn current() -> Option<Interpreter> {
    if let Some(i) = CURRENT.read().ok().and_then(|c| c.clone()) {
        return Some(i);
    }
    let detected = auto();
    if let Ok(mut c) = CURRENT.write() {
        if c.is_none() {
            *c = detected.clone();
        }
        return c.clone();
    }
    detected
}

fn set_current(i: Option<Interpreter>) {
    if let Ok(mut c) = CURRENT.write() {
        *c = i;
    }
}

/// Where `create_venv` puts the tools environment.
pub fn venv_dir() -> Option<PathBuf> {
    VENV_DIR.read().ok().and_then(|d| d.clone())
}

fn venv_python(dir: &Path) -> PathBuf {
    if cfg!(windows) { dir.join("Scripts").join("python.exe") } else { dir.join("bin").join("python") }
}

/// Load the stored choice; fall back to auto-detection.
pub async fn init(storage: &Storage, data_dir: &Path) {
    if let Ok(mut d) = VENV_DIR.write() {
        *d = Some(data_dir.join("tools-venv"));
    }
    let stored = storage
        .get_node_state(SETTING_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| v["path"].as_str().map(PathBuf::from));
    let chosen = match stored {
        Some(path) => match tokio::task::spawn_blocking(move || probe(&path)).await.ok().flatten() {
            Some(i) => Some(i),
            None => {
                tracing::warn!("tools.python: the chosen interpreter no longer works — auto-detecting");
                None
            }
        },
        None => None,
    };
    MANUAL.store(chosen.is_some(), std::sync::atomic::Ordering::Relaxed);
    let chosen = match chosen {
        Some(i) => Some(i),
        None => tokio::task::spawn_blocking(auto).await.ok().flatten(),
    };
    if let Some(i) = &chosen {
        tracing::info!(python = %i.path.display(), version = %i.version, "tools.python");
    } else {
        tracing::warn!("tools.python: no Python 3 found — custom tools are unavailable");
    }
    set_current(chosen);
}

/// `BOARDDO_PYTHON` if it works, else the newest candidate.
pub fn auto() -> Option<Interpreter> {
    if let Some(p) = std::env::var_os("BOARDDO_PYTHON").map(PathBuf::from) {
        if let Some(i) = probe(&p) {
            return Some(i);
        }
        tracing::warn!(path = %p.display(), "BOARDDO_PYTHON does not run — auto-detecting");
    }
    pick_newest(candidates())
}

pub fn pick_newest(mut list: Vec<Interpreter>) -> Option<Interpreter> {
    // Newest version; on a tie prefer the one with more packages, then the BoardDo venv.
    // Healthy first; then the newest version.
    list.sort_by_key(|i| {
        (
            !i.healthy(),
            std::cmp::Reverse(i.version_tuple),
            std::cmp::Reverse(i.packages.values().filter(|ok| **ok).count()),
            !i.venv,
        )
    });
    list.into_iter().next()
}

/// Every working interpreter on this machine (deduplicated by real binary).
pub fn candidates() -> Vec<Interpreter> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("BOARDDO_PYTHON") {
        paths.push(PathBuf::from(p));
    }
    if let Some(dir) = venv_dir() {
        paths.push(venv_python(&dir));
    }
    for minor in (MIN_VERSION.1..=20).rev() {
        paths.push(PathBuf::from(format!("python3.{minor}")));
    }
    paths.push(PathBuf::from("python3"));
    for fixed in ["/opt/homebrew/bin/python3", "/usr/local/bin/python3", "/usr/bin/python3"] {
        paths.push(PathBuf::from(fixed));
    }

    let mut found: Vec<Interpreter> = Vec::new();
    for p in paths {
        if p.is_absolute() && !p.exists() {
            continue;
        }
        if !p.is_absolute() && which(&p).is_none() {
            continue;
        }
        let Some(i) = probe(&p) else { continue };
        if !found.iter().any(|f| f.executable == i.executable && f.prefix == i.prefix) {
            found.push(i);
        }
    }
    found
}

fn which(name: &Path) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// Run the interpreter: version, real executable and installed packages.
pub fn probe(path: &Path) -> Option<Interpreter> {
    let imports: Vec<&str> = PACKAGES.iter().map(|(_, import)| *import).collect();
    let script = format!(
        "import sys, json, importlib, importlib.util as u\n\
         broken = []\n\
         for m in {health:?}:\n    \
             try:\n        importlib.import_module(m)\n    \
             except Exception as e:\n        broken.append('%s: %s' % (m, str(e).splitlines()[0][:160]))\n\
         print(json.dumps({{'v': list(sys.version_info[:3]), 'exe': sys.executable, 'prefix': sys.prefix, 'broken': broken, \
         'pkgs': {{m: u.find_spec(m) is not None for m in {imports:?}}}}}))",
        health = HEALTH_MODULES,
    );
    let mut child = std::process::Command::new(path)
        .args(["-c", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().ok()? {
            break s;
        }
        if started.elapsed() > PROBE_TIMEOUT {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        return None;
    }
    let mut out = String::new();
    std::io::Read::read_to_string(child.stdout.as_mut()?, &mut out).ok()?;
    let v: Value = serde_json::from_str(out.lines().last()?.trim()).ok()?;
    let ver: Vec<u32> = v["v"].as_array()?.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect();
    let tuple = (*ver.first()?, *ver.get(1)?, ver.get(2).copied().unwrap_or(0));
    if (tuple.0, tuple.1) < MIN_VERSION {
        return None;
    }
    let packages = v["pkgs"]
        .as_object()?
        .iter()
        .map(|(k, ok)| (k.clone(), ok.as_bool().unwrap_or(false)))
        .collect();
    // Real file behind symlinks (python3 → python3.8 → Python.framework/…) for deduplication.
    let executable = PathBuf::from(v["exe"].as_str().unwrap_or_default());
    let executable = std::fs::canonicalize(&executable).unwrap_or(executable);
    let prefix = PathBuf::from(v["prefix"].as_str().unwrap_or_default());
    let prefix = std::fs::canonicalize(&prefix).unwrap_or(prefix);
    let venv = venv_dir().is_some_and(|d| {
        let d = std::fs::canonicalize(&d).unwrap_or(d);
        prefix.starts_with(&d) || path.starts_with(&d)
    });
    let broken = v["broken"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Some(Interpreter {
        path: path.to_path_buf(),
        executable,
        version: format!("{}.{}.{}", tuple.0, tuple.1, tuple.2),
        version_tuple: tuple,
        packages,
        prefix,
        venv,
        broken,
    })
}

/// Choose an interpreter (`None` = automatic) and remember it.
pub async fn select(storage: &Storage, path: Option<PathBuf>) -> Result<Interpreter, String> {
    let automatic = path.is_none();
    let chosen = match path {
        Some(p) => {
            let shown = p.display().to_string();
            tokio::task::spawn_blocking(move || probe(&p))
                .await
                .ok()
                .flatten()
                .ok_or_else(|| format!("`{shown}` is not a working Python ≥ {}.{}", MIN_VERSION.0, MIN_VERSION.1))?
        }
        None => tokio::task::spawn_blocking(auto).await.ok().flatten().ok_or("no Python 3 found")?,
    };
    // An empty setting means "detect again on every start".
    let raw = if automatic { String::new() } else { serde_json::json!({ "path": chosen.path }).to_string() };
    storage
        .set_node_state(SETTING_KEY, &raw, Utc::now())
        .await
        .map_err(|e| e.to_string())?;
    MANUAL.store(!automatic, std::sync::atomic::Ordering::Relaxed);
    set_current(Some(chosen.clone()));
    Ok(chosen)
}

/// Create (or update) the BoardDo tools venv from `base` and install the
/// allowlisted packages; the venv becomes the tools interpreter.
pub async fn create_venv(storage: &Storage, base: Option<PathBuf>, packages: Vec<String>) -> Result<Value, String> {
    let dir = venv_dir().ok_or("tools runtime is not initialised")?;
    let base = match base {
        Some(b) => b,
        None => current().map(|i| i.executable).ok_or("no Python 3 found")?,
    };
    let pip_names: Vec<String> = packages
        .iter()
        .map(|p| {
            let p = p.trim().to_lowercase();
            PACKAGES
                .iter()
                .find(|(pip, import)| *pip == p || *import == p)
                .map(|(pip, _)| pip.to_string())
                .ok_or_else(|| format!("`{p}` is not an allowed package ({})", PACKAGES.iter().map(|(p, _)| *p).collect::<Vec<_>>().join(", ")))
        })
        .collect::<Result<_, _>>()?;

    let probe_base = base.clone();
    let base_info = tokio::task::spawn_blocking(move || probe(&probe_base))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| format!("`{}` is not a working Python", base.display()))?;
    if !base_info.healthy() {
        return Err(format!(
            "Python {} at {} is broken ({}) — reinstall it or pick another interpreter",
            base_info.version,
            base.display(),
            base_info.broken.join("; ")
        ));
    }

    let mut log = Vec::new();
    let python = venv_python(&dir);
    // A venv without pip (an earlier failed attempt) is rebuilt.
    let has_pip = python.exists()
        && run(&python, &["-m".into(), "pip".into(), "--version".into()], Duration::from_secs(60)).await.is_ok();
    if !has_pip {
        let _ = tokio::fs::remove_dir_all(&dir).await;
        match run(&base, &["-m".into(), "venv".into(), dir.display().to_string()], Duration::from_secs(180)).await {
            Ok(out) => log.push(out),
            Err(e) => {
                let _ = tokio::fs::remove_dir_all(&dir).await;
                return Err(e);
            }
        }
    }
    if !pip_names.is_empty() {
        let mut args = vec!["-m".into(), "pip".into(), "install".into(), "--disable-pip-version-check".into(), "--upgrade".into()];
        args.extend(pip_names.iter().cloned());
        log.push(run(&python, &args, PIP_TIMEOUT).await?);
    }
    let interpreter = select(storage, Some(python)).await?;
    let tail: String = log.join("\n").lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    Ok(serde_json::json!({ "interpreter": interpreter, "venv": dir, "installed": pip_names, "log": tail }))
}

async fn run(program: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    let fut = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| format!("`{} {}` timed out", program.display(), args.join(" ")))?
        .map_err(|e| format!("cannot run {}: {e}", program.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(text)
    } else {
        let tail: String = text.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        Err(format!("`{} {}` failed:\n{tail}", program.display(), args.join(" ")))
    }
}

/// What the API / UI shows.
pub fn status() -> Value {
    let list = candidates();
    let auto_pick = pick_newest(list.clone());
    serde_json::json!({
        "current": current(),
        "mode": if MANUAL.load(std::sync::atomic::Ordering::Relaxed) { "manual" } else { "auto" },
        "automatic": auto_pick,
        "candidates": list,
        "venv": venv_dir(),
        "packages": PACKAGES.iter().map(|(pip, import)| serde_json::json!({ "pip": pip, "import": import })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interp(v: (u32, u32, u32), pkgs: usize, venv: bool) -> Interpreter {
        Interpreter {
            path: PathBuf::from(format!("python{}.{}", v.0, v.1)),
            executable: PathBuf::from(format!("/x/{}.{}.{}/{venv}", v.0, v.1, v.2)),
            version: format!("{}.{}.{}", v.0, v.1, v.2),
            version_tuple: v,
            packages: PACKAGES
                .iter()
                .enumerate()
                .map(|(i, (_, m))| (m.to_string(), i < pkgs))
                .collect(),
            prefix: PathBuf::from(format!("/env/{venv}")),
            venv,
            broken: vec![],
        }
    }

    #[test]
    fn newest_wins_then_packages_then_venv() {
        let picked = pick_newest(vec![interp((3, 8, 0), 4, false), interp((3, 14, 7), 0, false), interp((3, 13, 1), 3, false)]).unwrap();
        assert_eq!(picked.version, "3.14.7");
        let picked = pick_newest(vec![interp((3, 14, 7), 0, false), interp((3, 14, 7), 2, true)]).unwrap();
        assert!(picked.venv);
        assert!(pick_newest(vec![]).is_none());
        // A broken newest build loses to a healthy older one.
        let mut broken = interp((3, 14, 7), 0, false);
        broken.broken = vec!["pyexpat: Symbol not found".into()];
        assert_eq!(pick_newest(vec![broken, interp((3, 13, 1), 3, false)]).unwrap().version, "3.13.1");
        assert_eq!(interp((3, 14, 7), 0, false).label(), "Python 3.14.7 (standard library only)");
        assert_eq!(interp((3, 13, 1), 2, false).label(), "Python 3.13.1 with numpy, pandas");
    }

    #[test]
    fn probe_rejects_broken_interpreters() {
        assert!(probe(Path::new("/definitely/not/python")).is_none());
        assert!(probe(Path::new("false")).is_none());
        if let Some(i) = probe(Path::new("python3")) {
            assert!(i.version_tuple >= (3, 8, 0));
            assert!(i.executable.is_absolute());
            assert_eq!(i.packages.len(), PACKAGES.len());
        }
    }
}
