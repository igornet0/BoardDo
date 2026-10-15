//! AI assistant for objectives: fill an objective from its title, or split
//! the objective / an item into sub-items.

use boarddo_shared::v2::{
    OBJECTIVE_MAX_DEPTH, Objective, ObjectiveAssistMode, ObjectiveAssistRequest,
    ObjectiveCharacteristic, ObjectiveDraft, ObjectiveItem, ObjectiveItemDraft, ObjectiveItemKind,
    ObjectiveItemStatus,
};
use serde_json::{Value, json};
use std::collections::HashMap;

use super::llm::ObjectiveLlm;
use super::scenario::{is_russian, objective_locale};
use crate::integrations::openai::extract_json_object;

const MAX_ITEMS: usize = 12;

pub async fn draft(
    llm: &ObjectiveLlm,
    objective: &Objective,
    req: &ObjectiveAssistRequest,
    project: Option<&str>,
) -> Result<ObjectiveDraft, String> {
    if let Some(item_id) = req.item_id.as_deref() {
        if objective.item(item_id).is_none() {
            return Err(format!("item `{item_id}` not found"));
        }
        if objective.depth(item_id) + 1 >= OBJECTIVE_MAX_DEPTH {
            return Err(format!(
                "item `{item_id}` is already {OBJECTIVE_MAX_DEPTH} levels deep"
            ));
        }
    }
    // Content follows the language the goal is written in; the UI locale only
    // breaks the tie when the title carries no letters to judge by.
    let has_letters = objective.title.chars().any(char::is_alphabetic);
    let locale = match req.locale.as_deref() {
        Some(ui) if !has_letters => ui.to_string(),
        _ => objective_locale(objective).to_string(),
    };
    let system = system_prompt(&locale, req.mode, req.item_id.is_some(), project.is_some());
    let mut user = user_prompt(objective, req);
    if let Some(p) = project {
        user.push_str(&format!("\n## Linked code project (ProjectHub .brdd)\n{p}\n"));
    }
    let raw = llm.chat(&system, &user, 0.3, 16_000, 300_000).await?;
    let value = extract_json_object(&raw)
        .ok_or_else(|| format!("AI reply is not JSON: {}", raw.chars().take(300).collect::<String>()))?;
    Ok(parse_draft(&value))
}

fn system_prompt(locale: &str, mode: ObjectiveAssistMode, for_item: bool, has_project: bool) -> String {
    let task_kind = if has_project {
        "\n- kind \"task\": a change in the linked code project (feature, fix, refactor, docs, release) done by a developer \
         or a coding agent. It becomes a ProjectHub task tracked until done — imperative title, description with \
         acceptance criteria and the files/modules to touch (use the project facts below)."
    } else {
        ""
    };
    let lang = if is_russian(locale) {
        "Пиши все тексты на русском языке."
    } else {
        "Write all texts in English."
    };
    let task = match (mode, for_item) {
        (ObjectiveAssistMode::Fill, _) => {
            "Fill in the objective: a clear description (2-4 sentences), 3-6 measurable \
             characteristics (key/value/unit: budget, volume, audience, format, quality bar…), \
             2-5 checkable success criteria, and 3-7 top-level items that achieve it. \
             `unit` only for measurable numbers (pcs, days, $, words) — otherwise leave it empty. \
             A deadline goes into `deadline` (YYYY-MM-DD), not into characteristics; use null \
             when the user implied none. Keep facts the user already gave; improve vague parts."
        }
        (ObjectiveAssistMode::Decompose, false) => {
            "Split the objective into 3-7 top-level items (sequential where order matters). \
             Leave description, characteristics and success_criteria empty."
        }
        (ObjectiveAssistMode::Decompose, true) => {
            "Split ONLY the target item into 2-5 concrete sub-items that together complete it. \
             Leave description, characteristics and success_criteria empty."
        }
        (ObjectiveAssistMode::Refine, _) => {
            "The objective was evaluated as NOT achieved. Add 1-3 NEW top-level items that close \
             exactly the gaps named in the evaluation — do not repeat work that is already done, \
             build on its results. Prefer a different approach where the previous one failed \
             (e.g. another search query or source). Leave description, characteristics and \
             success_criteria empty."
        }
    };
    format!(
        r#"You are the BoardDo objective planner. {lang}
{task}

Every item is executed automatically by a BoardDo scenario, so make items concrete and self-contained:
- kind "research": needs fresh facts from the internet (prices, competitors, news, docs); give a short web search `query`.
- kind "ai": writing, analysis, planning, calculations, code, summaries from previous results.
- kind "manual": only for actions in the physical world or in accounts BoardDo cannot reach (pay, sign, call).{task_kind}
- `prompt`: precise instruction for the executor — what to produce and in which format.
- `depends_on`: keys of earlier items whose results this item needs (items see previous results anyway).
- Use `children` only for an item that is clearly too big for one step (max depth 2).
- Small objectives deserve few items: don't pad.

Return ONLY a JSON object, no markdown fences:
{{"message":"one sentence for the user","title":"optional improved title","description":"…","deadline":null,
"characteristics":[{{"key":"…","value":"…","unit":"…"}}],
"success_criteria":["…"],
"items":[{{"key":"a","title":"…","description":"…","kind":"research|ai|manual{task_opt}","prompt":"…","query":"…","depends_on":[],"children":[]}}]}}"#,
        task_opt = if has_project { "|task" } else { "" }
    )
}

fn user_prompt(o: &Objective, req: &ObjectiveAssistRequest) -> String {
    let items: Vec<Value> = o
        .ordered_ids()
        .iter()
        .filter_map(|id| o.item(id))
        .map(|i| {
            json!({
                "id": i.id,
                "parent_id": i.parent_id,
                "title": i.title,
                "description": i.description,
                "kind": i.kind,
                "status": i.status,
            })
        })
        .collect();
    let current = json!({
        "title": o.title,
        "description": o.description,
        "characteristics": o.characteristics,
        "success_criteria": o.success_criteria,
        "deadline": o.deadline,
        "items": items,
    });
    let mut out = format!(
        "Today: {}\nCurrent objective:\n{}\n",
        chrono::Utc::now().format("%Y-%m-%d"),
        serde_json::to_string_pretty(&current).unwrap_or_default()
    );
    if let Some(item) = req.item_id.as_deref().and_then(|id| o.item(id)) {
        out.push_str(&format!(
            "\nTarget item to split: id={} «{}» — {}\n",
            item.id, item.title, item.description
        ));
    }
    if req.mode == ObjectiveAssistMode::Refine {
        out.push_str("\n## Evaluation\n");
        out.push_str(o.summary.as_deref().unwrap_or("(no evaluation yet)"));
        out.push_str("\n\n## Results so far\n");
        for id in o.ordered_ids() {
            let Some(item) = o.item(&id) else { continue };
            if let Some(result) = item.result.as_deref() {
                out.push_str(&format!(
                    "### {} {}\n{}\n\n",
                    item.id,
                    item.title,
                    super::scenario::truncate(result, 2500)
                ));
            }
        }
    }
    if let Some(instr) = req.instruction.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("\nUser wish: {instr}\n"));
    }
    out
}

// ─── Lenient parsing (models return numbers, strings or objects freely) ─────

fn parse_draft(v: &Value) -> ObjectiveDraft {
    ObjectiveDraft {
        message: text(v.get("message")),
        title: opt_text(v.get("title")),
        description: opt_text(v.get("description")),
        characteristics: v
            .get("characteristics")
            .map(parse_characteristics)
            .unwrap_or_default(),
        success_criteria: list_of_text(v.get("success_criteria")),
        deadline: opt_text(v.get("deadline")).and_then(|d| parse_deadline(&d)),
        items: v
            .get("items")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|i| parse_item(i, 0)).take(MAX_ITEMS).collect())
            .unwrap_or_default(),
    }
}

fn parse_characteristics(v: &Value) -> Vec<ObjectiveCharacteristic> {
    match v {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|c| {
                let key = text(c.get("key").or_else(|| c.get("name")));
                if key.is_empty() {
                    return None;
                }
                Some(ObjectiveCharacteristic {
                    key,
                    value: text(c.get("value")),
                    unit: opt_text(c.get("unit")),
                })
            })
            .collect(),
        // {"Бюджет": "0", …}
        Value::Object(map) => map
            .iter()
            .map(|(k, val)| ObjectiveCharacteristic {
                key: k.clone(),
                value: text(Some(val)),
                unit: None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_item(v: &Value, depth: usize) -> Option<ObjectiveItemDraft> {
    if let Some(s) = v.as_str() {
        return Some(ObjectiveItemDraft {
            title: s.trim().to_string(),
            ..Default::default()
        })
        .filter(|d| !d.title.is_empty());
    }
    let title = text(v.get("title").or_else(|| v.get("name")));
    if title.is_empty() {
        return None;
    }
    Some(ObjectiveItemDraft {
        key: opt_text(v.get("key").or_else(|| v.get("id"))),
        title,
        description: text(v.get("description")),
        kind: opt_text(v.get("kind").or_else(|| v.get("type"))),
        prompt: opt_text(v.get("prompt").or_else(|| v.get("instruction"))),
        query: opt_text(v.get("query")),
        depends_on: list_of_text(v.get("depends_on")),
        children: if depth + 1 < OBJECTIVE_MAX_DEPTH {
            v.get("children")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|i| parse_item(i, depth + 1)).take(MAX_ITEMS).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        },
    })
}

/// `2026-10-15` or a full RFC 3339 timestamp (date-only → end of that day).
fn parse_deadline(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = raw.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    let day = chrono::NaiveDate::parse_from_str(raw.get(..10)?, "%Y-%m-%d").ok()?;
    Some(day.and_hms_opt(23, 59, 59)?.and_utc())
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string().trim_matches('"').to_string(),
    }
}

fn opt_text(v: Option<&Value>) -> Option<String> {
    Some(text(v)).filter(|s| !s.is_empty())
}

fn list_of_text(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().map(|x| text(Some(x))).filter(|s| !s.is_empty()).collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_string()],
        _ => Vec::new(),
    }
}

// ─── Merge a draft into an objective ─────────────────────────────────────────

/// Returns a note when items were kept because they already ran.
pub fn apply(o: &mut Objective, draft: &ObjectiveDraft, req: &ObjectiveAssistRequest) -> Option<String> {
    let mut note = None;
    if req.mode == ObjectiveAssistMode::Fill {
        // Only a stub title ("Лендинг") is worth replacing.
        if let Some(title) = draft.title.as_deref().filter(|t| !t.trim().is_empty())
            && o.title.trim().len() < 8
        {
            o.title = title.to_string();
        }
        if let Some(desc) = draft.description.as_deref().filter(|d| !d.trim().is_empty()) {
            o.description = desc.to_string();
        }
        for c in &draft.characteristics {
            match o
                .characteristics
                .iter_mut()
                .find(|x| x.key.eq_ignore_ascii_case(&c.key))
            {
                Some(existing) if existing.value.trim().is_empty() => *existing = c.clone(),
                Some(_) => {}
                None => o.characteristics.push(c.clone()),
            }
        }
        if !draft.success_criteria.is_empty() {
            o.success_criteria = draft.success_criteria.clone();
        }
        if o.deadline.is_none() {
            o.deadline = draft.deadline;
        }
    }

    if draft.items.is_empty() {
        o.normalize();
        return note;
    }

    if req.mode == ObjectiveAssistMode::Refine {
        // Gap-closing work always comes after what already ran.
        insert_drafts(o, None, &draft.items);
        o.normalize();
        return None;
    }

    let parent = req.item_id.clone();
    let scope: Vec<String> = match parent.as_deref() {
        Some(pid) => o.children(pid).map(|c| c.id.clone()).collect(),
        None => o.items.iter().map(|i| i.id.clone()).collect(),
    };
    let touched = scope.iter().any(|id| {
        o.item(id).is_some_and(|i| {
            i.status != ObjectiveItemStatus::Pending || i.result.is_some() || i.workflow_id.is_some()
        })
    });

    if touched {
        // Keep executed work; append drafted items after it.
        note = Some("existing items already ran — new items were appended".into());
    } else {
        o.items.retain(|i| !scope.contains(&i.id));
    }
    insert_drafts(o, parent.as_deref(), &draft.items);
    if let Some(pid) = parent.as_deref() {
        // A split item is now a container; its own scenario no longer applies.
        if let Some(item) = o.item_mut(pid) {
            item.reset();
            item.workflow_id = None;
            item.scenario_hash = None;
        }
    }
    o.normalize();
    note
}

fn insert_drafts(o: &mut Objective, parent: Option<&str>, drafts: &[ObjectiveItemDraft]) {
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut created: Vec<(String, &ObjectiveItemDraft)> = Vec::new();
    for d in drafts {
        let id = o.next_item_id(parent);
        let mut item = ObjectiveItem::new(id.clone(), d.title.clone());
        item.parent_id = parent.map(str::to_string);
        item.description = d.description.clone();
        item.kind = match d.kind.as_deref().map(ObjectiveItemKind::parse) {
            // A drafted item has no scenario to point at.
            Some(ObjectiveItemKind::Workflow) | None => ObjectiveItemKind::Ai,
            Some(k) => k,
        };
        item.prompt = d.prompt.clone();
        item.query = d.query.clone();
        o.items.push(item);
        if let Some(k) = &d.key {
            keys.insert(k.to_lowercase(), id.clone());
        }
        keys.insert(d.title.to_lowercase(), id.clone());
        created.push((id, d));
    }
    for (id, d) in &created {
        let deps: Vec<String> = d
            .depends_on
            .iter()
            .filter_map(|k| {
                keys.get(&k.to_lowercase())
                    .cloned()
                    .or_else(|| o.item(k).map(|i| i.id.clone()))
            })
            .filter(|dep| dep != id)
            .collect();
        if let Some(item) = o.item_mut(id) {
            item.depends_on = deps;
        }
        if !d.children.is_empty() && o.depth(id) + 1 < OBJECTIVE_MAX_DEPTH {
            insert_drafts(o, Some(id), &d.children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lenient_draft_and_applies_tree() {
        let v = json!({
            "message": "ok",
            "description": "Сделать лендинг",
            "characteristics": {"Бюджет": 0, "Срок": "7 дней"},
            "success_criteria": "Страница опубликована",
            "deadline": "2026-10-15",
            "items": [
                {"key": "a", "title": "Исследовать конкурентов", "kind": "research", "query": "landing builders"},
                {"key": "b", "title": "Написать тексты", "kind": "ai", "depends_on": ["a"],
                 "children": [{"title": "Заголовок"}, "Подзаголовок"]},
                {"title": "Оплатить домен", "kind": "manual"}
            ]
        });
        let draft = parse_draft(&v);
        assert_eq!(draft.characteristics.len(), 2);
        assert_eq!(draft.success_criteria, vec!["Страница опубликована"]);
        assert_eq!(draft.items[1].children.len(), 2);

        let mut o = Objective::new("Лендинг");
        let req = ObjectiveAssistRequest::default();
        assert!(apply(&mut o, &draft, &req).is_none());
        assert_eq!(o.description, "Сделать лендинг");
        assert_eq!(o.deadline.unwrap().format("%Y-%m-%d").to_string(), "2026-10-15");
        let ids: Vec<String> = o.ordered_ids();
        assert_eq!(ids, vec!["i1", "i2", "i2.1", "i2.2", "i3"]);
        assert_eq!(o.item("i2").unwrap().depends_on, vec!["i1"]);
        assert_eq!(o.item("i1").unwrap().kind, ObjectiveItemKind::Research);
        assert_eq!(o.item("i3").unwrap().kind, ObjectiveItemKind::Manual);
        assert_eq!(o.progress.total, 4);
    }

    #[test]
    fn decompose_item_replaces_pending_children_only() {
        let mut o = Objective::new("x");
        o.items.push(ObjectiveItem::new("i1", "Big step"));
        o.normalize();
        let req = ObjectiveAssistRequest {
            mode: ObjectiveAssistMode::Decompose,
            item_id: Some("i1".into()),
            ..Default::default()
        };
        let draft = parse_draft(&json!({"items": ["A", "B"]}));
        apply(&mut o, &draft, &req);
        assert_eq!(o.ordered_ids(), vec!["i1", "i1.1", "i1.2"]);
        // Second split replaces untouched children.
        let draft = parse_draft(&json!({"items": ["C"]}));
        apply(&mut o, &draft, &req);
        assert_eq!(o.ordered_ids(), vec!["i1", "i1.1"]);
        assert_eq!(o.item("i1.1").unwrap().title, "C");
        // Executed children are kept, new ones appended.
        o.item_mut("i1.1").unwrap().status = ObjectiveItemStatus::Done;
        let draft = parse_draft(&json!({"items": ["D"]}));
        assert!(apply(&mut o, &draft, &req).is_some());
        assert_eq!(o.ordered_ids(), vec!["i1", "i1.1", "i1.2"]);
    }
}
