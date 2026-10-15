//! Request-level checks. Source code is checked by the sandbox harness (AST +
//! audit hook), which knows the tool's permissions.

/// `snake_case`, starts with a letter, 2–64 chars: safe in URLs, logs and prompts.
pub fn validate_name(name: &str) -> Result<(), String> {
    let ok = (2..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid tool name `{name}`: use 2–64 characters a-z, 0-9 and _, starting with a letter"
        ))
    }
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
