//! Typed `web.*` action nodes.

use async_trait::async_trait;
use boarddo_shared::v2::{WebPolicy, WebFetchRequest, WebSearchRequest};
use boarddo_shared::{Node, type_ids};
use serde_json::{Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::registry::{NodeHandler, NodeOutput};

pub struct WebSearchNode;

#[async_trait]
impl NodeHandler for WebSearchNode {
    fn type_id(&self) -> &'static str {
        type_ids::WEB_SEARCH
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let query = resolve_query(node, ctx)?;
        if query.trim().is_empty() {
            return Err("web.search: query resolved to empty text".into());
        }
        let limit = node
            .config
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(8)
            .clamp(1, 20) as u32;

        let policy = WebPolicy::default();
        let result = ctx
            .web
            .search(
                WebSearchRequest {
                    query: query.clone(),
                    limit: Some(limit),
                    freshness: None,
                },
                &policy,
            )
            .await
            .map_err(|e| format!("web.search: {e}"))?;

        Ok(NodeOutput::data(json!({
            "query": query,
            "results": result.results,
        })))
    }
}

pub struct WebFetchNode;

#[async_trait]
impl NodeHandler for WebFetchNode {
    fn type_id(&self) -> &'static str {
        type_ids::WEB_FETCH
    }

    async fn execute(&self, node: &Node, ctx: &mut ExecutionContext) -> Result<NodeOutput, String> {
        let url = resolve_url(node, ctx)?;
        if url.trim().is_empty() {
            return Err("web.fetch: url resolved to empty text".into());
        }
        let method = node
            .config
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_string();
        let mut headers = std::collections::BTreeMap::new();
        if let Some(Value::Object(map)) = node.config.get("headers") {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    headers.insert(k.clone(), s.to_string());
                }
            }
        }
        let policy = WebPolicy::default();
        let result = ctx
            .web
            .fetch(
                WebFetchRequest {
                    url: url.clone(),
                    method,
                    headers,
                },
                &policy,
            )
            .await
            .map_err(|e| format!("web.fetch: {e}"))?;

        Ok(NodeOutput::data(json!({
            "url": result.url,
            "status": result.status,
            "content_type": result.content_type,
            "body": result.body,
            "truncated": result.truncated,
        })))
    }
}

fn resolve_url(node: &Node, ctx: &ExecutionContext) -> Result<String, String> {
    if let Some(raw) = node.config.get("url").cloned() {
        return match ctx.resolve_value(raw)? {
            Value::String(s) => Ok(s),
            Value::Null => Ok(String::new()),
            other => Ok(other.to_string().trim_matches('"').to_string()),
        };
    }
    Ok(String::new())
}

fn resolve_query(node: &Node, ctx: &ExecutionContext) -> Result<String, String> {
    if let Some(raw) = node.config.get("query").cloned() {
        return match ctx.resolve_value(raw)? {
            Value::String(s) => Ok(s),
            Value::Null => Ok(String::new()),
            other => Ok(other.to_string().trim_matches('"').to_string()),
        };
    }
    match ctx.resolve_template_result("{{trigger.text}}") {
        Ok(Value::String(s)) => Ok(s),
        Ok(other) => Ok(other.to_string().trim_matches('"').to_string()),
        Err(_) => Ok(String::new()),
    }
}
