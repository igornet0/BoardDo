use boarddo_shared::{Edge, Node, WorkflowDefinition, type_ids};
use serde_json::Value;

use super::registry::NodeRegistry;

/// Validates a workflow definition before execution.
pub fn validate_workflow(
    definition: &WorkflowDefinition,
    registry: &NodeRegistry,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    if definition.nodes.is_empty() {
        errors.push("workflow has no nodes".into());
        return Err(errors);
    }

    // Unique node IDs
    let mut seen = std::collections::HashSet::new();
    for node in &definition.nodes {
        if node.id.trim().is_empty() {
            errors.push("node with empty id".into());
            continue;
        }
        if !seen.insert(node.id.clone()) {
            errors.push(format!("duplicate node id `{}`", node.id));
        }
    }

    let node_ids: std::collections::HashSet<_> =
        definition.nodes.iter().map(|n| n.id.as_str()).collect();

    // At least one trigger
    let triggers: Vec<_> = definition
        .nodes
        .iter()
        .filter(|n| n.type_id.starts_with("trigger."))
        .collect();
    if triggers.is_empty() {
        errors.push("workflow must contain at least one trigger node".into());
    }

    // Known types + required config
    for node in &definition.nodes {
        if !registry.contains(&node.type_id) {
            errors.push(format!(
                "node `{}`: unknown type `{}`",
                node.id, node.type_id
            ));
            continue;
        }
        errors.extend(validate_node_config(node));
    }

    // Edges reference existing nodes
    let mut edge_ids = std::collections::HashSet::new();
    for edge in &definition.edges {
        if !edge_ids.insert(edge.id.clone()) {
            errors.push(format!("duplicate edge id `{}`", edge.id));
        }
        if !node_ids.contains(edge.source.as_str()) {
            errors.push(format!(
                "edge `{}`: source `{}` does not exist",
                edge.id, edge.source
            ));
        }
        if !node_ids.contains(edge.target.as_str()) {
            errors.push(format!(
                "edge `{}`: target `{}` does not exist",
                edge.id, edge.target
            ));
        }
        if edge.source == edge.target {
            errors.push(format!("edge `{}`: self-loop is not allowed", edge.id));
        }
    }

    // Cycle detection (simple DFS)
    if let Some(cycle_node) = find_cycle(&definition.nodes, &definition.edges) {
        errors.push(format!(
            "workflow graph contains a cycle at node `{cycle_node}`"
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_node_config(node: &Node) -> Vec<String> {
    let mut errors = Vec::new();
    let cfg = &node.config;

    match node.type_id.as_str() {
        type_ids::DATA_SET => {
            if cfg.get("name").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing parameter `name`", node.id));
            }
            if cfg.get("value").is_none() {
                errors.push(format!("node `{}`: missing parameter `value`", node.id));
            }
        }
        type_ids::DATA_TRANSFORM => {
            if cfg.get("mapping").and_then(Value::as_object).is_none() {
                errors.push(format!("node `{}`: missing parameter `mapping`", node.id));
            }
        }
        type_ids::LOGIC_CONDITION => {
            let has_expr = cfg.get("expression").is_some();
            let has_compare = cfg.get("left").is_some()
                && cfg.get("operator").is_some()
                && cfg.get("right").is_some();
            if !has_expr && !has_compare {
                errors.push(format!(
                    "node `{}`: provide `expression` or `left`/`operator`/`right`",
                    node.id
                ));
            }
        }
        type_ids::LOGIC_DELAY => {
            if cfg.get("ms").and_then(Value::as_u64).is_none() {
                errors.push(format!(
                    "node `{}`: missing or invalid parameter `ms`",
                    node.id
                ));
            }
        }
        type_ids::HTTP_REQUEST => {
            if cfg.get("url").is_none() {
                errors.push(format!("node `{}`: missing parameter `url`", node.id));
            }
            if let Some(method) = cfg.get("method").and_then(Value::as_str) {
                let m = method.to_uppercase();
                if !matches!(
                    m.as_str(),
                    "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
                ) {
                    errors.push(format!(
                        "node `{}`: unsupported HTTP method `{method}`",
                        node.id
                    ));
                }
            }
        }
        type_ids::WEB_FETCH => {
            if cfg.get("url").is_none() {
                errors.push(format!("node `{}`: missing parameter `url`", node.id));
            }
        }
        type_ids::WEB_EXTRACT => {
            for key in ["url", "instruction"] {
                if cfg.get(key).is_none() {
                    errors.push(format!("node `{}`: missing parameter `{key}`", node.id));
                }
            }
            if cfg.get("connection_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `connection_id`", node.id));
            }
        }
        type_ids::AI_ANALYZE => {
            if cfg.get("prompt").is_none() && cfg.get("text").is_none() {
                errors.push(format!(
                    "node `{}`: provide `prompt` or `text`",
                    node.id
                ));
            }
        }
        type_ids::TRIGGER_SCHEDULE => {
            if let Err(e) = crate::triggers::schedule::parse_schedule_config(cfg) {
                errors.push(format!("node `{}`: {e}", node.id));
            }
        }
        type_ids::TELEGRAM_SEND_MESSAGE => {
            if cfg.get("connection_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `connection_id`", node.id));
            }
            if cfg.get("chat_id").is_none() {
                errors.push(format!("node `{}`: missing `chat_id`", node.id));
            }
            if cfg.get("text").is_none() {
                errors.push(format!("node `{}`: missing `text`", node.id));
            }
        }
        type_ids::TELEGRAM_SEND_PHOTO => {
            if cfg.get("connection_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `connection_id`", node.id));
            }
            if cfg.get("chat_id").is_none() {
                errors.push(format!("node `{}`: missing `chat_id`", node.id));
            }
            if cfg.get("photo").is_none() {
                errors.push(format!("node `{}`: missing `photo`", node.id));
            }
        }
        type_ids::TELEGRAM_SEND_DOCUMENT => {
            if cfg.get("connection_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `connection_id`", node.id));
            }
            if cfg.get("chat_id").is_none() {
                errors.push(format!("node `{}`: missing `chat_id`", node.id));
            }
            if cfg.get("document").is_none() {
                errors.push(format!("node `{}`: missing `document`", node.id));
            }
        }
        type_ids::TELEGRAM_USER_SEND_MESSAGE => {
            if cfg.get("account_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `account_id`", node.id));
            }
            if cfg.get("chat_id").is_none() {
                errors.push(format!("node `{}`: missing `chat_id`", node.id));
            }
            if cfg.get("text").is_none() {
                errors.push(format!("node `{}`: missing `text`", node.id));
            }
        }
        type_ids::TELEGRAM_USER_FORWARD_MESSAGE => {
            if cfg.get("account_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `account_id`", node.id));
            }
            if cfg.get("from_chat_id").is_none() {
                errors.push(format!("node `{}`: missing `from_chat_id`", node.id));
            }
            if cfg.get("to_chat_id").is_none() {
                errors.push(format!("node `{}`: missing `to_chat_id`", node.id));
            }
            if cfg.get("message_id").is_none() {
                errors.push(format!("node `{}`: missing `message_id`", node.id));
            }
        }
        type_ids::TELEGRAM_USER_EDIT_MESSAGE => {
            if cfg.get("account_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `account_id`", node.id));
            }
            if cfg.get("chat_id").is_none() {
                errors.push(format!("node `{}`: missing `chat_id`", node.id));
            }
            if cfg.get("message_id").is_none() {
                errors.push(format!("node `{}`: missing `message_id`", node.id));
            }
            if cfg.get("text").is_none() {
                errors.push(format!("node `{}`: missing `text`", node.id));
            }
        }
        type_ids::TELEGRAM_USER_DELETE_MESSAGES => {
            if cfg.get("account_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `account_id`", node.id));
            }
            if cfg.get("chat_id").is_none() {
                errors.push(format!("node `{}`: missing `chat_id`", node.id));
            }
            if cfg.get("message_id").is_none() {
                errors.push(format!("node `{}`: missing `message_id`", node.id));
            }
        }
        type_ids::TELEGRAM_USER_GET_CHAT_MEMBERS => {
            if cfg.get("account_id").and_then(Value::as_str).is_none() {
                errors.push(format!("node `{}`: missing `account_id`", node.id));
            }
            if cfg.get("chat_id").is_none() {
                errors.push(format!("node `{}`: missing `chat_id`", node.id));
            }
        }
        type_ids::GITHUB_GET_LATEST_RELEASE => {
            if cfg
                .get("connection_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .is_none()
            {
                errors.push(format!("node `{}`: missing `connection_id`", node.id));
            }
        }
        type_ids::DEBUG_LOG
        | type_ids::TRIGGER_MANUAL
        | type_ids::TRIGGER_WEBHOOK
        | type_ids::TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED => {}
        _ => {}
    }

    errors
}

fn find_cycle(nodes: &[Node], edges: &[Edge]) -> Option<String> {
    use std::collections::{HashMap, HashSet};

    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes {
        adj.entry(node.id.as_str()).or_default();
    }
    for edge in edges {
        adj.entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        visiting: &mut HashSet<&'a str>,
        visited: &mut HashSet<&'a str>,
    ) -> Option<&'a str> {
        if visited.contains(node) {
            return None;
        }
        if !visiting.insert(node) {
            return Some(node);
        }
        if let Some(neighbors) = adj.get(node) {
            for &next in neighbors {
                if let Some(c) = dfs(next, adj, visiting, visited) {
                    return Some(c);
                }
            }
        }
        visiting.remove(node);
        visited.insert(node);
        None
    }

    for node in nodes {
        if let Some(c) = dfs(node.id.as_str(), &adj, &mut visiting, &mut visited) {
            return Some(c.to_string());
        }
    }
    None
}
