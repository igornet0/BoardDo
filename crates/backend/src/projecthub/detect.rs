//! Finds a Project Hub that is already on this machine: the desktop app
//! (which ships `project-hub-server` next to its executable), a binary on
//! `PATH` / in `~/.cargo/bin`, `PROJECTHUB_BIN`, or an earlier BoardDo build.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

use super::install_root;

pub const BIN_NAME: &str = if cfg!(windows) { "project-hub-server.exe" } else { "project-hub-server" };
const APP_NAME: &str = "Project Hub";
const APP_ID: &str = "com.projecthub.desktop";
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// A runnable `project-hub-server`.
#[derive(Debug, Clone, Serialize)]
pub struct ServerCandidate {
    /// `app` | `env` | `path` | `managed`
    pub source: String,
    pub binary: PathBuf,
    /// `None` for builds older than `--version`.
    pub version: Option<String>,
    /// The desktop app bundle the binary belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesktopApp {
    pub path: PathBuf,
    pub version: Option<String>,
    /// Builds before the HTTP API do not ship the server.
    pub has_server: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Database {
    pub path: PathBuf,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub projects: Option<i64>,
    pub tasks: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    pub servers: Vec<ServerCandidate>,
    pub apps: Vec<DesktopApp>,
    pub database: Database,
}

impl Detection {
    /// Preferred server: the desktop app's, then explicit env, PATH, BoardDo's own build.
    pub fn best(&self) -> Option<&ServerCandidate> {
        ["app", "env", "path", "managed"]
            .iter()
            .find_map(|src| self.servers.iter().find(|c| c.source == *src))
    }

    pub fn find(&self, binary: &Path) -> Option<&ServerCandidate> {
        let want = canonical(binary);
        self.servers.iter().find(|c| canonical(&c.binary) == want)
    }
}

/// Where to look.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    pub app_dirs: Vec<PathBuf>,
    pub bin_dirs: Vec<PathBuf>,
    pub env_bin: Option<PathBuf>,
    /// BoardDo's own build output.
    pub managed: Option<PathBuf>,
    pub default_db: PathBuf,
}

impl Roots {
    pub fn system() -> Self {
        Self {
            app_dirs: app_dirs(),
            bin_dirs: bin_dirs(),
            env_bin: std::env::var_os("PROJECTHUB_BIN").map(PathBuf::from),
            managed: Some(install_root().join("target/release").join(BIN_NAME)),
            default_db: default_db_path(),
        }
    }
}

#[cfg(test)]
static TEST_ROOTS: std::sync::RwLock<Option<Roots>> = std::sync::RwLock::new(None);

/// Tests never look at the real machine: empty roots unless set here.
#[cfg(test)]
pub fn set_test_roots(roots: Option<Roots>) {
    *TEST_ROOTS.write().unwrap() = roots;
}

fn roots() -> Roots {
    #[cfg(test)]
    return TEST_ROOTS.read().unwrap().clone().unwrap_or_else(|| Roots {
        default_db: PathBuf::from("/nonexistent/project-hub.db"),
        ..Roots::default()
    });
    #[cfg(not(test))]
    Roots::system()
}

/// Look everywhere; `db_override` is the extension's database setting.
pub async fn detect(db_override: Option<&str>) -> Detection {
    detect_in(&roots(), db_override).await
}

pub async fn detect_in(roots: &Roots, db_override: Option<&str>) -> Detection {
    let mut found: Vec<(String, PathBuf, Option<PathBuf>)> = Vec::new();
    let mut apps = Vec::new();

    for dir in &roots.app_dirs {
        let Some((app, server)) = app_bundle(dir) else { continue };
        let has_server = server.as_ref().is_some_and(|s| s.is_file());
        apps.push(DesktopApp { version: app_version(&app), path: app.clone(), has_server });
        if let Some(server) = server.filter(|s| s.is_file()) {
            found.push(("app".into(), server, Some(app)));
        }
    }
    if let Some(bin) = roots.env_bin.clone().filter(|b| b.is_file()) {
        found.push(("env".into(), bin, None));
    }
    for dir in &roots.bin_dirs {
        let bin = dir.join(BIN_NAME);
        if bin.is_file() {
            found.push(("path".into(), bin, None));
        }
    }
    if let Some(managed) = roots.managed.clone().filter(|b| b.is_file()) {
        found.push(("managed".into(), managed, None));
    }

    let mut servers: Vec<ServerCandidate> = Vec::new();
    for (source, binary, app) in found {
        if servers.iter().any(|c| canonical(&c.binary) == canonical(&binary)) {
            continue;
        }
        servers.push(ServerCandidate { version: server_version(&binary).await, source, binary, app });
    }

    let db = db_override
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| roots.default_db.clone());
    Detection { servers, apps, database: database(db).await }
}

/// `dir` is either an `.app` bundle / install folder itself or a folder holding one.
fn app_bundle(dir: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    if cfg!(target_os = "macos") {
        let app = if dir.extension().is_some_and(|e| e == "app") {
            dir.to_path_buf()
        } else {
            dir.join(format!("{APP_NAME}.app"))
        };
        if !app.join("Contents/Info.plist").is_file() {
            return None;
        }
        Some((app.clone(), Some(app.join("Contents/MacOS").join(BIN_NAME))))
    } else {
        // Windows / Linux installs: the server sits next to the app executable.
        let exe = if cfg!(windows) { "project-hub.exe" } else { "project-hub" };
        dir.join(exe).is_file().then(|| (dir.to_path_buf(), Some(dir.join(BIN_NAME))))
    }
}

fn app_dirs() -> Vec<PathBuf> {
    if let Some(list) = std::env::var_os("PROJECTHUB_APP_DIRS") {
        return std::env::split_paths(&list).collect();
    }
    let home = home();
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/Applications"));
        if let Some(h) = &home {
            dirs.push(h.join("Applications"));
        }
    } else if cfg!(windows) {
        for var in ["LOCALAPPDATA", "ProgramFiles"] {
            if let Some(base) = std::env::var_os(var) {
                dirs.push(PathBuf::from(base).join(APP_NAME));
            }
        }
    } else {
        dirs.extend(["/usr/bin", "/usr/local/bin", "/opt/project-hub"].map(PathBuf::from));
    }
    dirs
}

fn bin_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    // GUI-launched processes often miss these on PATH.
    if let Some(h) = home() {
        dirs.push(h.join(".cargo/bin"));
        dirs.push(h.join(".local/bin"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// `project-hub-server --version` → `0.1.1`.
pub async fn server_version(binary: &Path) -> Option<String> {
    let out = tokio::time::timeout(
        VERSION_TIMEOUT,
        Command::new(binary)
            .arg("--version")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .nth(1)
        .map(str::to_string)
}

fn app_version(app: &Path) -> Option<String> {
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let at = plist.find("<key>CFBundleShortVersionString</key>")?;
    let rest = &plist[at..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")?;
    Some(rest[start..start + end].trim().to_string())
}

/// The desktop app's database (same rule as `project-hub-server`).
pub fn default_db_path() -> PathBuf {
    let base = if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join(APP_ID).join("project-hub.db")
}

async fn database(path: PathBuf) -> Database {
    let meta = std::fs::metadata(&path).ok();
    let (projects, tasks) = if meta.is_some() { counts(&path).await } else { (None, None) };
    Database {
        exists: meta.is_some(),
        size_bytes: meta.map(|m| m.len()),
        projects,
        tasks,
        path,
    }
}

/// Read-only peek so the UI can say "651 projects" before anything is enabled.
async fn counts(path: &Path) -> (Option<i64>, Option<i64>) {
    use sqlx::ConnectOptions;
    let Ok(opts) = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .disable_statement_logging()
        .connect()
        .await
    else {
        return (None, None);
    };
    let mut conn = opts;
    let count = |table: &'static str| format!("SELECT COUNT(*) FROM {table}");
    let projects = sqlx::query_scalar::<_, i64>(&count("projects")).fetch_one(&mut conn).await.ok();
    let tasks = sqlx::query_scalar::<_, i64>(&count("tasks")).fetch_one(&mut conn).await.ok();
    let _ = sqlx::Connection::close(conn).await;
    (projects, tasks)
}
