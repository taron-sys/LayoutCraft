//! File > Open of a draw.io file: a Rust port of `web/drawio.js` (docs/34, step N4.8b). A .drawio file (an `mxfile` with one `diagram` per page, its content either a plain
//! `mxGraphModel` or the compressed form base64(raw deflate(url-encoded XML))) or a bare `mxGraphModel` becomes a drawing in the engine, one page per diagram, with the SAME
//! engine calls as the web code. `tests/drawio_import_oracle.rs` compares the drawings the two make from the same files.
use std::collections::{BTreeMap, HashMap};
use std::io::Read;

use serde_json::{json, Map, Value};

use crate::drawio_edges::{arrow_index, arrow_size, curve_through, exit_horizontal, label_position, orthogonalize, side_of};
use crate::drawio_shapes::{canonical_name, shape_geometry};
use crate::excalidraw_import::atob;
use crate::svgutil::{is_js_space, js_parse_float, js_round, js_string, Child, Node};
use crate::Engine;

const PX_PER_IN: f64 = 96.0;

/// What `importDrawio` returns.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawioSummary {
    pub pages: usize,
    pub shapes: usize,
    pub connectors: usize,
    pub approximated: BTreeMap<String, usize>,
    pub skipped: BTreeMap<String, usize>,
    pub map: Vec<BTreeMap<String, String>>,
}

// ---- the XML reader of the web code (`parseXmlTree`): lenient, regex based --------------------------------------------------------------------

fn decode(text: &str) -> String {
    // /&(#x[0-9a-fA-F]+|#\d+|amp|lt|gt|quot|apos);/g
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find(';');
        let replaced = end.and_then(|e| {
            let body = &after[..e];
            let ch = if let Some(hex) = body.strip_prefix("#x") {
                (!hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)).flatten()
            } else if let Some(dec) = body.strip_prefix('#') {
                (!dec.is_empty() && dec.chars().all(|c| c.is_ascii_digit())).then(|| dec.parse::<u32>().ok().and_then(char::from_u32)).flatten()
            } else {
                match body {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => None,
                }
            };
            ch.map(|c| (c, e))
        });
        match replaced {
            Some((c, e)) => {
                out.push(c);
                rest = &after[e + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-')
}

/// Try `<(/?)(name)(attrs)\s*(/?)>` at the start of `s` (which starts with '<'): (closing, name, attrs text, self closing, length).
fn tag_at(s: &str) -> Option<(bool, String, Vec<(String, String)>, bool, usize)> {
    let b: Vec<char> = s.chars().collect();
    let mut i = 1;
    let closing = b.get(i) == Some(&'/');
    if closing {
        i += 1;
    }
    let start = i;
    if !b.get(i).is_some_and(|c| c.is_ascii_alphabetic() || *c == '_') {
        return None;
    }
    while i < b.len() && is_name_char(b[i]) {
        i += 1;
    }
    let name: String = b[start..i].iter().collect();
    let mut attrs = Vec::new();
    loop {
        // (?:\s+[\w:.-]+\s*=\s*(?:"[^"]*"|'[^']*'))*
        let mut j = i;
        while j < b.len() && is_js_space(b[j]) {
            j += 1;
        }
        if j == i {
            break;
        }
        let ns = j;
        while j < b.len() && is_name_char(b[j]) {
            j += 1;
        }
        if j == ns {
            break;
        }
        let key: String = b[ns..j].iter().collect();
        while j < b.len() && is_js_space(b[j]) {
            j += 1;
        }
        if b.get(j) != Some(&'=') {
            break;
        }
        j += 1;
        while j < b.len() && is_js_space(b[j]) {
            j += 1;
        }
        let quote = match b.get(j) {
            Some(q @ ('"' | '\'')) => *q,
            _ => break,
        };
        let vs = j + 1;
        let Some(close) = b[vs..].iter().position(|c| *c == quote) else { break };
        attrs.push((key, b[vs..vs + close].iter().collect::<String>()));
        i = vs + close + 1;
    }
    while i < b.len() && is_js_space(b[i]) {
        i += 1;
    }
    let self_closing = b.get(i) == Some(&'/');
    if self_closing {
        i += 1;
    }
    if b.get(i) != Some(&'>') {
        return None;
    }
    Some((closing, name, attrs, self_closing, b[..=i].iter().map(|c| c.len_utf8()).sum()))
}

/// The whole tree: the root has the top-level nodes as children; text nodes are `Child::Text`.
pub fn parse_xml_tree(source: &str) -> Node {
    let mut stack: Vec<Node> = vec![Node { tag: "#root".into(), ..Default::default() }];
    let mut i = 0;
    let bytes = source.as_bytes();
    let push_child = |stack: &mut Vec<Node>, child: Child| {
        stack.last_mut().unwrap().children.push(child);
    };
    fn close(stack: &mut Vec<Node>) {
        if stack.len() > 1 {
            let node = stack.pop().unwrap();
            stack.last_mut().unwrap().children.push(Child::Element(node));
        }
    }
    while i < bytes.len() {
        let rest = &source[i..];
        if bytes[i] != b'<' {
            let end = rest.find('<').unwrap_or(rest.len());
            push_child(&mut stack, Child::Text(decode(&rest[..end])));
            i += end;
            continue;
        }
        if rest.starts_with("<!--") {
            if let Some(e) = rest.find("-->") {
                i += e + 3;
                continue;
            }
        }
        if rest.starts_with("<?") {
            if let Some(e) = rest.find("?>") {
                i += e + 2;
                continue;
            }
        }
        if rest.starts_with("<![CDATA[") {
            if let Some(e) = rest.find("]]>") {
                push_child(&mut stack, Child::Text(rest[9..e].to_string()));
                i += e + 3;
                continue;
            }
        }
        if let Some((closing, name, attrs, self_closing, len)) = tag_at(rest) {
            if closing {
                close(&mut stack);
            } else {
                let mut node = Node { tag: name, ..Default::default() };
                for (k, v) in attrs {
                    node.attrs.insert(k, decode(&v));
                }
                stack.push(node);
                if self_closing {
                    close(&mut stack);
                }
            }
            i += len;
            continue;
        }
        i += 1;
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().unwrap()
}

fn child<'a>(node: &'a Node, tag: &str) -> Vec<&'a Node> {
    node.elements().filter(|c| c.tag == tag).collect()
}

fn first<'a>(node: &'a Node, tag: &str) -> Option<&'a Node> {
    node.elements().find(|c| c.tag == tag)
}

fn number_of(v: Option<&str>, d: f64) -> f64 {
    match v {
        Some(v) => {
            let n = js_parse_float(v);
            if n.is_finite() { n } else { d }
        }
        None => d,
    }
}

fn colour(c: Option<&str>) -> Option<String> {
    let s = c?;
    let hex = s.strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => Some(s.to_lowercase()),
        3 => Some(format!("#{}", hex.chars().flat_map(|c| [c, c]).collect::<String>().to_lowercase())),
        _ => None,
    }
}

/// "a=b;c=d;ellipse" -> {a: b, c: d, name: ellipse} (a bare first token is the shape's style name).
pub fn parse_style(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (i, part) in text.split(';').enumerate() {
        if part.is_empty() {
            continue;
        }
        match part.find('=') {
            None => {
                if i == 0 || !out.contains_key("name") {
                    out.insert("name".to_string(), part.to_string());
                }
            }
            Some(eq) => {
                out.insert(part[..eq].to_string(), part[eq + 1..].to_string());
            }
        }
    }
    out
}

// ---- HTML labels --------------------------------------------------------------------------------------------------------------------------

/// A formatted run of a label, positions in UTF-16 units.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub start: usize,
    pub end: usize,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<String>,
    pub size_px: Option<f64>,
    pub family: Option<String>,
}

#[derive(Clone, Default)]
struct RunStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    color: Option<String>,
    size_px: Option<f64>,
    family: Option<String>,
}

fn attr_value(attrs: &str, name: &str) -> Option<String> {
    // new RegExp(`${n}\s*=\s*"([^"]*)"|${n}\s*=\s*'([^']*)'`, 'i')
    let lower = attrs.to_lowercase();
    let n = name.to_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(&n) {
        let pos = from + at;
        let rest = &attrs[pos + n.len()..];
        let t = rest.trim_start_matches(is_js_space);
        if let Some(eq) = t.strip_prefix('=') {
            let v = eq.trim_start_matches(is_js_space);
            for quote in ['"', '\''] {
                if let Some(body) = v.strip_prefix(quote) {
                    if let Some(end) = body.find(quote) {
                        return Some(body[..end].to_string());
                    }
                }
            }
        }
        from = pos + n.len().max(1);
        if from >= lower.len() {
            break;
        }
    }
    None
}

fn parse_int_prefix(v: &str) -> f64 {
    let t = v.trim_start_matches(is_js_space);
    let digits: String = t.chars().enumerate().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))).map(|(_, c)| c).collect();
    digits.parse::<f64>().unwrap_or(f64::NAN)
}

/// An HTML label as plain text plus formatted runs.
pub fn label_runs(value: &str) -> (String, Vec<Run>) {
    let mut text = String::new();
    let mut len16 = 0usize;
    let mut runs: Vec<Run> = Vec::new();
    let mut stack: Vec<RunStyle> = vec![RunStyle::default()];
    let emit = |chunk: &str, text: &mut String, len16: &mut usize, runs: &mut Vec<Run>, stack: &Vec<RunStyle>| {
        if chunk.is_empty() {
            return;
        }
        let st = stack.last().unwrap();
        let start = *len16;
        text.push_str(chunk);
        *len16 += chunk.encode_utf16().count();
        if st.bold || st.italic || st.underline || st.color.is_some() || st.size_px.is_some_and(|s| s != 0.0) || st.family.as_deref().is_some_and(|f| !f.is_empty()) {
            runs.push(Run { start, end: *len16, bold: st.bold, italic: st.italic, underline: st.underline, color: st.color.clone(), size_px: st.size_px, family: st.family.clone() });
        }
    };
    let src = value;
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            let end = src[i..].find('<').map_or(src.len(), |e| i + e);
            emit(&decode_entities(&src[i..end]), &mut text, &mut len16, &mut runs, &stack);
            i = end;
            continue;
        }
        // /<(\/?)([a-z0-9]+)((?:\s+[^>]*?)?)\s*(\/?)>/i
        let rest = &src[i + 1..];
        let (closing, after_slash) = match rest.strip_prefix('/') {
            Some(r) => (true, r),
            None => (false, rest),
        };
        let name_len = after_slash.bytes().take_while(u8::is_ascii_alphanumeric).count();
        let tag_match = (name_len > 0).then(|| {
            let tail = &after_slash[name_len..];
            let gt = tail.find('>')?;
            let r = &tail[..gt];
            let all_ws_slash = r.trim_matches(is_js_space).is_empty() || r.trim_end_matches(is_js_space) == "/" || (r.trim_matches(is_js_space) == "/");
            if !(r.starts_with(is_js_space) || all_ws_slash) {
                return None;
            }
            let self_closing = r.trim_end_matches(is_js_space).ends_with('/');
            let mut attrs = r;
            if self_closing {
                attrs = attrs.trim_end_matches(is_js_space);
                attrs = &attrs[..attrs.len() - 1];
            }
            attrs = attrs.trim_end_matches(is_js_space);
            Some((after_slash[..name_len].to_lowercase(), attrs.to_string(), self_closing, name_len + gt + 1))
        });
        let Some(Some((tag, attrs, self_closing, consumed))) = tag_match else {
            emit("<", &mut text, &mut len16, &mut runs, &stack);
            i += 1;
            continue;
        };
        i += 1 + usize::from(closing) + consumed;
        if tag == "br" {
            emit("\n", &mut text, &mut len16, &mut runs, &stack);
            continue;
        }
        if ["div", "p", "li", "h1", "h2", "h3", "h4", "h5", "h6"].contains(&tag.as_str()) {
            if closing {
                emit("\n", &mut text, &mut len16, &mut runs, &stack);
            }
            continue;
        }
        if closing {
            if stack.len() > 1 {
                stack.pop();
            }
            continue;
        }
        if self_closing {
            continue;
        }
        let mut next = stack.last().unwrap().clone();
        match tag.as_str() {
            "b" | "strong" => next.bold = true,
            "i" | "em" => next.italic = true,
            "u" => next.underline = true,
            "font" | "span" => {
                if let Some(c) = colour(attr_value(&attrs, "color").as_deref()) {
                    next.color = Some(c);
                }
                if let Some(sz) = attr_value(&attrs, "size") {
                    let px = match sz.as_str() {
                        "1" => Some(10.0),
                        "2" => Some(13.0),
                        "3" => Some(16.0),
                        "4" => Some(18.0),
                        "5" => Some(24.0),
                        "6" => Some(32.0),
                        "7" => Some(48.0),
                        _ => None,
                    };
                    if px.is_some() {
                        next.size_px = px;
                    }
                }
                if let Some(face) = attr_value(&attrs, "face").filter(|f| !f.is_empty()) {
                    next.family = Some(face.split(',').next().unwrap_or("").trim_matches(is_js_space).to_string());
                }
                let css = attr_value(&attrs, "style").unwrap_or_default();
                for decl in css.split(';') {
                    let mut parts = decl.split(':').map(|q| q.trim_matches(is_js_space).to_lowercase());
                    let (k, v) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
                    match k.as_str() {
                        "color" => {
                            if let Some(cc) = colour(Some(&v)) {
                                next.color = Some(cc);
                            }
                        }
                        "font-size" => {
                            let n = js_parse_float(&v);
                            if n > 0.0 {
                                next.size_px = Some(if v.ends_with("pt") { n / 0.75 } else { n });
                            }
                        }
                        "font-weight" if v == "bold" || parse_int_prefix(&v) >= 600.0 => next.bold = true,
                        "font-style" if v == "italic" => next.italic = true,
                        "text-decoration" if v.contains("underline") => next.underline = true,
                        "font-family" if !v.is_empty() => next.family = Some(v.split(',').next().unwrap_or("").replace(['\'', '"'], "").trim_matches(is_js_space).to_string()),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        stack.push(next);
    }
    // text.replace(/\n+$/, '')
    let trimmed = text.trim_end_matches('\n').to_string();
    let total = trimmed.encode_utf16().count();
    let runs = runs.into_iter().filter(|r| r.start < total).map(|mut r| {
        r.end = r.end.min(total);
        r
    }).collect();
    (trimmed, runs)
}

fn decode_entities(t: &str) -> String {
    // /&(#x[0-9a-f]+|#\d+|[a-z]+);/gi with the entity table of the web code
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let found = after.find(';').and_then(|e| {
            let body = &after[..e];
            let ch: Option<String> = if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
                (!hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(String::from)).flatten()
            } else if let Some(dec) = body.strip_prefix('#') {
                (!dec.is_empty() && dec.chars().all(|c| c.is_ascii_digit())).then(|| dec.parse::<u32>().ok().and_then(char::from_u32).map(String::from)).flatten()
            } else if !body.is_empty() && body.chars().all(|c| c.is_ascii_alphabetic()) {
                match body.to_lowercase().as_str() {
                    "nbsp" => Some(" ".to_string()),
                    "lt" => Some("<".to_string()),
                    "gt" => Some(">".to_string()),
                    "quot" => Some("\"".to_string()),
                    "apos" => Some("'".to_string()),
                    "amp" => Some("&".to_string()),
                    _ => None,
                }
            } else {
                None
            };
            ch.map(|c| (c, e))
        });
        match found {
            Some((c, e)) => {
                out.push_str(&c);
                rest = &after[e + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text of a label: HTML (html=1) loses its tags.
fn label_text(value: &str, html: bool) -> String {
    if html { label_runs(value).0 } else { value.to_string() }
}

// ---- reading the file -------------------------------------------------------------------------------------------------------------------

/// `decodeURIComponent` (None for a malformed sequence, as the URIError of JavaScript).
fn decode_uri_component(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The `mxGraphModel` of each diagram: (name, model node). Compressed diagrams are inflated here.
pub fn read_diagrams(input: &[u8]) -> Result<Vec<(String, Node)>, String> {
    let source = String::from_utf8_lossy(input).to_string();
    let has_root = {
        let mut found = false;
        for key in ["<mxfile", "<mxGraphModel"] {
            let mut from = 0;
            while let Some(at) = source[from..].find(key) {
                let after = source[from + at + key.len()..].chars().next();
                if after.is_some_and(|c| is_js_space(c) || c == '>') {
                    found = true;
                }
                from += at + key.len();
            }
        }
        found
    };
    if !has_root {
        return Err("This is not a draw.io file (no mxfile or mxGraphModel)".to_string());
    }
    let tree = parse_xml_tree(&source);
    let top = tree.elements().find(|c| c.tag == "mxfile" || c.tag == "mxGraphModel").ok_or("This is not a draw.io file (the XML cannot be read)")?;
    if top.tag == "mxGraphModel" {
        return Ok(vec![("Page-1".to_string(), top.clone())]);
    }
    let mut out: Vec<(String, Node)> = Vec::new();
    for d in child(top, "diagram") {
        let model = match first(d, "mxGraphModel") {
            Some(m) => m.clone(),
            None => {
                let packed: String = d.children.iter().filter_map(|c| if let Child::Text(t) = c { Some(t.as_str()) } else { None }).collect();
                let packed = packed.trim_matches(is_js_space);
                if packed.is_empty() {
                    continue;
                }
                let name = d.attrs.get("name").cloned().unwrap_or_default();
                let unreadable = || format!("The diagram \"{name}\" is compressed in a way that cannot be read");
                let bytes = atob(packed).ok_or_else(unreadable)?;
                let mut inflated = Vec::new();
                flate2::read::DeflateDecoder::new(&bytes[..]).read_to_end(&mut inflated).map_err(|_| unreadable())?;
                let xml = decode_uri_component(&String::from_utf8_lossy(&inflated)).ok_or_else(unreadable)?;
                let parsed = parse_xml_tree(&xml);
                let found = parsed.elements().find(|c| c.tag == "mxGraphModel").cloned();
                found.ok_or_else(|| format!("The diagram \"{name}\" has no model"))?
            }
        };
        let name = d.attrs.get("name").filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("Page-{}", out.len() + 1));
        out.push((name, model));
    }
    if out.is_empty() {
        return Err("This draw.io file has no diagram".to_string());
    }
    Ok(out)
}

// ---- building the drawing ---------------------------------------------------------------------------------------------------------------

fn style_json(m: &Map<String, Value>) -> String {
    Value::Object(m.clone()).to_string()
}

fn picture_of(spec: Option<&str>, skip: &mut dyn FnMut(&str)) -> Option<Vec<u8>> {
    let spec = spec.filter(|s| !s.is_empty())?;
    // /^data:image\/(png|jpe?g|gif|bmp),(.+)$/is
    let lower = spec.to_ascii_lowercase();
    let ok = lower.strip_prefix("data:image/").and_then(|r| ["png", "jpeg", "jpg", "gif", "bmp"].iter().find(|k| r.starts_with(*k) && r[k.len()..].starts_with(',')).map(|k| k.len()));
    let Some(kind_len) = ok else {
        skip(if lower.starts_with("http:") || lower.starts_with("https:") { "image from a web address" } else { "image of an unsupported kind" });
        return None;
    };
    let data = &spec["data:image/".len() + kind_len + 1..];
    if data.is_empty() {
        skip("image of an unsupported kind");
        return None;
    }
    match atob(data) {
        Some(b) => Some(b),
        None => {
            skip("image that cannot be read");
            None
        }
    }
}

struct Ctx<'a> {
    engine: &'a mut Engine,
    pi: usize,
    skipped: &'a mut BTreeMap<String, usize>,
}

impl Ctx<'_> {
    fn skip(&mut self, what: &str) {
        *self.skipped.entry(what.to_string()).or_insert(0) += 1;
    }

    fn style_shapes(&mut self, ids: &[&str], style: &Map<String, Value>) -> Result<(), String> {
        self.engine.edit(self.pi, 10, &format!("{}\n{}", ids.join("\n"), style_json(style)), 0.0, 0.0, 0.0, 0.0, 0)
    }

    fn format_text(&mut self, id: &str, spec: &Value) -> Result<(), String> {
        self.engine.edit(self.pi, 13, &format!("{id}\n{spec}"), 0.0, 0.0, 0.0, 0.0, 0)
    }

    fn set_text(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.engine.edit(self.pi, 8, &format!("{id}\n{text}"), 0.0, 0.0, 0.0, 0.0, 0)
    }
}

fn font_style(st: &HashMap<String, String>) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("fontPt".into(), json!(js_round(number_of(st.get("fontSize").map(String::as_str), 12.0) * 0.75 * 10.0) / 10.0));
    m.insert("textColor".into(), json!(colour(st.get("fontColor").map(String::as_str)).unwrap_or_else(|| "#000000".to_string())));
    let bits = number_of(st.get("fontStyle").map(String::as_str), 0.0) as i64;
    if bits & 1 == 1 {
        m.insert("bold".into(), json!(true));
    }
    if bits & 2 == 2 {
        m.insert("italic".into(), json!(true));
    }
    m
}

/// `paint(id, st, {vertex})`: the style of a vertex / edge.
fn paint(st: &HashMap<String, String>, vertex: bool) -> Map<String, Value> {
    let mut s = Map::new();
    let text = st.get("name").map(String::as_str) == Some("text") || st.get("shape").map(String::as_str) == Some("text");
    let stroke = if st.get("strokeColor").map(String::as_str) == Some("none") { "none".to_string() } else { colour(st.get("strokeColor").map(String::as_str)).unwrap_or_else(|| if text && vertex { "none".into() } else { "#000000".into() }) };
    let fill = if st.get("fillColor").map(String::as_str) == Some("none") { "none".to_string() } else { colour(st.get("fillColor").map(String::as_str)).unwrap_or_else(|| if text || !vertex { "none".into() } else { "#ffffff".into() }) };
    s.insert("line".into(), json!(stroke));
    s.insert("fill".into(), json!(fill));
    if st.get("strokeWidth").is_some_and(|w| !w.is_empty()) {
        s.insert("weight".into(), json!(number_of(st.get("strokeWidth").map(String::as_str), 1.0)));
    }
    if st.get("dashed").map(String::as_str) == Some("1") {
        s.insert("pattern".into(), json!(2));
    }
    let op = number_of(st.get("opacity").map(String::as_str), 100.0);
    if op < 100.0 {
        let t = js_round((1.0 - op / 100.0) * 100.0) / 100.0;
        s.insert("lineTrans".into(), json!(t));
        if fill != "none" {
            s.insert("fillTrans".into(), json!(t));
        }
    }
    let gc = colour(st.get("gradientColor").map(String::as_str));
    if let Some(gc) = gc {
        if fill != "none" && colour(st.get("fillColor").map(String::as_str)).is_some() {
            s.insert("gradientStops".into(), json!([[0, fill, 0], [1, gc, 0]]));
            let angle = match st.get("gradientDirection").map(String::as_str).filter(|d| !d.is_empty()).unwrap_or("south") {
                "south" => 90,
                "north" => 270,
                "east" => 0,
                "west" => 180,
                _ => 90,
            };
            s.insert("gradientAngle".into(), json!(angle));
        }
    }
    if st.get("shadow").map(String::as_str) == Some("1") {
        s.insert("shadow".into(), json!(true));
    }
    s
}

fn is_array_index(key: &str) -> Option<u64> {
    let ok = key == "0" || (!key.is_empty() && !key.starts_with('0') && key.chars().all(|c| c.is_ascii_digit()));
    if !ok || key.len() > 10 {
        return None;
    }
    key.parse::<u64>().ok().filter(|n| *n < 4_294_967_295)
}

/// JavaScript object entry order: array-index keys ascending, then the others in insertion order.
fn entries_in_js_order<T: Clone>(items: &[(String, T)]) -> Vec<(String, T)> {
    let mut v: Vec<(String, T)> = items.to_vec();
    v.sort_by_key(|(k, _)| is_array_index(k).map_or((1u8, 0u64), |n| (0, n)));
    v
}

fn set_entry<T>(items: &mut Vec<(String, T)>, key: &str, value: T) {
    if let Some(slot) = items.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value;
    } else {
        items.push((key.to_string(), value));
    }
}

fn geo(c: &Node) -> Option<&Node> {
    first(c, "mxGeometry")
}

fn attr<'a>(n: &'a Node, k: &str) -> Option<&'a str> {
    n.attrs.get(k).map(String::as_str)
}

/// Build the drawing in `engine` (a new document; one page per diagram).
pub fn import_drawio(engine: &mut Engine, input: &[u8], margin: f64) -> Result<DrawioSummary, String> {
    let diagrams = read_diagrams(input)?;
    let mut approximated: BTreeMap<String, usize> = BTreeMap::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut maps: Vec<BTreeMap<String, String>> = Vec::new();
    let (mut shapes, mut connectors) = (0usize, 0usize);
    for (pi, (name, model)) in diagrams.iter().enumerate() {
        let root = first(model, "root");
        let mut cells: Vec<Node> = Vec::new();
        if let Some(root) = root {
            cells.extend(child(root, "mxCell").into_iter().cloned());
            for o in child(root, "object") {
                if let Some(c) = first(o, "mxCell") {
                    let mut c = c.clone();
                    if let Some(id) = o.attrs.get("id") {
                        c.attrs.insert("id".into(), id.clone());
                    } else {
                        c.attrs.remove("id");
                    }
                    let value = o.attrs.get("label").or_else(|| o.attrs.get("value")).or_else(|| c.attrs.get("value")).cloned();
                    match value {
                        Some(v) => {
                            c.attrs.insert("value".into(), v);
                        }
                        None => {
                            c.attrs.remove("value");
                        }
                    }
                    cells.push(c);
                }
            }
        }
        let key_of = |c: &Node| attr(c, "id").unwrap_or("undefined").to_string();
        let mut by_id: HashMap<String, usize> = HashMap::new();
        for (i, c) in cells.iter().enumerate() {
            by_id.insert(key_of(c), i);
        }
        let get = |id: Option<&str>| -> Option<&Node> { id.and_then(|id| by_id.get(id)).map(|i| &cells[*i]) };
        let layer_cells: Vec<&Node> = cells.iter().filter(|c| attr(c, "parent") == Some("0") && attr(c, "vertex") != Some("1") && attr(c, "edge") != Some("1") && attr(c, "id") != Some("0")).collect();
        let layer_of = |c: &Node| -> Option<String> {
            let mut k = Some(c);
            for _ in 0..50 {
                let n = k?;
                if attr(n, "parent") == Some("0") {
                    return Some(attr(n, "id").unwrap_or("undefined").to_string());
                }
                k = get(attr(n, "parent"));
            }
            None
        };
        let hidden = |c: &Node| -> bool {
            let mut k = Some(c);
            let mut n = 0;
            while let Some(node) = k {
                if n >= 50 {
                    break;
                }
                if attr(node, "visible") == Some("0") && (layer_cells.len() < 2 || attr(node, "parent") != Some("0")) {
                    return true;
                }
                k = get(attr(node, "parent"));
                n += 1;
            }
            false
        };
        let origin = |c: &Node| -> [f64; 2] {
            let (mut x, mut y) = (0.0, 0.0);
            let mut k = Some(c);
            let mut n = 0;
            while let Some(node) = k {
                if n >= 50 {
                    break;
                }
                let g = if attr(node, "vertex") == Some("1") { geo(node) } else { None };
                if let Some(g) = g {
                    x += number_of(attr(g, "x"), 0.0);
                    y += number_of(attr(g, "y"), 0.0);
                }
                k = get(attr(node, "parent"));
                n += 1;
            }
            [x, y]
        };
        let vertices: Vec<&Node> = cells.iter().filter(|c| attr(c, "vertex") == Some("1") && geo(c).is_some() && !hidden(c)).collect();
        let edges: Vec<&Node> = cells.iter().filter(|c| attr(c, "edge") == Some("1") && !hidden(c)).collect();
        let box_of = |c: &Node| -> (f64, f64, f64, f64) {
            let [x, y] = origin(c);
            let g = geo(c).unwrap();
            (x, y, number_of(attr(g, "width"), 0.0), number_of(attr(g, "height"), 0.0))
        };
        let centre = |c: &Node| -> [f64; 2] {
            let b = box_of(c);
            [b.0 + b.2 / 2.0, b.1 + b.3 / 2.0]
        };
        let point_of = |g: &Node, role: &str| -> Option<[f64; 2]> { g.elements().find(|n| n.tag == "mxPoint" && attr(n, "as") == Some(role)).map(|p| [number_of(attr(p, "x"), 0.0), number_of(attr(p, "y"), 0.0)]) };
        let edge_points = |e: &Node| -> Vec<[f64; 2]> {
            let empty = Node::default();
            let g = geo(e).unwrap_or(&empty);
            let (s, t) = (get(attr(e, "source")), get(attr(e, "target")));
            let mut pts: Vec<Option<[f64; 2]>> = vec![point_of(g, "sourcePoint").or_else(|| s.filter(|s| geo(s).is_some()).map(&centre))];
            if let Some(arr) = g.elements().find(|n| n.tag == "Array" && attr(n, "as") == Some("points")) {
                for p in arr.elements().filter(|n| n.tag == "mxPoint") {
                    pts.push(Some([number_of(attr(p, "x"), 0.0), number_of(attr(p, "y"), 0.0)]));
                }
            }
            pts.push(point_of(g, "targetPoint").or_else(|| t.filter(|t| geo(t).is_some()).map(&centre)));
            pts.into_iter().flatten().collect()
        };
        // page size from every vertex box and edge point
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut add = |x: f64, y: f64| {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        };
        for v in &vertices {
            let b = box_of(v);
            add(b.0, b.1);
            add(b.0 + b.2, b.1 + b.3);
        }
        for e in &edges {
            for p in edge_points(e) {
                add(p[0], p[1]);
            }
        }
        if !x0.is_finite() {
            (x0, y0, x1, y1) = (0.0, 0.0, 0.0, 0.0);
        }
        let width = 2.0f64.max(js_round(((x1 - x0) / PX_PER_IN + 2.0 * margin) * 1000.0) / 1000.0);
        let height = 2.0f64.max(js_round(((y1 - y0) / PX_PER_IN + 2.0 * margin) * 1000.0) / 1000.0);
        if pi == 0 {
            engine.new_blank(width, height, false, name)?;
        } else {
            engine.page_op(1, pi - 1, 0, name)?;
            engine.page_op(6, pi, 0, &format!("{},{}", js_string(width), js_string(height)))?;
        }
        let xf = |x: f64| (x - x0) / PX_PER_IN + margin;
        let yf = |y: f64| height - ((y - y0) / PX_PER_IN + margin);
        let mut map: Vec<(String, String)> = Vec::new();
        let mut ctx = Ctx { engine, pi, skipped: &mut skipped };

        let write_label = |ctx: &mut Ctx, id: &str, st: &HashMap<String, String>, raw: &str| -> Result<(), String> {
            let html = st.get("html").map(String::as_str) == Some("1");
            let (text, runs) = if html { label_runs(raw) } else { (raw.to_string(), Vec::new()) };
            if text.is_empty() {
                return Ok(());
            }
            ctx.set_text(id, &text)?;
            ctx.style_shapes(&[id], &font_style(st))?;
            for r in &runs {
                let mut cells = Map::new();
                if let Some(c) = &r.color {
                    cells.insert("Color".into(), json!(c));
                }
                if let Some(px) = r.size_px.filter(|p| *p != 0.0) {
                    cells.insert("Size".into(), json!(js_string((px * 0.75) / 72.0)));
                }
                if let Some(f) = r.family.as_ref().filter(|f| !f.is_empty()) {
                    cells.insert("Font".into(), json!(f));
                }
                let mut spec = json!({"start": r.start, "end": r.end, "set": (r.bold as i32) | ((r.italic as i32) << 1) | ((r.underline as i32) << 2)});
                if !cells.is_empty() {
                    spec["cells"] = Value::Object(cells);
                }
                if ctx.format_text(id, &spec).is_err() {
                    ctx.skip("label run format");
                }
            }
            if let Some(al) = match st.get("align").map(String::as_str) {
                Some("left") => Some("0"),
                Some("center") => Some("1"),
                Some("right") => Some("2"),
                _ => None,
            } {
                let spec = json!({"start": 0, "end": text.encode_utf16().count(), "para": {"HorzAlign": al}});
                if ctx.format_text(id, &spec).is_err() {
                    ctx.skip("label alignment");
                }
            }
            Ok(())
        };

        for v in &vertices {
            let st = parse_style(attr(v, "style").unwrap_or(""));
            let b = box_of(v);
            let shape_name = st.get("shape").filter(|s| !s.is_empty()).or_else(|| st.get("name")).cloned().unwrap_or_default();
            let canon = canonical_name(&shape_name).to_string();
            let is_text = canon == "text";
            let img = if canon == "image" || st.get("image").is_some_and(|i| !i.is_empty()) {
                let mut sk = |w: &str| {
                    *ctx.skipped.entry(w.to_string()).or_insert(0) += 1;
                };
                picture_of(st.get("image").map(String::as_str), &mut sk)
            } else {
                None
            };
            let basic = match canon.as_str() {
                "ellipse" => Some(2),
                "rhombus" => Some(3),
                "triangle" => Some(4),
                _ => None,
            };
            let kind = if is_text { 5 } else { basic.unwrap_or(if st.get("rounded").map(String::as_str) == Some("1") || canon == "rounded" { 1 } else { 0 }) };
            let geo_shape = if is_text { None } else { shape_geometry(&shape_name, &st, b.2, b.3) };
            if geo_shape.is_none() && !is_text && img.is_none() && basic.is_none() && !canon.is_empty() && canon != "rect" && canon != "rounded" && canon != "whiteSpace" && canon != "html" {
                *approximated.entry(shape_name.clone()).or_insert(0) += 1;
            }
            let at = |pt: [f64; 2]| (xf(b.0 + pt[0]), yf(b.1 + pt[1]));
            let mut extras: Vec<String> = Vec::new();
            let mut p_style = paint(&st, true);
            let html = st.get("html").map(String::as_str) == Some("1");
            let raw = attr(v, "value").unwrap_or("");
            let label = label_text(raw, html);
            let mut id: Option<String> = None;
            if let Some(img) = &img {
                match ctx.engine.insert_picture(pi, img, xf(b.0), yf(b.1 + b.3), xf(b.0 + b.2), yf(b.1)) {
                    Ok(i) => id = Some(i),
                    Err(_) => ctx.skip("image that cannot be read"),
                }
            }
            let id = if let Some(id) = id {
                if !label.is_empty() {
                    write_label(&mut ctx, &id, &st, raw)?;
                }
                id
            } else if let (Some(_), "swimlane") = (&geo_shape, canon.as_str()) {
                let hs = 8.0f64.max(number_of(st.get("startSize").map(String::as_str), 23.0).min(b.3 * 0.9));
                let id = ctx.engine.draw(pi, 0, xf(b.0), yf(b.1 + b.3), xf(b.0 + b.2), yf(b.1))?;
                let mut body = p_style.clone();
                body.insert("fill".into(), json!(colour(st.get("swimlaneFillColor").map(String::as_str)).unwrap_or_else(|| "none".to_string())));
                ctx.style_shapes(&[&id], &body)?;
                let header = ctx.engine.draw(pi, 0, xf(b.0), yf(b.1 + hs), xf(b.0 + b.2), yf(b.1))?;
                ctx.style_shapes(&[&header], &p_style)?;
                write_label(&mut ctx, &header, &st, raw)?;
                extras.push(header);
                id
            } else if let Some(g) = &geo_shape {
                let id = if g.invisible_main { ctx.engine.draw(pi, 0, xf(b.0), yf(b.1 + b.3), xf(b.0 + b.2), yf(b.1))? } else { ctx.engine.draw_polyline(pi, &g.outline.iter().map(|p| at(*p)).collect::<Vec<_>>(), true)? };
                if g.invisible_main {
                    let mut s = p_style.clone();
                    s.insert("fill".into(), json!("none"));
                    s.insert("line".into(), json!("none"));
                    ctx.style_shapes(&[&id], &s)?;
                } else {
                    ctx.style_shapes(&[&id], &p_style)?;
                }
                for d in &g.details {
                    let mut dpts: Vec<(f64, f64)> = d.pts.iter().map(|p| at(*p)).collect();
                    if dpts.iter().all(|q| (q.0 - dpts[0].0).abs() < 1e-9) {
                        let last = dpts.len() - 1;
                        dpts[last].0 += 0.5 / PX_PER_IN;
                    }
                    let did = ctx.engine.draw_polyline(pi, &dpts, d.closed)?;
                    let mut s = p_style.clone();
                    if !d.closed {
                        s.insert("fill".into(), json!("none"));
                    }
                    s.remove("gradientStops");
                    ctx.style_shapes(&[&did], &s)?;
                    extras.push(did);
                }
                write_label(&mut ctx, &id, &st, raw)?;
                id
            } else {
                let id = ctx.engine.draw(pi, kind, xf(b.0), yf(b.1 + b.3), xf(b.0 + b.2), yf(b.1))?;
                ctx.style_shapes(&[&id], &p_style)?;
                write_label(&mut ctx, &id, &st, raw)?;
                id
            };
            p_style.clear();
            let top = if extras.is_empty() {
                id
            } else {
                let mut ids = vec![id];
                ids.extend(extras);
                ctx.engine.group_op(pi, 0, &ids.join("\n"))?
            };
            set_entry(&mut map, attr(v, "id").unwrap_or("undefined"), top.clone());
            shapes += 1;
            let rot = number_of(st.get("rotation").map(String::as_str), 0.0);
            if rot != 0.0 {
                let two_pi = 2.0 * std::f64::consts::PI;
                ctx.engine.edit(pi, 7, &top, (((-rot * std::f64::consts::PI) / 180.0) % two_pi + two_pi) % two_pi, 0.0, 0.0, 0.0, 0)?;
            }
        }

        for e in &edges {
            let st = parse_style(attr(e, "style").unwrap_or(""));
            let s_id = attr(e, "source").and_then(|k| map.iter().find(|(m, _)| m == k)).map(|(_, v)| v.clone());
            let t_id = attr(e, "target").and_then(|k| map.iter().find(|(m, _)| m == k)).map(|(_, v)| v.clone());
            let g = geo(e);
            let waypoints = g.is_some_and(|g| g.elements().any(|n| n.tag == "Array" && attr(n, "as") == Some("points") && !n.children.is_empty()));
            let curved = st.get("curved").map(String::as_str) == Some("1");
            let edge_style = st.get("edgeStyle").map(String::as_str).unwrap_or("");
            let orthogonal = edge_style.contains("orthogonal") || edge_style.contains("elbow") || edge_style.contains("entityRelation");
            let frac = |k: &str| st.get(k).map(|v| js_parse_float(v)).filter(|n| n.is_finite());
            let side_n = |ctx: &mut Ctx, shape: &str, fx: Option<f64>, fy: Option<f64>| -> usize {
                let (Some(fx), Some(fy)) = (fx, fy) else { return 0 };
                match ctx.engine.connection_points_json(pi, shape).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_array().map(Vec::len)) {
                    Some(n) if n >= 4 => side_of(fx, fy),
                    _ => 0,
                }
            };
            let (id, mut route_px): (String, Option<Vec<[f64; 2]>>);
            if let (Some(s), Some(t)) = (&s_id, &t_id) {
                if s != t && !waypoints {
                    let sn = side_n(&mut ctx, s, frac("exitX"), frac("exitY"));
                    let tn = side_n(&mut ctx, t, frac("entryX"), frac("entryY"));
                    id = ctx.engine.add_connector(pi, s, (sn > 0).then_some(sn), t, (tn > 0).then_some(tn))?;
                    connectors += 1;
                    route_px = None;
                    set_entry(&mut map, attr(e, "id").unwrap_or("undefined"), id.clone());
                    shapes += 1;
                    finish_edge(&mut ctx, &write_label, e, &st, &id, &mut route_px, g, (x0, y0, height, margin), &xf, &yf, &mut shapes)?;
                    continue;
                }
            }
            let mut pts_px = edge_points(e);
            if pts_px.len() < 2 {
                ctx.skip("edge without two ends");
                continue;
            }
            if orthogonal && !curved {
                pts_px = orthogonalize(&pts_px, exit_horizontal(frac("exitX"), frac("exitY")));
            }
            let pts: Vec<(f64, f64)> = pts_px.iter().map(|q| (xf(q[0]), yf(q[1]))).collect();
            if curved {
                let through: Vec<(f64, f64)> = curve_through(&pts.iter().map(|p| [p.0, p.1]).collect::<Vec<_>>()).iter().map(|p| (p[0], p[1])).collect();
                id = match ctx.engine.draw_curve(pi, &through) {
                    Ok(i) => i,
                    Err(_) => ctx.engine.draw_polyline(pi, &pts, false)?,
                };
            } else {
                id = ctx.engine.draw_polyline(pi, &pts, false)?;
            }
            route_px = Some(pts_px);
            set_entry(&mut map, attr(e, "id").unwrap_or("undefined"), id.clone());
            shapes += 1;
            finish_edge(&mut ctx, &write_label, e, &st, &id, &mut route_px, g, (x0, y0, height, margin), &xf, &yf, &mut shapes)?;
        }

        // the layers: one engine layer per layer cell, the shapes and edges on the layer of their cell, hidden / locked layers as such
        if layer_cells.len() >= 2 {
            let mut index: Vec<(String, usize)> = Vec::new();
            for (k, lc) in layer_cells.iter().enumerate() {
                let label = attr(lc, "value").filter(|v| !v.is_empty()).map_or_else(|| format!("Layer {}", k + 1), str::to_string);
                ctx.engine.page_op(7, pi, 0, &label)?;
                set_entry(&mut index, attr(lc, "id").unwrap_or("undefined"), k);
            }
            let mut by_layer: Vec<(String, Vec<String>)> = Vec::new();
            for (cell_id, shape_id) in entries_in_js_order(&map) {
                let Some(c) = get(Some(&cell_id)) else { continue };
                let Some(lid) = layer_of(c) else { continue };
                if !index.iter().any(|(k, _)| *k == lid) {
                    continue;
                }
                if let Some(slot) = by_layer.iter_mut().find(|(k, _)| *k == lid) {
                    slot.1.push(shape_id);
                } else {
                    by_layer.push((lid, vec![shape_id]));
                }
            }
            for (lid, list) in entries_in_js_order(&by_layer) {
                let layer = index.iter().find(|(k, _)| *k == lid).map_or(0, |(_, k)| *k);
                if ctx.engine.edit(pi, 16, &list.join("\n"), layer as f64, 0.0, 0.0, 0.0, 1).is_err() {
                    ctx.skip("layer membership");
                }
            }
            for lc in &layer_cells {
                let visible = if attr(lc, "visible") == Some("0") { "0" } else { "" };
                let locked = if parse_style(attr(lc, "style").unwrap_or("")).get("locked").map(String::as_str) == Some("1") { "1" } else { "" };
                if !visible.is_empty() || !locked.is_empty() {
                    let layer = index.iter().find(|(k, _)| Some(k.as_str()) == attr(lc, "id")).map_or(0, |(_, k)| *k);
                    if ctx.engine.page_op(8, pi, layer, &format!("|{visible}||{locked}")).is_err() {
                        ctx.skip("layer state");
                    }
                }
            }
        }
        maps.push(map.into_iter().collect());
    }
    Ok(DrawioSummary { pages: diagrams.len(), shapes, connectors, approximated, skipped, map: maps })
}

/// The part of the edge loop after the line / connector exists: style, arrowheads and the label.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn finish_edge(
    ctx: &mut Ctx,
    write_label: &dyn Fn(&mut Ctx, &str, &HashMap<String, String>, &str) -> Result<(), String>,
    e: &Node,
    st: &HashMap<String, String>,
    id: &str,
    route_px: &mut Option<Vec<[f64; 2]>>,
    g: Option<&Node>,
    (x0, y0, height, margin): (f64, f64, f64, f64),
    xf: &dyn Fn(f64) -> f64,
    yf: &dyn Fn(f64) -> f64,
    shapes: &mut usize,
) -> Result<(), String> {
    let pi = ctx.pi;
    let mut style = paint(st, false);
    style.insert("fill".into(), json!("none"));
    let end_name = st.get("endArrow").map(String::as_str).or(Some("classic"));
    let mut end = arrow_index(end_name, st.get("endFill").map(String::as_str) != Some("0"));
    let mut begin = arrow_index(st.get("startArrow").map(String::as_str), st.get("startFill").map(String::as_str) != Some("0"));
    if end.is_err() {
        end = Ok(Some(4));
        ctx.skip("arrow type");
    }
    if begin.is_err() {
        begin = Ok(Some(4));
        ctx.skip("arrow type");
    }
    if let Ok(Some(n)) = end {
        if n != 0 {
            style.insert("endArrow".into(), json!(n));
            style.insert("endArrowSize".into(), json!(arrow_size(js_parse_float(st.get("endSize").map_or("6", String::as_str)))));
        }
    }
    if let Ok(Some(n)) = begin {
        if n != 0 {
            style.insert("beginArrow".into(), json!(n));
            style.insert("beginArrowSize".into(), json!(arrow_size(js_parse_float(st.get("startSize").map_or("6", String::as_str)))));
        }
    }
    ctx.style_shapes(&[id], &style)?;
    // the label: at the middle of the edge, or (mxGeometry x / y / offset) at its own place as a text box
    let label_value = attr(e, "value").unwrap_or("");
    let gx = g.map_or(0.0, |g| number_of(attr(g, "x"), 0.0));
    let gy = g.map_or(0.0, |g| number_of(attr(g, "y"), 0.0));
    let off_point = g.and_then(|g| g.elements().find(|n| n.tag == "mxPoint" && attr(n, "as") == Some("offset")));
    let off = off_point.map_or([0.0, 0.0], |p| [number_of(attr(p, "x"), 0.0), number_of(attr(p, "y"), 0.0)]);
    let html = st.get("html").map(String::as_str) == Some("1");
    let text = label_text(label_value, html);
    if !text.is_empty() && (gx != 0.0 || gy != 0.0 || off[0] != 0.0 || off[1] != 0.0) {
        let attempt = (|| -> Result<(), String> {
            if route_px.is_none() {
                let connectors: Value = serde_json::from_str(&ctx.engine.connectors_json(pi)?).map_err(|e| e.to_string())?;
                let found = connectors.as_array().and_then(|a| a.iter().find(|q| q["id"].as_str() == Some(id)));
                let route: Vec<[f64; 2]> = found.and_then(|c| c["route"].as_array()).map_or_else(Vec::new, |r| r.iter().map(|p| [p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)]).collect());
                *route_px = Some(if route.is_empty() { vec![[x0, y0], [x0 + 1.0, y0]] } else { route.iter().map(|[rx, ry]| [(rx - margin) * PX_PER_IN + x0, (height - ry - margin) * PX_PER_IN + y0]).collect() });
            }
            let [lx, ly] = label_position(route_px.as_ref().unwrap(), gx, gy, off);
            let fs = number_of(st.get("fontSize").map(String::as_str), 12.0);
            let lines: Vec<&str> = text.split('\n').collect();
            let longest = lines.iter().map(|l| l.encode_utf16().count()).max().unwrap_or(0) as f64;
            let lw = longest * fs * 0.6 + 8.0;
            let lh = lines.len() as f64 * fs * 1.25 + 4.0;
            let box_id = ctx.engine.draw(pi, 5, xf(lx - lw / 2.0), yf(ly + lh / 2.0), xf(lx + lw / 2.0), yf(ly - lh / 2.0))?;
            let mut s = Map::new();
            s.insert("fill".into(), json!("none"));
            s.insert("line".into(), json!("none"));
            ctx.style_shapes(&[&box_id], &s)?;
            write_label(ctx, &box_id, st, label_value)?;
            *shapes += 1;
            Ok(())
        })();
        if attempt.is_err() {
            ctx.skip("edge label");
        }
    } else if write_label(ctx, id, st, label_value).is_err() {
        ctx.skip("edge label");
    }
    Ok(())
}

impl Engine {
    /// File > Open of a draw.io file (the bytes of a .drawio file): a new drawing replaces the open one.
    pub fn import_drawio(&mut self, bytes: &[u8]) -> Result<DrawioSummary, String> {
        import_drawio(self, bytes, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lenient_reader_keeps_text_and_decodes_entities() {
        let tree = parse_xml_tree("<?xml version=\"1.0\"?><a x=\"1&amp;2\"><!-- c --><b/>text &lt;<![CDATA[<raw>]]></a>");
        let a = tree.elements().next().unwrap();
        assert_eq!(a.attr("x"), Some("1&2"));
        assert_eq!(a.elements().count(), 1);
        assert_eq!(a.own_text(), "text <<raw>");
    }

    #[test]
    fn styles_labels_and_runs() {
        let st = parse_style("ellipse;whiteSpace=wrap;fillColor=#FF0000;");
        assert_eq!(st.get("name").map(String::as_str), Some("ellipse"));
        assert_eq!(st.get("fillColor").map(String::as_str), Some("#FF0000"));
        let (text, runs) = label_runs("a<b>bold</b><br>x &amp; <font color=\"#ABC\" size=\"3\">y</font><div>z</div>");
        assert_eq!(text, "abold\nx & yz");
        assert_eq!(runs.len(), 2);
        assert!(runs[0].bold && runs[0].start == 1 && runs[0].end == 5);
        assert_eq!(runs[1].color.as_deref(), Some("#aabbcc"));
        assert_eq!(runs[1].size_px, Some(16.0));
        assert_eq!(label_text("plain <b>", false), "plain <b>");
    }

    #[test]
    fn a_compressed_diagram_is_inflated_and_plain_files_are_read() {
        // raw deflate of "%3CmxGraphModel%2F%3E" made with zlib.deflateRawSync, base64
        let plain = "<mxfile><diagram name=\"P\"><mxGraphModel><root/></mxGraphModel></diagram></mxfile>";
        let d = read_diagrams(plain.as_bytes()).unwrap();
        assert_eq!(d[0].0, "P");
        assert!(read_diagrams(b"nothing").unwrap_err().contains("not a draw.io file"));
        assert!(read_diagrams(b"<mxfile></mxfile>").unwrap_err().contains("no diagram"));
        assert_eq!(decode_uri_component("%3Ca%3E%20"), Some("<a> ".to_string()));
        assert_eq!(decode_uri_component("%zz"), None);
    }
}
