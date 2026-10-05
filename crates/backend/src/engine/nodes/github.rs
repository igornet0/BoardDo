//! GitHub action nodes.

use async_trait::async_trait;
use boarddo_shared::{Node, type_ids};
use serde_json::Value;

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};
use crate::integrations::github::{GitHubClient, resolve_owner_repo};

/// Fetch the latest GitHub release for a repository.
///
/// Config:
/// ```json
/// {
///   "connection_id": "<uuid>",
///   "owner": "optional-override",
///   "repo": "optional-override",
///   "track_new": true
/// }
/// ```
///
/// When `track_new` is true (default), persists the last seen tag and sets
/// `is_new` / `baseline` so a schedule loop only continues on real updates.
pub struct GitHubGetLatestRelease;

#[async_trait]
impl NodeHandler for GitHubGetLatestRelease {
    fn type_id(&self) -> &'static str {
        type_ids::GITHUB_GET_LATEST_RELEASE
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let connection_id = node
            .config
            .get("connection_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "github.get_latest_release: `connection_id` is required".to_string())?;

        let resolved = ctx.connections.resolve(connection_id).await?;
        if resolved.connection_type != "github" {
            return Err(format!(
                "github.get_latest_release: connection `{}` must be type `github` (got `{}`)",
                resolved.name, resolved.connection_type
            ));
        }

        let (owner, repo) = resolve_owner_repo(&node.config, &resolved.config)?;
        let client = GitHubClient::from_resolved(&resolved.config, &resolved.credentials)?;
        let release = client.get_latest_release(&owner, &repo).await?;

        let track_new = node
            .config
            .get("track_new")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let (is_new, baseline, previous_tag) = if track_new {
            let state_key = format!(
                "github:latest_release:{}:{}:{}/{}",
                ctx.workflow_id, node.id, owner, repo
            );
            let previous = ctx.node_state.get(&state_key).await?;
            match previous {
                None => {
                    ctx.node_state.set(&state_key, &release.tag_name).await?;
                    (false, true, None)
                }
                Some(prev) if prev == release.tag_name => (false, false, Some(prev)),
                Some(prev) => {
                    ctx.node_state.set(&state_key, &release.tag_name).await?;
                    (true, false, Some(prev))
                }
            }
        } else {
            (true, false, None)
        };

        Ok(NodeOutput::data(release.to_output(
            &owner,
            &repo,
            is_new,
            baseline,
            previous_tag.as_deref(),
        )))
    }
}
