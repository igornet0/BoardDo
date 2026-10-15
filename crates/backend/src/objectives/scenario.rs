//! Scenarios that execute objective items.
//!
//! Generated graphs read everything item-specific from the trigger payload
//! (`{{trigger.brief}}`, `{{trigger.task}}`, `{{trigger.context}}`), so they
//! stay valid when the objective is edited and can be reused or customised
//! on the canvas. User-picked scenarios get the same payload.

use boarddo_shared::v2::{Objective, ObjectiveItem, ObjectiveItemKind, ObjectiveItemStatus};
use boarddo_shared::{CreateWorkflowRequest, Edge, Node, NodeExecution, Position, type_ids};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Node id whose output is the item result (generated scenarios; preferred in custom ones).
pub const RESULT_NODE: &str = "result";
const CONTEXT_LIMIT: usize = 40_000;
/// Per-item share of the context / evaluation input (chars).
pub const ITEM_RESULT_LIMIT: usize = 12_000;
const RESULT_LIMIT: usize = 20_000;

pub fn is_russian(locale: &str) -> bool {
    locale.starts_with("ru")
}

/// Guess the language of the objective from its text (Cyrillic → ru).
pub fn objective_locale(o: &Objective) -> &'static str {
    let sample = format!("{} {}", o.title, o.description);
    if sample.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')) {
        "ru"
    } else {
        "en"
    }
}

pub fn scenario_name(o: &Objective, item: &ObjectiveItem) -> String {
    let path = item_path(o, &item.id);
    format!(
        "🎯 {} · {} {}",
        truncate(&o.title, 40),
        path,
        truncate(&item.title, 60)
    )
}

/// `2.1`-style position of an item in the tree.
pub fn item_path(o: &Objective, id: &str) -> String {
    let mut parts = Vec::new();
    let mut cur = o.item(id);
    let mut guard = 0;
    while let Some(item) = cur {
        let siblings: Vec<&ObjectiveItem> = o
            .items
            .iter()
            .filter(|i| i.parent_id == item.parent_id)
            .collect();
        let pos = siblings.iter().position(|i| i.id == item.id).unwrap_or(0) + 1;
        parts.push(pos.to_string());
        guard += 1;
        if guard > o.items.len() {
            break;
        }
        cur = item.parent_id.as_deref().and_then(|p| o.item(p));
    }
    parts.reverse();
    parts.join(".")
}

/// Graph for an `ai` / `research` item.
pub fn build_definition(
    o: &Objective,
    item: &ObjectiveItem,
    connection_id: &str,
    model: &str,
) -> CreateWorkflowRequest {
    let ru = is_russian(objective_locale(o));
    let research = item.kind == ObjectiveItemKind::Research;

    let mut nodes = vec![node(type_ids::TRIGGER_MANUAL, "trigger", 80.0, json!({}))];
    let mut edges = Vec::new();
    let mut prev = "trigger";

    if research {
        nodes.push(node(
            type_ids::WEB_SEARCH,
            "search",
            340.0,
            json!({
                "query": "{{trigger.query}}",
                "limit": 6,
                "read_pages": 3,
                "freshness": "auto",
            }),
        ));
        edges.push(edge(prev, "search"));
        prev = "search";
    }

    let mut prompt = if ru {
        "{{trigger.brief}}\n\n## Текущий пункт\n{{trigger.task}}\n\n## Результаты предыдущих пунктов\n{{trigger.context}}"
            .to_string()
    } else {
        "{{trigger.brief}}\n\n## Current item\n{{trigger.task}}\n\n## Results of previous items\n{{trigger.context}}"
            .to_string()
    };
    if research {
        prompt.push_str(if ru {
            "\n\n## Найденные источники (с датами публикации)\n{{nodes.search.output.context}}"
        } else {
            "\n\n## Web sources (with publication dates)\n{{nodes.search.output.context}}"
        });
    }

    let system = if ru {
        "Ты исполнитель пунктов цели в BoardDo. Выполни текущий пункт полностью и верни готовый \
         результат в Markdown — сам результат, а не план его получения. Учитывай характеристики \
         и критерии успеха цели. Если данных не хватает, сделай разумные допущения и явно перечисли \
         их в конце. Не выдумывай факты: опирайся на источники, если они даны, и указывай ссылки."
    } else {
        "You execute objective items in BoardDo. Complete the current item and return the finished \
         result in Markdown — the deliverable itself, not a plan for producing it. Respect the \
         objective's characteristics and success criteria. If data is missing, make reasonable \
         assumptions and list them at the end. Do not invent facts: rely on the sources when given \
         and cite links."
    };

    let x = if research { 600.0 } else { 340.0 };
    nodes.push(node(
        type_ids::AI_CHAT,
        RESULT_NODE,
        x,
        json!({
            "connection_id": connection_id,
            "model": model,
            "system": system,
            "prompt": prompt,
            "temperature": 0.3,
            // Reasoning models (Claude adaptive thinking, DeepSeek R) spend
            // from the same budget before writing the answer.
            "max_tokens": 16_000,
            "timeout_ms": 600_000,
        }),
    ));
    edges.push(edge(prev, RESULT_NODE));

    CreateWorkflowRequest {
        name: scenario_name(o, item),
        description: format!(
            "{}\n\nobjective:{}/{}",
            item.title, o.id, item.id
        ),
        nodes,
        edges,
    }
}

/// Stable fingerprint of a graph (nodes + edges only).
pub fn definition_hash(nodes: &[Node], edges: &[Edge]) -> String {
    let body = json!({ "nodes": nodes, "edges": edges });
    let mut hasher = Sha256::new();
    hasher.update(body.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

fn node(type_id: &str, id: &str, x: f64, config: Value) -> Node {
    Node {
        id: id.into(),
        type_id: type_id.into(),
        category: Some(type_ids::category_for(type_id)),
        position: Position { x, y: 160.0 },
        config,
    }
}

fn edge(source: &str, target: &str) -> Edge {
    Edge {
        id: format!("e_{source}_{target}"),
        source: source.into(),
        target: target.into(),
        source_port: None,
        target_port: None,
    }
}

/// Text block describing the objective for prompts.
pub fn objective_brief(o: &Objective) -> String {
    let ru = is_russian(objective_locale(o));
    let mut out = format!("# {} {}\n", if ru { "Цель:" } else { "Objective:" }, safe(&o.title));
    if !o.description.trim().is_empty() {
        out.push_str(&format!("{}\n", safe(&o.description)));
    }
    if !o.characteristics.is_empty() {
        out.push_str(if ru { "\n## Характеристики\n" } else { "\n## Characteristics\n" });
        for c in &o.characteristics {
            let unit = c.unit.as_deref().map(|u| format!(" {u}")).unwrap_or_default();
            out.push_str(&format!("- {}: {}{}\n", safe(&c.key), safe(&c.value), safe(&unit)));
        }
    }
    if !o.success_criteria.is_empty() {
        out.push_str(if ru { "\n## Критерии успеха\n" } else { "\n## Success criteria\n" });
        for c in &o.success_criteria {
            out.push_str(&format!("- {}\n", safe(c)));
        }
    }
    if let Some(deadline) = o.deadline {
        out.push_str(&format!(
            "\n{} {}\n",
            if ru { "Срок:" } else { "Deadline:" },
            deadline.format("%Y-%m-%d")
        ));
    }
    out
}

pub fn item_task(o: &Objective, item: &ObjectiveItem) -> String {
    let mut out = format!("{} {}", item_path(o, &item.id), safe(&item.title));
    if let Some(parent) = item.parent_id.as_deref().and_then(|p| o.item(p)) {
        out.push_str(&format!(" ({} {})", "⊂", safe(&parent.title)));
    }
    out.push('\n');
    if !item.description.trim().is_empty() {
        out.push_str(&format!("{}\n", safe(&item.description)));
    }
    if let Some(p) = item.prompt.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("\n{}\n", safe(p)));
    }
    out
}

/// Results of explicit dependencies, else of every earlier settled item.
pub fn item_context(o: &Objective, item: &ObjectiveItem) -> String {
    let ordered = o.ordered_ids();
    let mut ids: Vec<String> = item
        .depends_on
        .iter()
        .flat_map(|d| leaves_under(o, d))
        .collect();
    if ids.is_empty() {
        ids = ordered
            .iter()
            .take_while(|id| *id != &item.id)
            .filter(|id| o.is_leaf(id))
            .cloned()
            .collect();
    }
    let mut out = String::new();
    for id in ids {
        let Some(dep) = o.item(&id) else { continue };
        if dep.status != ObjectiveItemStatus::Done {
            continue;
        }
        let Some(result) = dep.result.as_deref().filter(|r| !r.trim().is_empty()) else {
            continue;
        };
        out.push_str(&format!(
            "### {} {}\n{}\n\n",
            item_path(o, &dep.id),
            safe(&dep.title),
            safe(&truncate(result, ITEM_RESULT_LIMIT))
        ));
    }
    if out.is_empty() {
        return "—".into();
    }
    truncate_tail(&out, CONTEXT_LIMIT)
}

fn leaves_under(o: &Objective, id: &str) -> Vec<String> {
    if o.is_leaf(id) {
        return vec![id.to_string()];
    }
    o.children(id)
        .map(|c| c.id.clone())
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|c| leaves_under(o, &c))
        .collect()
}

/// Payload every item scenario receives as `trigger`. `project` is the
/// ProjectHub `.brdd` digest of the goal's project, when linked.
pub fn trigger_payload(o: &Objective, item: &ObjectiveItem, project: Option<&str>) -> Value {
    let task = item_task(o, item);
    let query = item
        .query
        .clone()
        .filter(|q| !q.trim().is_empty())
        .unwrap_or_else(|| item.title.clone());
    json!({
        "objective_id": o.id,
        "item_id": item.id,
        "objective": {
            "title": o.title,
            "description": o.description,
            "characteristics": o.characteristics,
            "success_criteria": o.success_criteria,
        },
        "item": {
            "id": item.id,
            "path": item_path(o, &item.id),
            "title": item.title,
            "description": item.description,
            "prompt": item.prompt,
            "query": query,
        },
        "brief": match project {
            Some(p) => format!(
                "{}\n## {}\n{}\n",
                objective_brief(o),
                if is_russian(objective_locale(o)) { "Проект (ProjectHub · .brdd)" } else { "Project (ProjectHub · .brdd)" },
                safe(p)
            ),
            None => objective_brief(o),
        },
        "project": project.map(safe),
        "task": task,
        "query": safe(&query),
        "context": item_context(o, item),
        "text": task,
    })
}

/// Result text of a finished execution: `result` node, else the last node with output.
pub fn extract_result(nodes: &[NodeExecution]) -> Option<String> {
    let pick = nodes
        .iter()
        .find(|n| n.node_id == RESULT_NODE && n.output.is_some())
        .or_else(|| {
            nodes
                .iter()
                .filter(|n| n.output.is_some() && !n.node_id.starts_with("trigger"))
                .max_by_key(|n| n.finished_at.unwrap_or(n.started_at))
        })?;
    let output = pick.output.as_ref()?;
    let output = output.get("output").unwrap_or(output);
    let text = ["text", "message", "content", "result", "summary", "context"]
        .iter()
        .find_map(|k| output.get(*k).and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string_pretty(output).unwrap_or_default());
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(truncate(&text, RESULT_LIMIT))
    }
}

/// Literal text inside node configs must not open `{{…}}` expressions.
pub fn safe(s: &str) -> String {
    s.replace("{{", "{ {").replace("}}", "} }")
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn truncate_tail(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(count - max).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use boarddo_shared::v2::{ObjectiveCharacteristic, ObjectiveItem};

    fn objective() -> Objective {
        let mut o = Objective::new("Подготовить пост");
        o.characteristics.push(ObjectiveCharacteristic {
            key: "Объём".into(),
            value: "300".into(),
            unit: Some("слов".into()),
        });
        let mut a = ObjectiveItem::new("i1", "Найти факты");
        a.kind = ObjectiveItemKind::Research;
        a.status = ObjectiveItemStatus::Done;
        a.result = Some("Факт {{x}}".into());
        let b = ObjectiveItem::new("i2", "Написать текст");
        o.items = vec![a, b];
        o.normalize();
        o
    }

    #[test]
    fn research_graph_has_search_then_result() {
        let o = objective();
        let def = build_definition(&o, o.item("i1").unwrap(), "conn", "claude-opus-5-5");
        let ids: Vec<&str> = def.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["trigger", "search", "result"]);
        assert_eq!(def.edges.len(), 2);
        assert!(def.nodes[2].config["prompt"].as_str().unwrap().contains("nodes.search.output.context"));
        assert!(def.name.starts_with("🎯 Подготовить пост · 1 "));
    }

    #[test]
    fn context_uses_previous_results_and_escapes_templates() {
        let o = objective();
        let payload = trigger_payload(&o, o.item("i2").unwrap(), None);
        let ctx = payload["context"].as_str().unwrap();
        assert!(ctx.contains("Факт { {x} }"));
        assert!(payload["brief"].as_str().unwrap().contains("Объём: 300 слов"));
        assert_eq!(payload["item"]["path"], "2");
    }

    #[test]
    fn hash_is_stable() {
        let o = objective();
        let a = build_definition(&o, o.item("i2").unwrap(), "c", "m");
        let b = build_definition(&o, o.item("i2").unwrap(), "c", "m");
        assert_eq!(definition_hash(&a.nodes, &a.edges), definition_hash(&b.nodes, &b.edges));
    }
}
