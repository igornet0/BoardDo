//! GitHub REST API client (releases).

use serde_json::{Value, json};
use std::time::Duration;

const DEFAULT_API_BASE: &str = "https://api.github.com";
const DEFAULT_TIMEOUT_MS: u64 = 15_000;

#[derive(Debug, Clone)]
pub struct GitHubClient {
    pub token: Option<String>,
    pub api_base: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct LatestRelease {
    pub tag_name: String,
    pub name: String,
    pub html_url: String,
    pub body: String,
    pub published_at: Option<String>,
    pub prerelease: bool,
    pub draft: bool,
    pub raw: Value,
}

impl GitHubClient {
    pub fn from_resolved(config: &Value, credentials: &Value) -> Result<Self, String> {
        let api_base = config
            .get("api_base")
            .or_else(|| config.get("base_url"))
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_API_BASE)
            .trim()
            .trim_end_matches('/')
            .to_string();
        if api_base.is_empty() {
            return Err("github: empty api_base".into());
        }

        let token = credentials
            .get("token")
            .or_else(|| credentials.get("access_token"))
            .or_else(|| credentials.get("api_key"))
            .and_then(Value::as_str)
            .or_else(|| {
                credentials
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let timeout_ms = config
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .max(1_000);

        Ok(Self {
            token,
            api_base,
            timeout_ms,
        })
    }

    pub async fn get_authenticated_user(&self) -> Result<Value, String> {
        self.get_json("/user").await
    }

    pub async fn get_repo(&self, owner: &str, repo: &str) -> Result<Value, String> {
        let path = format!(
            "/repos/{}/{}",
            encode_segment(owner),
            encode_segment(repo)
        );
        self.get_json(&path).await
    }

    pub async fn get_latest_release(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<LatestRelease, String> {
        let path = format!(
            "/repos/{}/{}/releases/latest",
            encode_segment(owner),
            encode_segment(repo)
        );
        let raw = self.get_json(&path).await?;
        parse_release(&raw)
    }

    async fn get_json(&self, path: &str) -> Result<Value, String> {
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!(
                "{}{}",
                self.api_base,
                if path.starts_with('/') {
                    path.to_string()
                } else {
                    format!("/{path}")
                }
            )
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(self.timeout_ms))
            .build()
            .map_err(|e| format!("github: http client: {e}"))?;

        let mut req = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "BoardDo-SmartDo")
            .header("X-GitHub-Api-Version", "2022-11-28");

        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }

        let response = req
            .send()
            .await
            .map_err(|e| format!("github: request failed: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("github: read body: {e}"))?;

        if !status.is_success() {
            let snippet: String = body.chars().take(400).collect();
            return Err(format!("github: HTTP {status}: {snippet}"));
        }

        serde_json::from_str(&body).map_err(|e| format!("github: invalid JSON: {e}"))
    }
}

pub fn parse_release(raw: &Value) -> Result<LatestRelease, String> {
    let tag_name = raw
        .get("tag_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "github: release missing tag_name".to_string())?
        .to_string();
    let name = raw
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(tag_name.as_str())
        .to_string();
    let html_url = raw
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let body = raw
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let published_at = raw
        .get("published_at")
        .and_then(Value::as_str)
        .map(str::to_string);
    let prerelease = raw
        .get("prerelease")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let draft = raw.get("draft").and_then(Value::as_bool).unwrap_or(false);

    Ok(LatestRelease {
        tag_name,
        name,
        html_url,
        body,
        published_at,
        prerelease,
        draft,
        raw: raw.clone(),
    })
}

impl LatestRelease {
    pub fn to_output(
        &self,
        owner: &str,
        repo: &str,
        is_new: bool,
        baseline: bool,
        previous_tag: Option<&str>,
    ) -> Value {
        json!({
            "owner": owner,
            "repo": repo,
            "tag_name": self.tag_name,
            "name": self.name,
            "html_url": self.html_url,
            "body": self.body,
            "published_at": self.published_at,
            "prerelease": self.prerelease,
            "draft": self.draft,
            "is_new": is_new,
            "baseline": baseline,
            "previous_tag": previous_tag,
            "raw": self.raw,
        })
    }
}

fn encode_segment(raw: &str) -> String {
    // owner/repo path segments — reject separators to avoid path injection.
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() || trimmed.contains('/') {
        return String::new();
    }
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

pub fn resolve_owner_repo(
    node_config: &Value,
    connection_config: &Value,
) -> Result<(String, String), String> {
    let owner = node_config
        .get("owner")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            connection_config
                .get("owner")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| "github: `owner` is required on node or connection config".to_string())?
        .to_string();

    let repo = node_config
        .get("repo")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            connection_config
                .get("repo")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| "github: `repo` is required on node or connection config".to_string())?
        .to_string();

    if owner.contains('/') || repo.contains('/') {
        return Err("github: owner/repo must not contain '/'".into());
    }
    Ok((owner, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_release_extracts_fields() {
        let raw = json!({
            "tag_name": "v1.0.0",
            "name": "First",
            "html_url": "https://github.com/o/r/releases/tag/v1.0.0",
            "body": "notes",
            "published_at": "2026-01-01T00:00:00Z",
            "prerelease": false,
            "draft": false
        });
        let rel = parse_release(&raw).unwrap();
        assert_eq!(rel.tag_name, "v1.0.0");
        assert_eq!(rel.name, "First");
        assert!(!rel.prerelease);
    }

    #[test]
    fn resolve_owner_repo_prefers_node() {
        let (o, r) = resolve_owner_repo(
            &json!({"owner": "acme", "repo": "app"}),
            &json!({"owner": "other", "repo": "x"}),
        )
        .unwrap();
        assert_eq!((o, r), ("acme".into(), "app".into()));
    }

    #[test]
    fn from_resolved_reads_token() {
        let client = GitHubClient::from_resolved(
            &json!({"api_base": "https://api.github.com"}),
            &json!({"token": "ghp_test"}),
        )
        .unwrap();
        assert_eq!(client.token.as_deref(), Some("ghp_test"));
    }
}
