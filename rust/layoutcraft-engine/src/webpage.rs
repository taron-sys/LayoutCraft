//! File > Export > Web page: a Rust port of `web/webpage.js` (docs/34, step N4.6). The pages' SVG are put inline into one self-contained HTML file (live, selectable text) and the
//! hyperlinks of the shapes become real `<a>` elements around the shapes. Same text as the web (`tests/webpage_oracle.rs` compares them on every example drawing).
use serde_json::Value;

use crate::svgutil::{is_js_space, js_number, js_string};
use crate::webpage_template::{AFTER_NAV, AFTER_TITLE, HEAD, TAIL};
use crate::Engine;

/// One hyperlink of a shape (the fields `buildWebPage` reads).
#[derive(Debug, Clone, Default)]
pub struct Link {
    pub address: Option<String>,
    pub sub_address: Option<String>,
    pub description: Option<String>,
    pub new_window: bool,
    pub invisible: bool,
}

/// A page of the file: its name, its SVG text and its links by shape id in the order they were collected.
#[derive(Debug, Clone, Default)]
pub struct WebPage {
    pub name: String,
    pub svg: String,
    pub links: Vec<(String, Vec<Link>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WebPageResult {
    pub html: String,
    pub links: usize,
    pub dropped: usize,
}

fn esc(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

fn starts_with_ci(a: &str, prefix: &str) -> bool {
    a.len() >= prefix.len() && a.is_char_boundary(prefix.len()) && a[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// The href to write for an address, or None when it is not safe to link.
pub fn safe_href(address: &str) -> Option<String> {
    let a = address.trim_matches(is_js_space);
    if a.is_empty() {
        return None;
    }
    if ["http:", "https:", "mailto:", "tel:"].iter().any(|p| starts_with_ci(a, p)) {
        return Some(a.to_string());
    }
    // ^[a-z][a-z0-9+.-]*:  (any other scheme)
    let mut chars = a.chars();
    if chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        let rest: String = chars.take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')).collect();
        if a[1 + rest.len()..].starts_with(':') {
            return None;
        }
    }
    // ^\s*(\/\/|\\\\)  (protocol-relative / UNC)
    let lead = a.trim_start_matches(is_js_space);
    if lead.starts_with("//") || lead.starts_with("\\\\") {
        return None;
    }
    Some(a.to_string())
}

/// Make the ids of one inline SVG unique to its page: every `id="x"`, `url(#x)` and `href="#x"` of the ids found.
pub fn prefix_ids(svg: &str, prefix: &str) -> String {
    // the ids: /\sid="([^"]+)"/g
    let mut ids: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(at) = svg[from..].find("id=\"") {
        let pos = from + at;
        let prev_ok = svg[..pos].chars().next_back().is_some_and(is_js_space);
        let body = &svg[pos + 4..];
        match body.find('"') {
            Some(end) if prev_ok && end > 0 => {
                let id = &body[..end];
                if !ids.iter().any(|i| i == id) {
                    ids.push(id.to_string());
                }
                from = pos + 4 + end + 1;
            }
            _ => from = pos + 3,
        }
    }
    let mut out = svg.to_string();
    for id in &ids {
        out = replace_attr_id(&out, id, prefix);
        out = out.replace(&format!("url(#{id})"), &format!("url(#{prefix}{id})"));
        out = out.replace(&format!("href=\"#{id}\""), &format!("href=\"#{prefix}{id}\""));
    }
    out
}

/// `replace(/(\sid=")ID(")/g, '$1PREFIXID$2')`.
fn replace_attr_id(text: &str, id: &str, prefix: &str) -> String {
    let needle = format!("id=\"{id}\"");
    let mut out = String::with_capacity(text.len() + 16);
    let mut from = 0;
    while let Some(at) = text[from..].find(&needle) {
        let pos = from + at;
        let prev_ok = text[..pos].chars().next_back().is_some_and(is_js_space);
        out.push_str(&text[from..pos]);
        if prev_ok {
            out.push_str(&format!("id=\"{prefix}{id}\""));
        } else {
            out.push_str(&needle);
        }
        from = pos + needle.len();
    }
    out.push_str(&text[from..]);
    out
}

/// Wrap the `<g data-vid="vid" ...>` element of an SVG text in `open` (an `<a ...>` tag), an optional `<title>` and `</a>`; the text unchanged when the shape is not found.
pub fn wrap_shape(svg: &str, vid: &str, open: &str, title_xml: &str) -> String {
    let Some(start) = svg.find(&format!("<g data-vid=\"{vid}\"")) else {
        return svg.to_string();
    };
    let b = svg.as_bytes();
    let mut depth: i64 = 0;
    let mut i = start;
    while i < b.len() {
        if svg[i..].starts_with("</g>") {
            depth -= 1;
            if depth == 0 {
                let end = i + 4;
                return format!("{}{}{}{}</a>{}", &svg[..start], open, title_xml, &svg[start..end], &svg[end..]);
            }
            i += 4;
            continue;
        }
        // <g(?=[\s/>])[^>]*?(\/?)>
        if svg[i..].starts_with("<g") && svg[i + 2..].chars().next().is_some_and(|c| is_js_space(c) || c == '/' || c == '>') {
            if let Some(rel) = svg[i + 2..].find('>') {
                let close = i + 2 + rel;
                let self_closing = rel >= 1 && b[close - 1] == b'/';
                if !self_closing {
                    depth += 1;
                }
                if depth == 0 {
                    let end = close + 1;
                    return format!("{}{}{}{}</a>{}", &svg[..start], open, title_xml, &svg[start..end], &svg[end..]);
                }
                i = close + 1;
                continue;
            }
        }
        i += svg[i..].chars().next().map_or(1, char::len_utf8);
    }
    svg.to_string()
}

/// A JavaScript object key that is an array index (those keys come first, in ascending order).
fn array_index(key: &str) -> Option<u64> {
    let ok = key == "0" || (!key.is_empty() && !key.starts_with('0') && key.chars().all(|c| c.is_ascii_digit()));
    if !ok || key.len() > 10 {
        return None;
    }
    key.parse::<u64>().ok().filter(|n| *n < 4_294_967_295)
}

/// The HTML file for `pages` (`buildWebPage`).
pub fn build_web_page(pages: &[WebPage], title: &str) -> WebPageResult {
    let (mut links, mut dropped) = (0usize, 0usize);
    let mut name_to_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, p) in pages.iter().enumerate() {
        name_to_index.insert(p.name.to_lowercase(), i);
    }
    let mut sections = Vec::new();
    for (i, p) in pages.iter().enumerate() {
        let mut svg = p.svg.clone();
        // .replace(/^<\?xml[^>]*\?>\s*/, '')
        if svg.starts_with("<?xml") {
            if let Some(end) = svg.find('>') {
                if svg[..end].ends_with('?') && end >= 2 {
                    svg = svg[end + 1..].trim_start_matches(is_js_space).to_string();
                }
            }
        }
        svg = prefix_ids(&svg, &format!("p{i}-"));
        // Object.entries order: array-index keys ascending, then the others in insertion order
        let mut entries: Vec<&(String, Vec<Link>)> = p.links.iter().collect();
        entries.sort_by_key(|(k, _)| array_index(k).map_or((1u8, 0u64), |n| (0, n)));
        for (vid, list) in entries {
            let Some(link) = list.iter().find(|l| !l.invisible).or_else(|| list.first()) else { continue };
            let sub = link.sub_address.as_deref().filter(|s| !s.is_empty());
            let target = match (sub, link.address.as_deref().is_none_or(str::is_empty)) {
                (Some(sub), true) => name_to_index.get(&sub.to_lowercase()).copied(),
                _ => None,
            };
            let open = if let Some(t) = target {
                Some(format!("<a href=\"#page:{t}\" data-page=\"{t}\">"))
            } else {
                safe_href(link.address.as_deref().unwrap_or("")).map(|href| format!("<a href=\"{}\"{} rel=\"noopener noreferrer\">", esc(&href), if link.new_window { " target=\"_blank\"" } else { "" }))
            };
            let Some(open) = open else {
                dropped += 1;
                continue;
            };
            let title_xml = link.description.as_deref().filter(|d| !d.is_empty()).map(|d| format!("<title>{}</title>", esc(d))).unwrap_or_default();
            let wrapped = wrap_shape(&svg, vid, &open, &title_xml);
            if wrapped == svg {
                continue;
            }
            svg = wrapped;
            links += 1;
        }
        sections.push(format!("<section data-i=\"{i}\"{} aria-label=\"{}\">{svg}</section>", if i > 0 { " hidden" } else { "" }, esc(&p.name)));
    }
    let nav: String = pages.iter().enumerate().map(|(i, p)| format!("<button role=\"tab\" aria-selected=\"{}\" data-i=\"{i}\">{}</button>", i == 0, esc(&p.name))).collect();
    let html = format!("{HEAD}{}{AFTER_TITLE}{nav}{AFTER_NAV}{}{TAIL}", esc(title), sections.join(""));
    WebPageResult { html, links, dropped }
}

/// The SVG of page `page` as the web's `exportSvg('page')` writes it with the plain (not fixed page) view: the viewBox of the drawing when the page has groups, else the page.
pub(crate) fn page_svg(engine: &Engine, page: usize) -> Result<String, String> {
    let text = engine.svg(page, true).ok_or("no document")??;
    let size: Value = serde_json::from_str(&engine.page_size_json(page)?).map_err(|e| e.to_string())?;
    let (w, h) = (size["w"].as_f64().ok_or("no page size")?, size["h"].as_f64().ok_or("no page size")?);
    let mut box_ = [0.0, 0.0, w * 96.0, h * 96.0];
    let groups: Value = serde_json::from_str(&engine.group_info_json(page)?).map_err(|e| e.to_string())?;
    let has_groups = match &groups {
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => false,
    };
    if let Some(at) = text.find("viewBox=\"") {
        let rest = &text[at + 9..];
        if let Some(end) = rest.find('"') {
            if has_groups && end > 0 {
                // split(/\s+/).map(Number)
                let parts: Vec<f64> = rest[..end].split(is_js_space).map(js_number).collect();
                if parts.len() == 4 {
                    box_ = [parts[0], parts[1], parts[2], parts[3]];
                }
            }
        }
    }
    // text.replace(/<svg ([^>]*)>/, ...)
    let Some(open_at) = text.find("<svg ") else { return Ok(text) };
    let Some(rel) = text[open_at + 5..].find('>') else { return Ok(text) };
    let close = open_at + 5 + rel;
    let mut attrs = text[open_at + 5..close].to_string();
    for name in ["viewBox", "width", "height"] {
        let key = format!("{name}=\"");
        if let Some(at) = attrs.find(&key) {
            if let Some(end) = attrs[at + key.len()..].find('"') {
                let mut from = at;
                if from > 0 && attrs.as_bytes()[from - 1] == b' ' {
                    from -= 1;
                }
                attrs.replace_range(from..at + key.len() + end + 1, "");
            }
        }
    }
    let vb: Vec<String> = box_.iter().map(|n| js_string(*n)).collect();
    let head = format!("<svg {attrs} width=\"{}\" height=\"{}\" viewBox=\"{}\">", vb[2], vb[3], vb.join(" "));
    Ok(format!("{}{}{}", &text[..open_at], head, &text[close + 1..]))
}

impl Engine {
    /// The pages of the drawing as the data of `build_web_page` (SVG text and the hyperlinks of the shapes the SVG mentions).
    pub fn web_pages(&self) -> Result<Vec<WebPage>, String> {
        let count = self.page_count().ok_or("no document")?;
        let names: Value = serde_json::from_str(&self.page_names_json()?).map_err(|e| e.to_string())?;
        let mut pages = Vec::new();
        for p in 0..count {
            let svg = page_svg(self, p)?;
            let mut vids: Vec<String> = Vec::new();
            let mut from = 0;
            while let Some(at) = svg[from..].find("<g data-vid=\"") {
                let start = from + at + 13;
                match svg[start..].find('"') {
                    Some(end) if end > 0 => {
                        let vid = svg[start..start + end].to_string();
                        if !vids.contains(&vid) {
                            vids.push(vid);
                        }
                        from = start + end;
                    }
                    _ => from = start,
                }
            }
            let mut links = Vec::new();
            for vid in vids {
                let Ok(text) = self.hyperlinks_json(p, &vid) else { continue };
                let list: Vec<Value> = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
                if list.is_empty() {
                    continue;
                }
                let s = |v: &Value, k: &str| v[k].as_str().map(str::to_string);
                links.push((vid, list.iter().map(|l| Link { address: s(l, "address"), sub_address: s(l, "subAddress"), description: s(l, "description"), new_window: l["newWindow"].as_bool().unwrap_or(false), invisible: l["invisible"].as_bool().unwrap_or(false) }).collect()));
            }
            let name = names.get(p).and_then(Value::as_str).filter(|n| !n.is_empty()).map_or_else(|| format!("Page {}", p + 1), str::to_string);
            pages.push(WebPage { name, svg, links });
        }
        Ok(pages)
    }

    /// File > Export > Web page: one self-contained HTML file with a tab per page.
    pub fn export_webpage(&self) -> Result<WebPageResult, String> {
        let pages = self.web_pages()?;
        let title = pages.first().map(|p| p.name.clone()).unwrap_or_else(|| "Drawing".to_string());
        Ok(build_web_page(&pages, &title))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_safe_addresses_become_links() {
        assert_eq!(safe_href(" https://example.org/a ").as_deref(), Some("https://example.org/a"));
        assert_eq!(safe_href("MAILTO:a@b.c").as_deref(), Some("MAILTO:a@b.c"));
        assert_eq!(safe_href("page2.html").as_deref(), Some("page2.html"));
        assert_eq!(safe_href("javascript:alert(1)"), None);
        assert_eq!(safe_href("file:///c:/x"), None);
        assert_eq!(safe_href("//evil.example"), None);
        assert_eq!(safe_href("\\\\server\\share"), None);
        assert_eq!(safe_href("  "), None);
    }

    #[test]
    fn ids_are_prefixed_with_their_references() {
        let svg = "<svg><defs><linearGradient id=\"g1\"/></defs><rect fill=\"url(#g1)\"/><use href=\"#g1\"/><g data-id=\"g1\" id=\"x\"/></svg>";
        let out = prefix_ids(svg, "p3-");
        assert!(out.contains("id=\"p3-g1\"") && out.contains("url(#p3-g1)") && out.contains("href=\"#p3-g1\"") && out.contains("id=\"p3-x\""));
        assert!(out.contains("data-id=\"g1\""), "data-id is not an id");
    }

    #[test]
    fn a_shape_is_wrapped_with_its_nested_groups() {
        let svg = "<svg><g data-vid=\"5\" a=\"1\"><g><rect/></g><g/></g><g data-vid=\"6\"/></svg>";
        let out = wrap_shape(svg, "5", "<a href=\"x\">", "<title>t</title>");
        assert_eq!(out, "<svg><a href=\"x\"><title>t</title><g data-vid=\"5\" a=\"1\"><g><rect/></g><g/></g></a><g data-vid=\"6\"/></svg>");
        assert_eq!(wrap_shape(svg, "9", "<a>", ""), svg);
    }

    #[test]
    fn links_to_page_names_switch_tabs_and_bad_ones_are_dropped() {
        let pages = vec![
            WebPage {
                name: "Start".into(),
                svg: "<svg><g data-vid=\"2\"></g><g data-vid=\"10\"></g><g data-vid=\"3\"></g></svg>".into(),
                links: vec![
                    ("10".into(), vec![Link { sub_address: Some("next".into()), ..Default::default() }]),
                    ("2".into(), vec![Link { address: Some("https://example.org".into()), description: Some("Go <now>".into()), new_window: true, ..Default::default() }]),
                    ("3".into(), vec![Link { address: Some("javascript:x".into()), ..Default::default() }]),
                ],
            },
            WebPage { name: "Next".into(), svg: "<svg></svg>".into(), links: vec![] },
        ];
        let made = build_web_page(&pages, "T");
        assert_eq!((made.links, made.dropped), (2, 1));
        assert!(made.html.contains("<a href=\"#page:1\" data-page=\"1\"><g data-vid=\"10\">"));
        assert!(made.html.contains("<a href=\"https://example.org\" target=\"_blank\" rel=\"noopener noreferrer\"><title>Go &lt;now&gt;</title><g data-vid=\"2\">"));
        assert!(made.html.contains("<section data-i=\"1\" hidden aria-label=\"Next\">"));
        assert!(made.html.starts_with("<!doctype html>\n<html lang=\"en\">") && made.html.ends_with("</body></html>\n"));
    }
}
