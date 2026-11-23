//! Process > Check Diagram: a Rust port of `web/checkdiagram.js` (docs/34, step N4.1). The same plain checks on the drawing model, with the same
//! issue texts and the same order; `tests/check_oracle.rs` compares the result with what the web code finds on 145 example drawings
//! (`visio-test-harness/dump-check-oracle.mjs`). The model is the JSON of `Engine::json()`.
use std::collections::HashMap;

use serde_json::Value;

const PX: f64 = 96.0;

/// One problem: `kind` is the rule, `id` the shape (or connector) to select, `fix` the repair the web offers ("Delete connector", "Move into page").
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub kind: String,
    pub id: String,
    pub message: String,
    pub fix: Option<String>,
    /// The page index when the issue comes from a check of every page.
    pub page: Option<usize>,
}

fn issue(kind: &str, id: &str, message: String, fix: Option<&str>) -> Issue {
    Issue { kind: kind.to_string(), id: id.to_string(), message, fix: fix.map(str::to_string), page: None }
}

/// JavaScript truthiness of a string field: present and not empty.
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// JavaScript truthiness of any field (the model's `group` is an object, not a string).
fn truthy(value: &Value, key: &str) -> bool {
    match value.get(key) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// JavaScript's `String.prototype.trim` (White_Space plus U+FEFF, without U+0085).
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

fn shape_id_of(end: Option<&Value>) -> Option<String> {
    let id = end?.get("shapeId")?;
    let raw = match id {
        Value::String(s) if !s.is_empty() => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    Some(raw.replacen("shape:visio:", "", 1))
}

fn visio_id(value: &Value) -> String {
    match value.get("visioId") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => "undefined".to_string(),
    }
}

/// `"first line"` (at most 30 UTF-16 units, as JavaScript's `slice`) or `shape <id>`.
fn name(shape: &Value) -> String {
    match text(shape, "label") {
        Some(label) => {
            let first = label.split('\n').next().unwrap_or("");
            let units: Vec<u16> = first.encode_utf16().take(30).collect();
            format!("\"{}\"", String::from_utf16_lossy(&units))
        }
        None => format!("shape {}", visio_id(shape)),
    }
}

fn is_filled(shape: &Value) -> bool {
    text(shape, "fill").is_some_and(|fill| fill != "none")
}

/// The problems of page `page_index` of `model`. `size` is the page size in inches (the "outside the page" rule is skipped without it);
/// `alt_text` also lists shapes without text and without alt text. Sorted by kind, like the web.
pub fn check_page(model: &Value, page_index: usize, size: Option<(f64, f64)>, alt_text: bool) -> Vec<Issue> {
    let Some(page) = model.get("pages").and_then(Value::as_array).and_then(|pages| pages.get(page_index)) else {
        return Vec::new();
    };
    let all_shapes: &[Value] = page.get("shapes").and_then(Value::as_array).map_or(&[], Vec::as_slice);
    let shapes: Vec<&Value> = all_shapes.iter().filter(|s| !truthy(s, "group") && number(s, "w") > 0.0 && number(s, "h") > 0.0).collect();
    let connectors: &[Value] = page.get("connectors").and_then(Value::as_array).map_or(&[], Vec::as_slice);
    let mut issues: Vec<Issue> = Vec::new();

    for c in connectors {
        let mut ends = Vec::new();
        if shape_id_of(c.get("from")).is_none() {
            ends.push("start");
        }
        if shape_id_of(c.get("to")).is_none() {
            ends.push("end");
        }
        if !ends.is_empty() {
            let id = visio_id(c);
            issues.push(issue("loose end", &id, format!("Connector {id} has a loose {} (not glued to a shape)", ends.join(" and ")), Some("Delete connector")));
        }
    }

    // two connectors that join the same two shapes in the same direction: the later one is reported
    let by_id: HashMap<String, &Value> = shapes.iter().map(|s| (visio_id(s), *s)).collect();
    let mut first_of: HashMap<String, &Value> = HashMap::new();
    for c in connectors {
        let (Some(from), Some(to)) = (shape_id_of(c.get("from")), shape_id_of(c.get("to"))) else {
            continue;
        };
        let key = format!("{from}>{to}");
        match first_of.get(&key) {
            None => {
                first_of.insert(key, c);
            }
            Some(first) => {
                let nm = |id: &str| by_id.get(id).map_or_else(|| format!("shape {id}"), |s| name(s));
                let id = visio_id(c);
                issues.push(issue("duplicate connector", &id, format!("Connector {id} joins {} to {} like connector {} does", nm(&from), nm(&to), visio_id(first)), Some("Delete connector")));
            }
        }
    }

    if !connectors.is_empty() {
        let mut used = std::collections::HashSet::new();
        for c in connectors {
            for end in [c.get("from"), c.get("to")] {
                if let Some(id) = shape_id_of(end) {
                    used.insert(id);
                }
            }
        }
        for s in &shapes {
            let filled = is_filled(s);
            let is_image = s.get("type").and_then(Value::as_str) == Some("image");
            if is_image || !(text(s, "label").is_some() || filled) || used.contains(&visio_id(s)) {
                continue;
            }
            // a text box (no fill, no line) is a caption, not a node of the diagram
            if !filled && (text(s, "stroke").is_none() || s.get("strokeWidth").and_then(Value::as_f64) == Some(0.0)) {
                continue;
            }
            issues.push(issue("not connected", &visio_id(s), format!("{} is not connected to anything", name(s)), None));
        }
    }

    let top: Vec<&Value> = shapes.iter().copied().filter(|s| is_filled(s) && s.get("type").and_then(Value::as_str) != Some("image")).collect();
    for i in 0..top.len() {
        for j in (i + 1)..top.len() {
            let (a, b) = (top[i], top[j]);
            let (ax, ay, aw, ah) = (number(a, "x"), number(a, "y"), number(a, "w"), number(a, "h"));
            let (bx, by, bw, bh) = (number(b, "x"), number(b, "y"), number(b, "w"), number(b, "h"));
            let w = (ax + aw).min(bx + bw) - ax.max(bx);
            let h = (ay + ah).min(by + bh) - ay.max(by);
            if !(w > 0.0 && h > 0.0) {
                continue;
            }
            let inside = |px: f64, py: f64, pw: f64, ph: f64, qx: f64, qy: f64, qw: f64, qh: f64| px >= qx - 0.5 && py >= qy - 0.5 && px + pw <= qx + qw + 0.5 && py + ph <= qy + qh + 0.5;
            if inside(ax, ay, aw, ah, bx, by, bw, bh) || inside(bx, by, bw, bh, ax, ay, aw, ah) {
                continue; // a shape on a container is not an overlap
            }
            let smaller = (aw * ah).min(bw * bh);
            if (w * h) / smaller >= 0.25 {
                issues.push(issue("overlap", &visio_id(a), format!("{} and {} overlap", name(a), name(b)), None));
            }
        }
    }

    if let Some((width, height)) = size {
        let (page_w, page_h) = (width * PX, height * PX);
        for s in &shapes {
            let (x, y, w, h) = (number(s, "x"), number(s, "y"), number(s, "w"), number(s, "h"));
            if x < -0.5 || y < -0.5 || x + w > page_w + 0.5 || y + h > page_h + 0.5 {
                let fix = (w <= page_w + 0.5 && h <= page_h + 0.5).then_some("Move into page");
                issues.push(issue("outside the page", &visio_id(s), format!("{} lies partly outside the page", name(s)), fix));
            }
        }
    }

    // accessibility (off unless asked for): no text of its own and no alt text; connectors are not listed
    if alt_text {
        for s in &shapes {
            let has_label = text(s, "label").is_some_and(|label| !js_trim(label).is_empty());
            let has_alt = text(s, "altText").is_some_and(|alt| !js_trim(alt).is_empty());
            if !has_label && !has_alt {
                let what = if s.get("type").and_then(Value::as_str) == Some("image") { "The picture" } else { "Shape" };
                issues.push(issue("no alt text", &visio_id(s), format!("{what} {} has neither text nor alt text (Shape data > Alt text)", visio_id(s)), None));
            }
        }
    }

    let order = ["loose end", "duplicate connector", "not connected", "overlap", "outside the page", "no alt text"];
    issues.sort_by_key(|i| order.iter().position(|k| *k == i.kind).map_or(-1, |at| at as i64));
    issues
}

/// The problems as CSV text (a report to keep or send): columns Page, Problem, Shape, Message, Fix. `names` are the page names. A field
/// with a comma, a quote or a line break is quoted, quotes doubled; lines end with CRLF (what spreadsheets expect).
pub fn issues_to_csv(issues: &[Issue], names: &[String]) -> String {
    fn field(value: &str) -> String {
        if value.contains(['"', ',', '\r', '\n']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    }
    let mut rows: Vec<[String; 5]> = vec![["Page", "Problem", "Shape", "Message", "Fix"].map(str::to_string)];
    for i in issues {
        let page_name = match i.page {
            Some(page) => names.get(page).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("Page {}", page + 1)),
            None => String::new(),
        };
        // a message that starts with "<page name>: " (every-page check) loses that prefix: the page is its own column
        // (exactly the web's `replace(/^[^:]*: /, ...)`: the text up to the first colon plus ": " must be the page name plus ": ")
        let mut message = i.message.clone();
        if i.page.is_some() {
            if let Some(colon) = message.find(':') {
                if message[colon + 1..].starts_with(' ') && message[..colon + 2] == format!("{page_name}: ") {
                    message = message[colon + 2..].to_string();
                }
            }
        }
        rows.push([page_name, i.kind.clone(), i.id.clone(), message, i.fix.clone().unwrap_or_default()]);
    }
    rows.iter().map(|r| r.iter().map(|f| field(f)).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rect(id: &str, x: f64, y: f64, w: f64, h: f64, fill: &str, label: &str) -> Value {
        json!({"visioId": id, "type": "rect", "x": x, "y": y, "w": w, "h": h, "fill": fill, "stroke": "#000000", "strokeWidth": 1.0, "label": label})
    }

    fn connector(id: &str, from: Option<&str>, to: Option<&str>) -> Value {
        let end = |id: Option<&str>| id.map(|id| json!({"shapeId": format!("shape:visio:{id}")}));
        json!({"visioId": id, "from": end(from), "to": end(to)})
    }

    fn model(shapes: Vec<Value>, connectors: Vec<Value>) -> Value {
        json!({"pages": [{"name": "Page-1", "shapes": shapes, "connectors": connectors}]})
    }

    fn kinds(issues: &[Issue]) -> Vec<&str> {
        issues.iter().map(|i| i.kind.as_str()).collect()
    }

    #[test]
    fn a_loose_end_names_which_end_and_offers_the_fix() {
        let m = model(vec![rect("1", 0.0, 0.0, 50.0, 50.0, "#ffffff", "A")], vec![connector("9", Some("1"), None), connector("8", None, None)]);
        let issues = check_page(&m, 0, None, false);
        assert_eq!(issues[0].message, "Connector 9 has a loose end (not glued to a shape)");
        assert_eq!(issues[1].message, "Connector 8 has a loose start and end (not glued to a shape)");
        assert!(issues.iter().all(|i| i.kind == "loose end" && i.fix.as_deref() == Some("Delete connector")));
    }

    #[test]
    fn the_second_connector_between_the_same_shapes_is_the_duplicate() {
        let m = model(
            vec![rect("1", 0.0, 0.0, 50.0, 50.0, "#ffffff", "A"), rect("2", 100.0, 0.0, 50.0, 50.0, "#ffffff", "")],
            vec![connector("5", Some("1"), Some("2")), connector("6", Some("1"), Some("2")), connector("7", Some("2"), Some("1"))],
        );
        let issues = check_page(&m, 0, None, false);
        assert_eq!(kinds(&issues), ["duplicate connector"]);
        assert_eq!(issues[0].id, "6");
        assert_eq!(issues[0].message, "Connector 6 joins \"A\" to shape 2 like connector 5 does");
    }

    #[test]
    fn a_shape_no_connector_touches_is_not_connected_but_a_caption_is_not_a_node() {
        let mut caption = rect("3", 300.0, 0.0, 50.0, 20.0, "none", "Title");
        caption["stroke"] = json!(null);
        let m = model(
            vec![rect("1", 0.0, 0.0, 50.0, 50.0, "#ffffff", "A"), rect("2", 100.0, 0.0, 50.0, 50.0, "#ffffff", "B"), rect("4", 200.0, 0.0, 50.0, 50.0, "#ffffff", "Lonely"), caption],
            vec![connector("5", Some("1"), Some("2"))],
        );
        let issues = check_page(&m, 0, None, false);
        assert_eq!(kinds(&issues), ["not connected"]);
        assert_eq!(issues[0].message, "\"Lonely\" is not connected to anything");
        // without connectors on the page nothing is "not connected"
        assert!(check_page(&model(vec![rect("1", 0.0, 0.0, 50.0, 50.0, "#fff", "A")], vec![]), 0, None, false).is_empty());
    }

    #[test]
    fn overlap_needs_a_quarter_of_the_smaller_shape_and_ignores_containers() {
        let m = model(
            vec![
                rect("1", 0.0, 0.0, 100.0, 100.0, "#ff0000", "A"),
                rect("2", 50.0, 50.0, 100.0, 100.0, "#00ff00", "B"),
                rect("3", 10.0, 10.0, 20.0, 20.0, "#0000ff", "inside A"),
                rect("4", 95.0, 95.0, 100.0, 100.0, "#00ffff", "touches B a little"),
            ],
            vec![],
        );
        let issues = check_page(&m, 0, None, false);
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert_eq!(issues[0].message, "\"A\" and \"B\" overlap");
        assert_eq!(issues[0].id, "1");
        assert_eq!(issues[1].message, "\"B\" and \"touches B a little\" overlap");
    }

    #[test]
    fn outside_the_page_offers_a_fix_only_for_a_shape_that_fits() {
        let m = model(vec![rect("1", -20.0, 0.0, 50.0, 50.0, "#fff", "Left"), rect("2", 0.0, 200.0, 2000.0, 50.0, "#fff", "Huge")], vec![]);
        let issues = check_page(&m, 0, Some((11.0, 8.5)), false);
        assert_eq!(kinds(&issues), ["outside the page", "outside the page"]);
        assert_eq!(issues[0].fix.as_deref(), Some("Move into page"));
        assert_eq!(issues[1].fix, None);
        assert!(check_page(&m, 0, None, false).is_empty(), "no size, no rule");
    }

    #[test]
    fn alt_text_is_off_unless_asked_and_a_picture_is_named_as_such() {
        let mut picture = rect("2", 100.0, 0.0, 50.0, 50.0, "none", "");
        picture["type"] = json!("image");
        let mut described = rect("3", 200.0, 0.0, 50.0, 50.0, "#fff", "");
        described["altText"] = json!("A server");
        let m = model(vec![rect("1", 0.0, 0.0, 50.0, 50.0, "#fff", "  "), picture, described], vec![]);
        assert!(check_page(&m, 0, None, false).is_empty());
        let issues = check_page(&m, 0, None, true);
        assert_eq!(issues.iter().map(|i| i.message.as_str()).collect::<Vec<_>>(), ["Shape 1 has neither text nor alt text (Shape data > Alt text)", "The picture 2 has neither text nor alt text (Shape data > Alt text)"]);
    }

    #[test]
    fn issues_come_out_sorted_by_kind_and_names_are_cut_at_30_units() {
        let long = "x".repeat(40);
        let m = model(vec![rect("1", 0.0, 0.0, 50.0, 50.0, "#fff", &long), rect("2", 100.0, 0.0, 50.0, 50.0, "#fff", "B")], vec![connector("9", Some("2"), None)]);
        let issues = check_page(&m, 0, None, false);
        assert_eq!(kinds(&issues), ["loose end", "not connected"]);
        assert_eq!(issues[1].message, format!("\"{}\" is not connected to anything", "x".repeat(30)));
    }

    #[test]
    fn csv_quotes_fields_and_uses_crlf_and_the_page_column() {
        let mut i = issue("overlap", "4", "\"A, B\" overlap".to_string(), None);
        i.page = Some(1);
        let csv = issues_to_csv(&[i], &["One".to_string(), "Two".to_string()]);
        assert_eq!(csv, "Page,Problem,Shape,Message,Fix\r\nTwo,overlap,4,\"\"\"A, B\"\" overlap\",\r\n");
    }
}
