//! Page reader: turns raw HTML into readable text, metadata, links and
//! CSS-selected data so downstream nodes (LLM, transform) can work with it.

use encoding_rs::Encoding;
use reqwest::Url;
use scraper::{ElementRef, Html, Node, Selector};
use serde::Serialize;
use serde_json::{Map, Value, json};

/// Elements whose content is never part of the readable text.
const SKIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "canvas", "iframe", "object", "embed",
    "head", "form", "button", "select", "input", "textarea", "nav", "aside", "footer", "dialog",
];

/// Class / id words (split on `-`, `_`, space) that mark boilerplate blocks:
/// cookie bars, share widgets, language dropdowns, edit links, …
const BOILERPLATE_HINTS: &[&str] = &[
    "cookie",
    "cookies",
    "ad",
    "ads",
    "advert",
    "advertisement",
    "share",
    "sharing",
    "social",
    "popup",
    "modal",
    "newsletter",
    "subscribe",
    "subscription",
    "breadcrumb",
    "breadcrumbs",
    "dropdown",
    "sidebar",
    "noprint",
    "editsection",
];

const BLOCK_TAGS: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "main",
    "header",
    "ul",
    "ol",
    "dl",
    "dd",
    "dt",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "blockquote",
    "figure",
    "figcaption",
    "address",
    "details",
    "summary",
    "hr",
];

const MAX_LINKS: usize = 100;
/// `article` / `main` is used as the content root only when it holds this much text.
const MIN_MAIN_CHARS: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PageLink {
    pub text: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PageContent {
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    pub text: String,
    pub truncated: bool,
    pub headings: Vec<String>,
    pub links: Vec<PageLink>,
}

#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub max_chars: usize,
    /// Optional CSS selector limiting the text to matching elements.
    pub selector: Option<String>,
    pub include_links: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            max_chars: 8_000,
            selector: None,
            include_links: true,
        }
    }
}

/// Decode a response body using the HTTP charset, a BOM, or `<meta charset>`.
pub fn decode_body(bytes: &[u8], content_type: Option<&str>) -> String {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        return enc
            .decode_without_bom_handling(&bytes[bom_len..])
            .0
            .into_owned();
    }
    let label = content_type
        .and_then(charset_from_content_type)
        .or_else(|| sniff_meta_charset(bytes));
    let encoding = label
        .and_then(|l| Encoding::for_label(l.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}

fn charset_from_content_type(ct: &str) -> Option<String> {
    ct.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| v.trim().trim_matches(['"', '\'']).to_string())
    })
}

fn sniff_meta_charset(bytes: &[u8]) -> Option<String> {
    let head = &bytes[..bytes.len().min(4096)];
    let lower = String::from_utf8_lossy(head).to_ascii_lowercase();
    let idx = lower.find("charset=")?;
    let rest = lower[idx + 8..].trim_start_matches(['"', '\'', ' ']);
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(rest.len());
    let label = &rest[..end];
    (!label.is_empty()).then(|| label.to_string())
}

/// True when the body should be parsed as HTML.
pub fn is_html(content_type: Option<&str>, body: &str) -> bool {
    if let Some(ct) = content_type {
        let ct = ct.to_ascii_lowercase();
        if ct.contains("html") || ct.contains("xml") {
            return true;
        }
        if ct.starts_with("text/") || ct.contains("json") {
            return false;
        }
    }
    let head: String = body
        .chars()
        .take(1024)
        .collect::<String>()
        .to_ascii_lowercase();
    head.contains("<html") || head.contains("<!doctype html") || head.contains("<body")
}

/// True when the content type is something we can turn into text at all.
pub fn is_textual(content_type: Option<&str>) -> bool {
    match content_type {
        None => true,
        Some(ct) => {
            let ct = ct.to_ascii_lowercase();
            ct.starts_with("text/")
                || ct.contains("html")
                || ct.contains("xml")
                || ct.contains("json")
                || ct.contains("javascript")
        }
    }
}

/// Parse an HTML document into readable content.
pub fn read_html(html: &str, base_url: &str, opts: &ReadOptions) -> Result<PageContent, String> {
    let doc = Html::parse_document(html);
    let base = Url::parse(base_url).ok();

    let title = meta_content(&doc, "meta[property='og:title']")
        .or_else(|| first_text(&doc, "title"))
        .or_else(|| first_text(&doc, "h1"))
        .unwrap_or_default();
    let description = meta_content(&doc, "meta[name='description']")
        .or_else(|| meta_content(&doc, "meta[property='og:description']"))
        .unwrap_or_default();
    let published = meta_content(&doc, "meta[property='article:published_time']")
        .or_else(|| meta_content(&doc, "meta[name='date']"))
        .or_else(|| attr_of(&doc, "time[datetime]", "datetime"));
    let lang = attr_of(&doc, "html[lang]", "lang");

    let roots = content_roots(&doc, opts.selector.as_deref())?;

    let mut writer = TextWriter::default();
    let mut headings = Vec::new();
    for root in &roots {
        walk(*root, &mut writer, &mut headings, true);
        writer.blank_line();
    }
    let full = writer.finish();
    let (text, truncated) = clip_chars(&full, opts.max_chars);

    let links = if opts.include_links {
        collect_links(&roots, base.as_ref())
    } else {
        Vec::new()
    };

    Ok(PageContent {
        title,
        description,
        lang,
        published,
        text,
        truncated,
        headings,
        links,
    })
}

/// Pull structured data with CSS selectors.
///
/// Each field maps a name to `"selector"`, `"selector@attr"`; a trailing `[]`
/// returns every match as an array instead of the first one.
pub fn select_fields(
    html: &str,
    base_url: &str,
    fields: &Map<String, Value>,
) -> Result<Value, String> {
    let doc = Html::parse_document(html);
    let base = Url::parse(base_url).ok();
    let mut out = Map::new();
    for (name, spec) in fields {
        let Some(spec) = spec.as_str() else {
            continue;
        };
        let (spec, many) = match spec.trim().strip_suffix("[]") {
            Some(s) => (s.trim(), true),
            None => (spec.trim(), false),
        };
        let (css, attr) = match spec.rsplit_once('@') {
            Some((css, attr)) if !attr.contains([' ', ']', '=']) => (css.trim(), Some(attr.trim())),
            _ => (spec, None),
        };
        let selector = Selector::parse(css)
            .map_err(|e| format!("field `{name}`: bad selector `{css}`: {e}"))?;
        let mut values = doc.select(&selector).filter_map(|el| {
            let raw = match attr {
                Some(a) => el.value().attr(a).map(str::to_string),
                None => Some(element_text(el)),
            }?;
            let value = match (attr, base.as_ref()) {
                (Some("href" | "src"), Some(base)) => {
                    base.join(raw.trim()).map(|u| u.to_string()).unwrap_or(raw)
                }
                _ => raw,
            };
            (!value.trim().is_empty()).then(|| value.trim().to_string())
        });
        let value = if many {
            Value::Array(values.map(Value::String).collect())
        } else {
            values.next().map(Value::String).unwrap_or(Value::Null)
        };
        out.insert(name.clone(), value);
    }
    Ok(Value::Object(out))
}

/// Split text into chunks of at most `size` chars, preferring paragraph breaks.
pub fn chunk_text(text: &str, size: usize) -> Vec<String> {
    let size = size.max(200);
    let mut chunks = Vec::new();
    let mut current = String::new();
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if !current.is_empty() && current.chars().count() + para.chars().count() + 2 > size {
            chunks.push(std::mem::take(&mut current));
        }
        if para.chars().count() > size {
            let chars: Vec<char> = para.chars().collect();
            for piece in chars.chunks(size) {
                chunks.push(piece.iter().collect());
            }
            continue;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

pub fn clip_chars(s: &str, max: usize) -> (String, bool) {
    match s.char_indices().nth(max) {
        Some((idx, _)) => (s[..idx].trim_end().to_string(), true),
        None => (s.to_string(), false),
    }
}

/// Serializable summary of a page, the shape nodes expose to the board.
pub fn page_json(url: &str, status: u16, page: &PageContent) -> Value {
    json!({
        "url": url,
        "status": status,
        "title": page.title,
        "description": page.description,
        "lang": page.lang,
        "published": page.published,
        "text": page.text,
        "truncated": page.truncated,
        "headings": page.headings,
        "links": page.links,
    })
}

fn content_roots<'a>(doc: &'a Html, selector: Option<&str>) -> Result<Vec<ElementRef<'a>>, String> {
    if let Some(css) = selector.map(str::trim).filter(|s| !s.is_empty()) {
        let sel = Selector::parse(css).map_err(|e| format!("bad selector `{css}`: {e}"))?;
        return Ok(doc.select(&sel).collect());
    }
    for css in [
        "[itemprop='articleBody']",
        ".mw-parser-output",
        "article",
        "main",
        "[role='main']",
        "#content",
        ".content",
        ".post",
        ".article",
    ] {
        let sel = Selector::parse(css).expect("static selector");
        let matches: Vec<ElementRef<'a>> = doc.select(&sel).collect();
        // Several <article>s mean a feed / listing page: read the whole container instead.
        if css == "article" && matches.len() > 1 {
            continue;
        }
        let best = matches
            .into_iter()
            .map(|el| (text_len(el), el))
            .max_by_key(|(len, _)| *len);
        if let Some((len, el)) = best {
            if len >= MIN_MAIN_CHARS {
                return Ok(vec![el]);
            }
        }
    }
    let body = Selector::parse("body").expect("static selector");
    Ok(doc
        .select(&body)
        .next()
        .map(|b| vec![b])
        .unwrap_or_else(|| vec![doc.root_element()]))
}

fn text_len(el: ElementRef<'_>) -> usize {
    el.text().map(|t| t.trim().chars().count()).sum()
}

fn is_skipped(el: ElementRef<'_>, is_root: bool) -> bool {
    let v = el.value();
    let name = v.name();
    if SKIP_TAGS.contains(&name) {
        return true;
    }
    if v.attr("hidden").is_some() || v.attr("aria-hidden") == Some("true") {
        return true;
    }
    if is_root {
        return false;
    }
    if matches!(
        v.attr("role"),
        Some("navigation" | "banner" | "contentinfo" | "complementary")
    ) {
        return true;
    }
    let marker = format!(
        "{} {}",
        v.attr("class").unwrap_or(""),
        v.attr("id").unwrap_or("")
    )
    .to_ascii_lowercase();
    marker
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .any(|word| BOILERPLATE_HINTS.contains(&word))
}

fn walk(el: ElementRef<'_>, w: &mut TextWriter, headings: &mut Vec<String>, is_root: bool) {
    if is_skipped(el, is_root) {
        return;
    }
    let name = el.value().name();
    match name {
        "br" => {
            w.newline();
            return;
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let text = element_text(el);
            if !text.is_empty() {
                let level = name[1..].parse::<usize>().unwrap_or(1);
                w.blank_line();
                w.push_raw(&format!("{} {text}", "#".repeat(level)));
                w.blank_line();
                headings.push(text);
            }
            return;
        }
        "pre" => {
            let text: String = el.text().collect();
            if !text.trim().is_empty() {
                w.blank_line();
                w.push_raw(text.trim_end());
                w.blank_line();
            }
            return;
        }
        "li" => {
            w.newline();
            w.push_raw("- ");
        }
        "tr" => w.newline(),
        "td" | "th" => {
            if !w.at_line_start() {
                w.push_raw(" | ");
            }
        }
        // Adjacent tag links (`<a>A</a><a>B</a>`) would otherwise glue into "AB".
        "a" => {
            if w.out.ends_with(|c: char| c.is_alphanumeric()) {
                w.pending_space = true;
            }
        }
        "img" => {
            if let Some(alt) = el
                .value()
                .attr("alt")
                .map(str::trim)
                .filter(|a| !a.is_empty())
            {
                w.push_text(&format!("[{alt}]"));
            }
            return;
        }
        _ if BLOCK_TAGS.contains(&name) => w.blank_line(),
        _ => {}
    }
    for child in el.children() {
        match child.value() {
            Node::Text(t) => w.push_text(t),
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    walk(child_el, w, headings, false);
                }
            }
            _ => {}
        }
    }
    if BLOCK_TAGS.contains(&name) || name == "li" {
        if name == "li" {
            w.newline();
        } else {
            w.blank_line();
        }
    }
}

fn collect_links(roots: &[ElementRef<'_>], base: Option<&Url>) -> Vec<PageLink> {
    let sel = Selector::parse("a[href]").expect("static selector");
    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();
    for root in roots {
        for a in root.select(&sel) {
            let href = a.value().attr("href").unwrap_or("").trim();
            if href.is_empty() || href.starts_with('#') {
                continue;
            }
            let resolved = match base {
                Some(b) => b.join(href).ok(),
                None => Url::parse(href).ok(),
            };
            let Some(mut url) = resolved else {
                continue;
            };
            if url.scheme() != "http" && url.scheme() != "https" {
                continue;
            }
            url.set_fragment(None);
            let url = url.to_string();
            if !seen.insert(url.clone()) {
                continue;
            }
            links.push(PageLink {
                text: element_text(a),
                url,
            });
            if links.len() >= MAX_LINKS {
                return links;
            }
        }
    }
    links
}

fn element_text(el: ElementRef<'_>) -> String {
    collapse_ws(&el.text().collect::<String>())
}

fn first_text(doc: &Html, css: &str) -> Option<String> {
    let sel = Selector::parse(css).ok()?;
    doc.select(&sel).map(element_text).find(|t| !t.is_empty())
}

fn meta_content(doc: &Html, css: &str) -> Option<String> {
    attr_of(doc, css, "content")
}

fn attr_of(doc: &Html, css: &str, attr: &str) -> Option<String> {
    let sel = Selector::parse(css).ok()?;
    doc.select(&sel)
        .filter_map(|el| el.value().attr(attr))
        .map(collapse_ws)
        .find(|v| !v.is_empty())
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Accumulates text with collapsed whitespace and explicit line structure.
#[derive(Default)]
struct TextWriter {
    out: String,
    pending_space: bool,
}

impl TextWriter {
    fn push_text(&mut self, text: &str) {
        let starts_ws = text.starts_with(char::is_whitespace);
        let ends_ws = text.ends_with(char::is_whitespace);
        let words = collapse_ws(text);
        if words.is_empty() {
            if starts_ws || ends_ws {
                self.pending_space = true;
            }
            return;
        }
        if (self.pending_space || starts_ws) && !self.at_line_start() && !self.out.ends_with(' ') {
            self.out.push(' ');
        }
        self.out.push_str(&words);
        self.pending_space = ends_ws;
    }

    fn push_raw(&mut self, text: &str) {
        if self.pending_space && !self.at_line_start() && !text.starts_with(' ') {
            self.out.push(' ');
        }
        self.out.push_str(text);
        self.pending_space = false;
    }

    fn at_line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    fn newline(&mut self) {
        self.pending_space = false;
        trim_trailing_spaces(&mut self.out);
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn blank_line(&mut self) {
        self.newline();
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn finish(self) -> String {
        let mut lines: Vec<&str> = Vec::new();
        let mut blank = 0;
        for line in self.out.lines() {
            let line = line.trim_end();
            // Drop empty list bullets / table separators left by skipped content.
            let meaningful = !matches!(line.trim(), "" | "-" | "|");
            if meaningful {
                blank = 0;
                lines.push(line);
            } else {
                blank += 1;
                if blank == 1 && !lines.is_empty() {
                    lines.push("");
                }
            }
        }
        lines.join("\n").trim().to_string()
    }
}

fn trim_trailing_spaces(s: &mut String) {
    while s.ends_with(' ') {
        s.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!doctype html><html lang="ru"><head>
        <meta charset="utf-8"><title>Заголовок страницы</title>
        <meta name="description" content="Описание">
        <style>.x{color:red}</style><script>var a = 1;</script>
        </head><body>
        <nav><a href="/menu">Меню</a></nav>
        <div class="cookie-banner">Мы используем cookie</div>
        <article>
          <h1>Главная новость</h1>
          <p>Первый абзац с <a href="/details#top">подробностями</a> и   лишними   пробелами.</p>
          <p>Второй абзац текста, достаточно длинный, чтобы статья считалась основным содержимым страницы.
             Добавим ещё немного слов, чтобы преодолеть порог в двести символов для выбора корня.</p>
          <ul><li>Пункт один</li><li>Пункт два</li></ul>
          <table><tr><th>Цена</th><td class="price">100 ₽</td></tr></table>
        </article>
        <footer>Подвал</footer>
        </body></html>"#;

    #[test]
    fn reads_main_content_without_boilerplate() {
        let page = read_html(PAGE, "https://example.com/news/1", &ReadOptions::default()).unwrap();
        assert_eq!(page.title, "Заголовок страницы");
        assert_eq!(page.description, "Описание");
        assert_eq!(page.lang.as_deref(), Some("ru"));
        assert!(page.text.starts_with("# Главная новость"), "{}", page.text);
        assert!(
            page.text
                .contains("Первый абзац с подробностями и лишними пробелами.")
        );
        assert!(
            page.text.contains("- Пункт один\n- Пункт два"),
            "{}",
            page.text
        );
        assert!(page.text.contains("\nЦена | 100 ₽"), "{}", page.text);
        for junk in ["Меню", "cookie", "Подвал", "color:red", "var a"] {
            assert!(!page.text.contains(junk), "leaked `{junk}`: {}", page.text);
        }
        assert_eq!(page.headings, vec!["Главная новость"]);
        assert_eq!(page.links.len(), 1);
        assert_eq!(page.links[0].url, "https://example.com/details");
        assert_eq!(page.links[0].text, "подробностями");
    }

    #[test]
    fn listing_page_reads_all_articles_and_spaces_links() {
        let html = format!(
            "<html><body><main>{}</main></body></html>",
            (1..=3)
                .map(|i| format!(
                    "<article><h2>Новость {i}</h2><p>{}</p><a href=\"/t/a\">Смартфоны</a><a href=\"/t/b\">Сети</a></article>",
                    "текст ".repeat(30)
                ))
                .collect::<String>()
        );
        let page = read_html(&html, "https://example.com/", &ReadOptions::default()).unwrap();
        assert_eq!(page.headings, vec!["Новость 1", "Новость 2", "Новость 3"]);
        assert!(page.text.contains("Смартфоны Сети"), "{}", page.text);
    }

    #[test]
    fn truncates_by_chars() {
        let opts = ReadOptions {
            max_chars: 10,
            ..Default::default()
        };
        let page = read_html(PAGE, "https://example.com/", &opts).unwrap();
        assert!(page.truncated);
        assert!(page.text.chars().count() <= 10);
    }

    #[test]
    fn selector_limits_text() {
        let opts = ReadOptions {
            selector: Some("li".into()),
            ..Default::default()
        };
        let page = read_html(PAGE, "https://example.com/", &opts).unwrap();
        assert_eq!(page.text, "- Пункт один\n\n- Пункт два");
    }

    #[test]
    fn selects_fields() {
        let fields = serde_json::json!({
            "price": ".price",
            "items": "li[]",
            "link": "article a@href",
            "missing": ".nope"
        });
        let data =
            select_fields(PAGE, "https://example.com/a/", fields.as_object().unwrap()).unwrap();
        assert_eq!(data["price"], "100 ₽");
        assert_eq!(
            data["items"],
            serde_json::json!(["Пункт один", "Пункт два"])
        );
        assert_eq!(data["link"], "https://example.com/details#top");
        assert_eq!(data["missing"], Value::Null);
    }

    #[test]
    fn decodes_windows_1251() {
        let (bytes, _, _) = encoding_rs::WINDOWS_1251.encode("<p>Привет</p>");
        assert_eq!(
            decode_body(&bytes, Some("text/html; charset=windows-1251")),
            "<p>Привет</p>"
        );
        let mut with_meta = b"<meta charset=\"windows-1251\">".to_vec();
        with_meta.extend_from_slice(&bytes);
        assert!(decode_body(&with_meta, Some("text/html")).ends_with("<p>Привет</p>"));
    }

    #[test]
    fn chunks_on_paragraphs() {
        let text = format!(
            "{}\n\n{}\n\n{}",
            "a".repeat(150),
            "b".repeat(150),
            "c".repeat(150)
        );
        let chunks = chunk_text(&text, 320);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains('a') && chunks[0].contains('b'));
    }
}
