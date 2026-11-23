//! Review > Compare Documents: a Rust port of `web/compare.js` (docs/34, step N4.2). The open drawing is "new" (`current`), the chosen file is "old" (`other`).
//! Pages are matched by name, shapes and connectors by their Visio id inside a page. The texts and the order are those of the web code;
//! `tests/compare_oracle.rs` compares the result with what the web finds (`visio-test-harness/dump-compare-oracle.mjs`).
use std::collections::HashMap;

use serde_json::Value;

const EPS: f64 = 0.5;

/// One difference. `page` is the page index in the open drawing, -1 for a page that is only in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub page: i64,
    pub page_name: String,
    /// "added", "removed" or "changed".
    pub kind: String,
    /// "page", "shape" or "connector".
    pub what: String,
    pub id: String,
    pub label: String,
    pub detail: String,
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// JavaScript `x || 0` for a number field.
fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).filter(|n| !n.is_nan()).unwrap_or(0.0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS
}

/// JavaScript `Math.round`: halves go up.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// JavaScript `String(number)` for the numbers made here (finite, modest size): no "-0", no trailing ".0".
fn js_num(x: f64) -> String {
    let x = x + 0.0;
    if x == 0.0 {
        "0".to_string()
    } else {
        format!("{x}")
    }
}

fn inches(px: f64) -> String {
    js_num(js_round(px / 0.96) / 100.0)
}

fn visio_id(v: &Value) -> String {
    match v.get("visioId") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => "undefined".to_string(),
    }
}

fn label(s: &Value) -> String {
    let text = str_of(s, "label");
    if text.is_empty() {
        format!("shape {}", visio_id(s))
    } else {
        let first = text.split('\n').next().unwrap_or("");
        let units: Vec<u16> = first.encode_utf16().take(30).collect();
        format!("\"{}\"", String::from_utf16_lossy(&units))
    }
}

fn or_none(v: &Value, key: &str) -> String {
    let t = str_of(v, key);
    if t.is_empty() { "none".to_string() } else { t.to_string() }
}

/// The differences between shape `a` (new) and `b` (old), as the texts the web shows.
pub fn compare_shape(a: &Value, b: &Value) -> Vec<String> {
    let mut d = Vec::new();
    if str_of(a, "label") != str_of(b, "label") {
        d.push(format!("text \"{}\" -> \"{}\"", str_of(b, "label"), str_of(a, "label")));
    }
    if !near(num(a, "x"), num(b, "x")) || !near(num(a, "y"), num(b, "y")) {
        d.push(format!("moved {} in right, {} in down", inches(num(a, "x") - num(b, "x")), inches(num(a, "y") - num(b, "y"))));
    }
    if !near(num(a, "w"), num(b, "w")) || !near(num(a, "h"), num(b, "h")) {
        d.push(format!("size {} x {} in -> {} x {} in", inches(num(b, "w")), inches(num(b, "h")), inches(num(a, "w")), inches(num(a, "h"))));
    }
    if str_of(a, "altText") != str_of(b, "altText") {
        d.push(format!("alt text \"{}\" -> \"{}\"", str_of(b, "altText"), str_of(a, "altText")));
    }
    if or_none(a, "fill") != or_none(b, "fill") {
        d.push(format!("fill {} -> {}", or_none(b, "fill"), or_none(a, "fill")));
    }
    if or_none(a, "stroke") != or_none(b, "stroke") {
        d.push(format!("line {} -> {}", or_none(b, "stroke"), or_none(a, "stroke")));
    }
    if !near(num(a, "rot"), num(b, "rot")) {
        d.push(format!("rotation {} -> {} degrees", js_num(js_round(num(b, "rot"))), js_num(js_round(num(a, "rot")))));
    }
    d
}

fn end_id(e: Option<&Value>) -> String {
    match e.and_then(|e| e.get("shapeId")) {
        Some(Value::String(s)) if !s.is_empty() => s.replacen("shape:visio:", "", 1),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// A JavaScript `Map` built from a list: first-seen order, the last value of a repeated key.
struct Ordered<'a> {
    order: Vec<String>,
    values: HashMap<String, &'a Value>,
}

fn ordered<'a>(list: Option<&'a Value>) -> Ordered<'a> {
    let mut o = Ordered { order: Vec::new(), values: HashMap::new() };
    for item in list.and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
        let id = visio_id(item);
        if o.values.insert(id.clone(), item).is_none() {
            o.order.push(id);
        }
    }
    o
}

fn page_name(page: &Value, index: usize) -> String {
    let n = str_of(page, "name");
    if n.is_empty() { format!("Page-{}", index + 1) } else { n.to_string() }
}

pub fn compare_models(current: &Value, other: &Value) -> Vec<Diff> {
    let mut out = Vec::new();
    let empty = Vec::new();
    let pages_a = current.get("pages").and_then(Value::as_array).unwrap_or(&empty);
    let pages_b = other.get("pages").and_then(Value::as_array).unwrap_or(&empty);
    let mut by_name: HashMap<String, &Value> = HashMap::new();
    for (i, p) in pages_b.iter().enumerate() {
        by_name.insert(page_name(p, i), p);
    }
    let mut seen = std::collections::HashSet::new();
    for (i, pa) in pages_a.iter().enumerate() {
        let name = page_name(pa, i);
        let mk = |kind: &str, what: &str, id: &str, label: String, detail: String| Diff { page: i as i64, page_name: name.clone(), kind: kind.to_string(), what: what.to_string(), id: id.to_string(), label, detail };
        let Some(pb) = by_name.get(&name) else {
            out.push(mk("added", "page", "", name.clone(), "page is not in the file".to_string()));
            continue;
        };
        seen.insert(name.clone());
        let (sa, sb) = (ordered(pa.get("shapes")), ordered(pb.get("shapes")));
        for id in &sa.order {
            let s = sa.values[id];
            match sb.values.get(id) {
                None => out.push(mk("added", "shape", id, label(s), "not in the file".to_string())),
                Some(o) => {
                    let diff = compare_shape(s, o);
                    if !diff.is_empty() {
                        out.push(mk("changed", "shape", id, label(s), diff.join("; ")));
                    }
                }
            }
        }
        for id in &sb.order {
            if !sa.values.contains_key(id) {
                out.push(mk("removed", "shape", id, label(sb.values[id]), "only in the file".to_string()));
            }
        }
        let (ca, cb) = (ordered(pa.get("connectors")), ordered(pb.get("connectors")));
        for id in &ca.order {
            let c = ca.values[id];
            match cb.values.get(id) {
                None => out.push(mk("added", "connector", id, format!("connector {id}"), "not in the file".to_string())),
                Some(o) => {
                    let mut diff = Vec::new();
                    let nothing = |s: String| if s.is_empty() { "nothing".to_string() } else { s };
                    if end_id(c.get("from")) != end_id(o.get("from")) || end_id(c.get("to")) != end_id(o.get("to")) {
                        diff.push(format!("glued {} - {} -> {} - {}", nothing(end_id(o.get("from"))), nothing(end_id(o.get("to"))), nothing(end_id(c.get("from"))), nothing(end_id(c.get("to")))));
                    }
                    if str_of(c, "label") != str_of(o, "label") {
                        diff.push(format!("text \"{}\" -> \"{}\"", str_of(o, "label"), str_of(c, "label")));
                    }
                    if str_of(c, "stroke") != str_of(o, "stroke") {
                        diff.push(format!("line {} -> {}", or_none(o, "stroke"), or_none(c, "stroke")));
                    }
                    if !diff.is_empty() {
                        out.push(mk("changed", "connector", id, format!("connector {id}"), diff.join("; ")));
                    }
                }
            }
        }
        for id in &cb.order {
            if !ca.values.contains_key(id) {
                out.push(mk("removed", "connector", id, format!("connector {id}"), "only in the file".to_string()));
            }
        }
    }
    for (i, pb) in pages_b.iter().enumerate() {
        let name = page_name(pb, i);
        if !seen.contains(&name) && !pages_a.iter().enumerate().any(|(k, pa)| page_name(pa, k) == name) {
            out.push(Diff { page: -1, page_name: name.clone(), kind: "removed".into(), what: "page".into(), id: String::new(), label: name, detail: "page is only in the file".into() });
        }
    }
    out
}

/// The differences as CSV (columns Page, Difference, Kind, Id, Item, Detail; CRLF; fields with comma, quote or line break quoted).
pub fn diffs_to_csv(diffs: &[Diff]) -> String {
    fn field(v: &str) -> String {
        if v.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v.to_string() }
    }
    let mut rows: Vec<[&str; 6]> = vec![["Page", "Difference", "Kind", "Id", "Item", "Detail"]];
    for d in diffs {
        rows.push([&d.page_name, &d.kind, &d.what, &d.id, &d.label, &d.detail]);
    }
    rows.iter().map(|r| r.iter().map(|f| field(f)).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shape(id: &str, x: f64, label: &str) -> Value {
        json!({"visioId": id, "x": x, "y": 10.0, "w": 96.0, "h": 48.0, "fill": "#ffffff", "stroke": "#000000", "rot": 0.0, "label": label})
    }

    fn doc(shapes: Vec<Value>, connectors: Vec<Value>) -> Value {
        json!({"pages": [{"name": "Page-1", "shapes": shapes, "connectors": connectors}]})
    }

    #[test]
    fn a_drawing_equals_itself() {
        let d = doc(vec![shape("1", 0.0, "A")], vec![]);
        assert!(compare_models(&d, &d).is_empty());
    }

    #[test]
    fn changes_added_and_removed_shapes_are_told_with_the_web_texts() {
        let new = doc(vec![shape("1", 96.0, "A2"), shape("3", 0.0, "")], vec![]);
        let old = doc(vec![shape("1", 0.0, "A"), shape("2", 0.0, "B")], vec![]);
        let diffs = compare_models(&new, &old);
        let details: Vec<String> = diffs.iter().map(|d| format!("{} {} {}: {}", d.kind, d.what, d.id, d.detail)).collect();
        assert_eq!(details, ["changed shape 1: text \"A\" -> \"A2\"; moved 1 in right, 0 in down", "added shape 3: not in the file", "removed shape 2: only in the file"]);
        assert_eq!(diffs[1].label, "shape 3");
    }

    #[test]
    fn half_a_pixel_is_no_move_and_rounding_follows_javascript() {
        assert!(compare_shape(&shape("1", 0.4, "A"), &shape("1", 0.0, "A")).is_empty());
        assert_eq!(compare_shape(&shape("1", -0.96 * 0.4, "A"), &shape("1", 0.0, "A")), Vec::<String>::new());
        assert_eq!(compare_shape(&shape("1", -0.1, "A"), &shape("1", 0.4, "A")), Vec::<String>::new());
        assert_eq!(js_round(-0.5), 0.0);
        assert_eq!(js_num(-0.0), "0");
        assert_eq!(compare_shape(&shape("1", -4.0, "A"), &shape("1", 0.0, "A")), ["moved -0.04 in right, 0 in down"]);
    }

    #[test]
    fn connectors_pages_and_csv() {
        let con = |id: &str, to: &str| json!({"visioId": id, "from": {"shapeId": "shape:visio:1"}, "to": {"shapeId": format!("shape:visio:{to}")}, "label": "", "stroke": "#000"});
        let new = doc(vec![], vec![con("5", "2"), con("6", "2")]);
        let mut old = doc(vec![], vec![con("5", "3")]);
        old["pages"].as_array_mut().unwrap().push(json!({"name": "Extra", "shapes": [], "connectors": []}));
        let diffs = compare_models(&new, &old);
        let lines: Vec<String> = diffs.iter().map(|d| format!("{}|{}|{}|{}", d.page, d.label, d.kind, d.detail)).collect();
        assert_eq!(lines, ["0|connector 5|changed|glued 1 - 3 -> 1 - 2", "0|connector 6|added|not in the file", "-1|Extra|removed|page is only in the file"]);
        assert_eq!(diffs_to_csv(&diffs[2..]), "Page,Difference,Kind,Id,Item,Detail\r\nExtra,removed,page,,Extra,page is only in the file\r\n");
    }
}
