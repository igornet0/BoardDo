//! Static checks on Python tool source before sandbox execution.

const BLOCKED_PATTERNS: &[(&str, &str)] = &[
    ("os.system", "os.system"),
    ("subprocess", "subprocess"),
    ("eval(", "eval"),
    ("exec(", "exec"),
    ("__import__", "__import__"),
    ("ctypes", "ctypes"),
    ("socket.socket", "socket"),
    ("open('/", "unsafe filesystem path"),
    ("open(\"/", "unsafe filesystem path"),
    ("environ[", "environment secret access"),
    ("getenv(", "environment access"),
];

pub fn scan_python_source(source: &str, permissions: &boarddo_shared::v2::ToolPermissions) -> Result<(), String> {
    let lower = source.to_lowercase();
    for (pat, label) in BLOCKED_PATTERNS {
        if source.contains(pat) || lower.contains(pat) {
            if *pat == "getenv(" && permissions.filesystem_read {
                continue;
            }
            return Err(format!("blocked construct: {label}"));
        }
    }
    if !permissions.network && (source.contains("import requests") || source.contains("urllib")) {
        return Err("network import without network permission".into());
    }
    if !permissions.shell && source.contains("Popen") {
        return Err("shell execution pattern without shell permission".into());
    }
    Ok(())
}

pub fn validate_requirements(requirements: &[String]) -> Result<(), String> {
    use boarddo_shared::v2::DEFAULT_PY_ALLOWLIST;
    for req in requirements {
        let name = req
            .split(['=', '<', '>', '['])
            .next()
            .unwrap_or(req.as_str())
            .trim()
            .to_lowercase()
            .replace('-', "_");
        let allowed = DEFAULT_PY_ALLOWLIST.iter().any(|a| {
            let a = a.replace('-', "_");
            name == a || name.starts_with(&format!("{a}_"))
        });
        if !allowed {
            return Err(format!("package `{req}` not in allowlist"));
        }
    }
    Ok(())
}
