//! Structured data already published by pages: JSON-LD, OpenGraph / meta,
//! microdata, data tables and embedded application state (`__NEXT_DATA__`, …).
//!
//! Much of the web exposes machine-readable data next to the visual page; reading
//! it is more reliable than scraping text and often works without a browser.

use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use serde_json::{Map, Value, json};

const MAX_JSON_LD: usize = 50;
const MAX_MICRODATA: usize = 20;
const MAX_TABLES: usize = 10;
const MAX_TABLE_ROWS: usize = 100;
const MAX_CELL_CHARS: usize = 300;
const MAX_EMBEDDED: usize = 5;
/// Embedded state blobs can be megabytes: skip anything larger than this.
const MAX_EMBEDDED_BYTES: usize = 512 * 1024;

/// Global assignments of client-side state that are usually plain JSON.
const STATE_MARKERS: &[&str] = &[
    "window.__INITIAL_STATE__",
    "window.__PRELOADED_STATE__",
    "window.__APOLLO_STATE__",
    "window.__NUXT__",
    "window.__DATA__",
    "window.__remixContext",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructuredData {
    pub json_ld: Vec<Value>,
    pub meta: Map<String, Value>,
    pub microdata: Vec<Value>,
    pub tables: Vec<Value>,
    pub embedded_json: Vec<Value>,
}

impl StructuredData {
    pub fn is_empty(&self) -> bool {
        self.json_ld.is_empty()
            && self.meta.is_empty()
            && self.microdata.is_empty()
            && self.tables.is_empty()
            && self.embedded_json.is_empty()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "json_ld": self.json_ld,
            "meta": self.meta,
            "microdata": self.microdata,
            "tables": self.tables,
            "embedded_json": self.embedded_json,
        })
    }

    /// First `datePublished`-like value in JSON-LD (articles, news).
    pub fn date_published(&self) -> Option<String> {
        self.json_ld.iter().find_map(|item| {
            ["datePublished", "dateCreated", "uploadDate"]
                .iter()
                .find_map(|k| item.get(*k).and_then(Value::as_str))
                .map(str::to_string)
        })
    }
}

pub fn extract_structured(html: &str, base_url: &str) -> StructuredData {
    let doc = Html::parse_document(html);
    let base = Url::parse(base_url).ok();
    StructuredData {
        json_ld: json_ld(&doc),
        meta: meta(&doc, base.as_ref()),
        microdata: microdata(&doc, base.as_ref()),
        tables: tables(&doc),
        embedded_json: embedded_json(&doc),
    }
}

fn sel(css: &str) -> Selector {
    Selector::parse(css).expect("static selector")
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text_of(el: ElementRef<'_>) -> String {
    collapse(&el.text().collect::<String>())
}

fn resolve(base: Option<&Url>, href: &str) -> String {
    match base {
        Some(b) => b
            .join(href.trim())
            .map(|u| u.to_string())
            .unwrap_or_else(|_| href.trim().to_string()),
        None => href.trim().to_string(),
    }
}

fn json_ld(doc: &Html) -> Vec<Value> {
    let mut items = Vec::new();
    for script in doc.select(&sel("script[type*='ld+json']")) {
        let raw: String = script.text().collect();
        let raw = raw
            .trim()
            .trim_start_matches("<!--")
            .trim_end_matches("-->")
            .trim()
            .trim_start_matches("//<![CDATA[")
            .trim_end_matches("//]]>")
            .trim();
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        flatten_ld(value, &mut items);
        if items.len() >= MAX_JSON_LD {
            break;
        }
    }
    items.truncate(MAX_JSON_LD);
    items
}

fn flatten_ld(value: Value, out: &mut Vec<Value>) {
    match value {
        Value::Array(items) => items.into_iter().for_each(|v| flatten_ld(v, out)),
        Value::Object(mut map) => match map.remove("@graph") {
            Some(graph) => flatten_ld(graph, out),
            None => out.push(Value::Object(map)),
        },
        _ => {}
    }
}

/// OpenGraph / Twitter / article / product meta, plus canonical URL and description.
fn meta(doc: &Html, base: Option<&Url>) -> Map<String, Value> {
    let mut out = Map::new();
    for el in doc.select(&sel("meta[content]")) {
        let v = el.value();
        let Some(key) = v.attr("property").or_else(|| v.attr("name")) else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let wanted = ["og:", "twitter:", "article:", "product:", "book:", "music:", "video:"]
            .iter()
            .any(|p| key.starts_with(p))
            || matches!(key.as_str(), "description" | "author" | "keywords" | "date");
        let content = collapse(v.attr("content").unwrap_or(""));
        if wanted && !content.is_empty() && !out.contains_key(&key) {
            out.insert(key, json!(content));
        }
    }
    if let Some(href) = doc
        .select(&sel("link[rel='canonical'][href]"))
        .next()
        .and_then(|l| l.value().attr("href"))
    {
        out.insert("canonical".into(), json!(resolve(base, href)));
    }
    out
}

fn microdata(doc: &Html, base: Option<&Url>) -> Vec<Value> {
    doc.select(&sel("[itemscope]"))
        .filter(|el| {
            // Top-level items only; nested ones are reached through their parent.
            el.value().attr("itemprop").is_none()
                && !el.ancestors().filter_map(ElementRef::wrap).any(|a| {
                    a.value().attr("itemscope").is_some()
                })
        })
        .take(MAX_MICRODATA)
        .map(|el| microdata_item(el, base))
        .collect()
}

fn microdata_item(item: ElementRef<'_>, base: Option<&Url>) -> Value {
    let mut props = Map::new();
    if let Some(t) = item.value().attr("itemtype") {
        let short = t.trim().rsplit('/').next().unwrap_or(t).to_string();
        props.insert("@type".into(), json!(short));
    }
    collect_props(item, base, &mut props);
    Value::Object(props)
}

fn collect_props(scope: ElementRef<'_>, base: Option<&Url>, props: &mut Map<String, Value>) {
    for child in scope.children().filter_map(ElementRef::wrap) {
        let v = child.value();
        if let Some(names) = v.attr("itemprop") {
            let value = if v.attr("itemscope").is_some() {
                microdata_item(child, base)
            } else {
                json!(prop_value(child, base))
            };
            for name in names.split_whitespace() {
                push_prop(props, name, value.clone());
            }
            if v.attr("itemscope").is_some() {
                continue; // its children belong to the nested item
            }
        } else if v.attr("itemscope").is_some() {
            continue; // independent nested item
        }
        collect_props(child, base, props);
    }
}

fn prop_value(el: ElementRef<'_>, base: Option<&Url>) -> String {
    let v = el.value();
    match v.name() {
        "meta" => v.attr("content").unwrap_or("").trim().to_string(),
        "a" | "link" | "area" => resolve(base, v.attr("href").unwrap_or("")),
        "img" | "audio" | "video" | "source" | "iframe" | "embed" | "track" => {
            resolve(base, v.attr("src").unwrap_or(""))
        }
        "time" => v
            .attr("datetime")
            .map(str::to_string)
            .unwrap_or_else(|| text_of(el)),
        "data" | "meter" => v
            .attr("value")
            .map(str::to_string)
            .unwrap_or_else(|| text_of(el)),
        _ => v
            .attr("content")
            .map(str::to_string)
            .unwrap_or_else(|| text_of(el)),
    }
}

fn push_prop(props: &mut Map<String, Value>, name: &str, value: Value) {
    match props.get_mut(name) {
        None => {
            props.insert(name.to_string(), value);
        }
        Some(Value::Array(items)) => items.push(value),
        Some(existing) => {
            let first = existing.take();
            *existing = Value::Array(vec![first, value]);
        }
    }
}

/// Data tables (≥ 2 rows and ≥ 2 columns) as `{caption, headers, rows}`.
fn tables(doc: &Html) -> Vec<Value> {
    let row_sel = sel("tr");
    let cell_sel = sel("th, td");
    let mut out = Vec::new();
    for table in doc.select(&sel("table")) {
        // Nested layout tables are read through their own element.
        let mut rows: Vec<(bool, Vec<String>)> = table
            .select(&row_sel)
            .filter(|tr| {
                tr.ancestors()
                    .filter_map(ElementRef::wrap)
                    .find(|a| a.value().name() == "table")
                    .is_some_and(|t| t.id() == table.id())
            })
            .map(|tr| {
                let cells: Vec<ElementRef<'_>> = tr
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|c| cell_sel.matches(c))
                    .collect();
                let all_th = !cells.is_empty() && cells.iter().all(|c| c.value().name() == "th");
                let texts: Vec<String> = cells
                    .iter()
                    .map(|c| text_of(*c).chars().take(MAX_CELL_CHARS).collect())
                    .collect();
                (all_th, texts)
            })
            .filter(|(_, cells)| cells.iter().any(|c: &String| !c.is_empty()))
            .collect();
        if rows.len() < 2 || rows.iter().map(|(_, c)| c.len()).max().unwrap_or(0) < 2 {
            continue;
        }
        let headers = if rows[0].0 {
            rows.remove(0).1
        } else {
            Vec::new()
        };
        let caption = table
            .select(&sel("caption"))
            .next()
            .map(text_of)
            .unwrap_or_default();
        let body: Vec<Vec<String>> = rows
            .into_iter()
            .take(MAX_TABLE_ROWS)
            .map(|(_, c)| c)
            .collect();
        out.push(json!({ "caption": caption, "headers": headers, "rows": body }));
        if out.len() >= MAX_TABLES {
            break;
        }
    }
    out
}

/// `__NEXT_DATA__`, `<script type="application/json">` and `window.__STATE__ = {…}`.
fn embedded_json(doc: &Html) -> Vec<Value> {
    let mut out = Vec::new();
    for script in doc.select(&sel("script[type='application/json']")) {
        let raw: String = script.text().collect();
        if raw.len() > MAX_EMBEDDED_BYTES {
            continue;
        }
        let Ok(data) = serde_json::from_str::<Value>(raw.trim()) else {
            continue;
        };
        if is_trivial(&data) {
            continue;
        }
        let id = script.value().attr("id").unwrap_or("");
        let source = if id.is_empty() {
            "script[type=application/json]".to_string()
        } else {
            format!("script#{id}")
        };
        out.push(json!({ "source": source, "data": data }));
        if out.len() >= MAX_EMBEDDED {
            return out;
        }
    }
    for script in doc.select(&sel("script:not([src])")) {
        let raw: String = script.text().collect();
        for marker in STATE_MARKERS {
            let Some(idx) = raw.find(marker) else {
                continue;
            };
            let after = &raw[idx + marker.len()..];
            let Some(eq) = after.find('=') else {
                continue;
            };
            if after[..eq].trim().is_empty() {
                if let Some(blob) = balanced_object(&after[eq + 1..]) {
                    if let Ok(data) = serde_json::from_str::<Value>(blob) {
                        if !is_trivial(&data) {
                            let name = marker.trim_start_matches("window.");
                            out.push(json!({ "source": name, "data": data }));
                        }
                    }
                }
            }
            if out.len() >= MAX_EMBEDDED {
                return out;
            }
        }
    }
    out
}

fn is_trivial(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => true,
    }
}

/// The `{…}` object literal at the start of `s`, honouring strings and escapes.
fn balanced_object(s: &str) -> Option<&str> {
    let start = s.find(|c: char| !c.is_whitespace())?;
    if !s[start..].starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_str: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s[start..].char_indices() {
        if i > MAX_EMBEDDED_BYTES {
            return None;
        }
        if let Some(q) = in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                in_str = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => in_str = Some(c),
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Resolve a `$.a.b[0].c` path (as produced for evidence) inside `value`.
pub fn json_path_get<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim().trim_start_matches('$');
    let mut cur = value;
    let mut rest = path;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['.', '[']).unwrap_or(r.len());
            let key = &r[..end];
            cur = cur.get(key)?;
            rest = &r[end..];
        } else if let Some(r) = rest.strip_prefix('[') {
            let end = r.find(']')?;
            let inner = r[..end].trim_matches(['"', '\'']);
            cur = match inner.parse::<usize>() {
                Ok(i) => cur.get(i)?,
                Err(_) => cur.get(inner)?,
            };
            rest = &r[end + 1..];
        } else {
            return None;
        }
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><head>
      <meta property="og:title" content="iPhone 17">
      <meta property="og:type" content="product">
      <meta name="description" content="Новый телефон">
      <meta property="product:price:amount" content="999">
      <link rel="canonical" href="/p/iphone-17">
      <script type="application/ld+json">
        {"@context":"https://schema.org","@graph":[
          {"@type":"Product","name":"iPhone 17","offers":{"@type":"Offer","price":"999","priceCurrency":"USD","availability":"https://schema.org/InStock"}},
          {"@type":"BreadcrumbList","itemListElement":[]}
        ]}
      </script>
      <script type="application/ld+json">{"@type":"NewsArticle","datePublished":"2026-10-04T08:00:00Z"}</script>
      <script type="application/ld+json">{ broken json </script>
      <script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"product":{"id":17,"price":999}}}}</script>
      <script>window.__INITIAL_STATE__ = {"cart":{"items":[1,2]},"note":"a } in string"};</script>
    </head><body>
      <div itemscope itemtype="https://schema.org/Product">
        <span itemprop="name">iPhone 17</span>
        <img itemprop="image" src="/img/17.png">
        <div itemprop="offers" itemscope itemtype="https://schema.org/Offer">
          <meta itemprop="price" content="999"><span itemprop="priceCurrency">USD</span>
        </div>
        <div itemprop="review" itemscope itemtype="https://schema.org/Review"><span itemprop="author">Ann</span></div>
        <div itemprop="review" itemscope itemtype="https://schema.org/Review"><span itemprop="author">Bob</span></div>
      </div>
      <table><caption>Цены</caption>
        <tr><th>Модель</th><th>Цена</th></tr>
        <tr><td>iPhone 17</td><td>999</td></tr>
        <tr><td>iPhone 17 Pro</td><td>1199</td></tr>
      </table>
      <table><tr><td>layout only</td></tr></table>
    </body></html>"#;

    #[test]
    fn extracts_all_kinds() {
        let s = extract_structured(PAGE, "https://shop.test/p/x");
        assert_eq!(s.json_ld.len(), 3, "@graph flattened, broken JSON skipped");
        assert_eq!(s.json_ld[0]["name"], "iPhone 17");
        assert_eq!(s.json_ld[0]["offers"]["price"], "999");
        assert_eq!(s.date_published().as_deref(), Some("2026-10-04T08:00:00Z"));

        assert_eq!(s.meta["og:title"], "iPhone 17");
        assert_eq!(s.meta["product:price:amount"], "999");
        assert_eq!(s.meta["canonical"], "https://shop.test/p/iphone-17");

        assert_eq!(s.microdata.len(), 1);
        let item = &s.microdata[0];
        assert_eq!(item["@type"], "Product");
        assert_eq!(item["image"], "https://shop.test/img/17.png");
        assert_eq!(item["offers"]["price"], "999");
        assert_eq!(item["offers"]["priceCurrency"], "USD");
        assert_eq!(item["review"][1]["author"], "Bob");

        assert_eq!(s.tables.len(), 1, "layout table skipped");
        assert_eq!(s.tables[0]["caption"], "Цены");
        assert_eq!(s.tables[0]["headers"], json!(["Модель", "Цена"]));
        assert_eq!(s.tables[0]["rows"][1], json!(["iPhone 17 Pro", "1199"]));

        assert_eq!(s.embedded_json.len(), 2);
        assert_eq!(s.embedded_json[0]["source"], "script#__NEXT_DATA__");
        assert_eq!(s.embedded_json[0]["data"]["props"]["pageProps"]["product"]["price"], 999);
        assert_eq!(s.embedded_json[1]["source"], "__INITIAL_STATE__");
        assert_eq!(s.embedded_json[1]["data"]["note"], "a } in string");
    }

    #[test]
    fn empty_page_has_no_structure() {
        assert!(extract_structured("<html><body><p>hi</p></body></html>", "https://a.test/").is_empty());
    }

    #[test]
    fn json_paths_resolve() {
        let v = json!({ "a": { "b": [ { "c": 5 }, { "d": "x" } ], "k y": 1 } });
        assert_eq!(json_path_get(&v, "$.a.b[0].c"), Some(&json!(5)));
        assert_eq!(json_path_get(&v, "$.a.b[1].d"), Some(&json!("x")));
        assert_eq!(json_path_get(&v, "$.a[\"k y\"]"), Some(&json!(1)));
        assert_eq!(json_path_get(&v, "$"), Some(&v));
        assert!(json_path_get(&v, "$.a.z").is_none());
    }
}
