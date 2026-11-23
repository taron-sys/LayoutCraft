//! File > Open of an Excalidraw scene: a Rust port of `web/excalidraw.js` (docs/34, step N4.7b). A scene (.excalidraw JSON) becomes a new drawing in the engine,
//! with the same engine calls as the web code (draw, style, text, rotate, picture, connector, polyline). `tests/excalidraw_import_oracle.rs` compares the drawings
//! the two make from the same scenes.
use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::dxf::hypot;
use crate::svgutil::js_round;
use crate::Engine;

const PX_PER_IN: f64 = 96.0;

/// What `importExcalidraw` returns.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportSummary {
    pub page: usize,
    pub shapes: Vec<String>,
    pub connectors: usize,
    pub skipped: BTreeMap<String, usize>,
    pub map: BTreeMap<String, String>,
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn number(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// JavaScript `x || 0`.
fn number_or_zero(v: Option<&Value>) -> f64 {
    let n = number(v);
    if n.is_nan() || n == 0.0 { 0.0 } else { n }
}

/// How JavaScript writes a property key of a value (`e.id`, `e.type` of an element that has none give "undefined").
fn key_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => crate::svgutil::js_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) => "null".to_string(),
        None => "undefined".to_string(),
        Some(_) => "[object Object]".to_string(),
    }
}

fn kind_of(type_: &str) -> Option<i32> {
    match type_ {
        "rectangle" | "frame" | "magicframe" => Some(0),
        "ellipse" => Some(2),
        "diamond" => Some(3),
        _ => None,
    }
}

fn font_name(family: Option<&Value>) -> &'static str {
    match key_of(family).as_str() {
        "1" | "5" => "Segoe Print",
        "2" => "Arial",
        "3" | "8" => "Consolas",
        "6" => "Calibri",
        "7" => "Cambria",
        _ => "Arial",
    }
}

fn arrow_head(name: Option<&Value>) -> Option<i64> {
    match name.and_then(Value::as_str)? {
        "arrow" | "triangle" | "triangle_outline" => Some(13),
        "bar" => Some(6),
        "dot" | "circle" | "circle_outline" => Some(10),
        "diamond" | "diamond_outline" => Some(15),
        _ => None,
    }
}

fn colour(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
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

/// `atob` of JavaScript: white space is ignored, padding is optional, anything else outside the alphabet fails.
pub fn atob(text: &str) -> Option<Vec<u8>> {
    let mut chars: Vec<u8> = text.bytes().filter(|b| !matches!(b, b' ' | b'\t' | b'\n' | 0x0c | b'\r')).collect();
    if chars.len() % 4 == 0 {
        for _ in 0..2 {
            if chars.last() == Some(&b'=') {
                chars.pop();
            }
        }
    }
    if chars.len() % 4 == 1 {
        return None;
    }
    let value = |b: u8| -> Option<u32> {
        Some(match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a') as u32 + 26,
            b'0'..=b'9' => (b - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::new();
    for group in chars.chunks(4) {
        let mut acc = 0u32;
        for (i, b) in group.iter().enumerate() {
            acc |= value(*b)? << (18 - 6 * i);
        }
        out.push((acc >> 16) as u8);
        if group.len() > 2 {
            out.push((acc >> 8) as u8);
        }
        if group.len() > 3 {
            out.push(acc as u8);
        }
    }
    Some(out)
}

/// The bytes of the picture an `image` element shows (`scene.files[fileId].dataURL`), or None.
fn picture_bytes(scene: &Value, e: &Value) -> Option<Vec<u8>> {
    let file = scene.get("files")?.get(key_of(e.get("fileId")))?;
    let url = file.get("dataURL")?.as_str()?;
    // /^data:image\/(png|jpe?g|gif|bmp);base64,(.+)$/is
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("data:image/")?;
    let kind_len = ["png", "jpeg", "jpg", "gif", "bmp"].iter().find(|k| rest.starts_with(*k) && rest[k.len()..].starts_with(";base64,")).map(|k| k.len())?;
    let start = "data:image/".len() + kind_len + ";base64,".len();
    let data = &url[start..];
    if data.is_empty() || !(number(e.get("width")) > 0.0) || !(number(e.get("height")) > 0.0) {
        return None;
    }
    atob(data)
}

fn bounds_of(elements: &[&Value]) -> (f64, f64, f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let nan_min = |a: f64, b: f64| if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) };
    let nan_max = |a: f64, b: f64| if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) };
    for e in elements {
        let (ex, ey) = (number(e.get("x")), number(e.get("y")));
        let points = e.get("points").and_then(Value::as_array).filter(|p| !p.is_empty());
        let mut add = |x: f64, y: f64| {
            x0 = nan_min(x0, x);
            y0 = nan_min(y0, y);
            x1 = nan_max(x1, x);
            y1 = nan_max(y1, y);
        };
        if let Some(points) = points {
            for p in points {
                add(ex + number(p.get(0)), ey + number(p.get(1)));
            }
        } else {
            add(ex, ey);
            add(ex + number_or_zero(e.get("width")), ey + number_or_zero(e.get("height")));
        }
    }
    if x0.is_finite() { (x0, y0, x1, y1) } else { (0.0, 0.0, 0.0, 0.0) }
}

/// Parse and check an Excalidraw scene.
pub fn parse_scene(text: &str) -> Result<Value, String> {
    let scene: Value = serde_json::from_str(text).map_err(|_| "This is not an Excalidraw file (the JSON cannot be read)".to_string())?;
    if scene.get("type").and_then(Value::as_str) != Some("excalidraw") || !scene.get("elements").is_some_and(Value::is_array) {
        return Err("This is not an Excalidraw file (no \"type\": \"excalidraw\" and elements list)".to_string());
    }
    Ok(scene)
}

struct Importer<'a> {
    engine: &'a mut Engine,
    b: (f64, f64, f64, f64),
    margin: f64,
    height: f64,
}

impl Importer<'_> {
    fn x(&self, x: f64) -> f64 {
        (x - self.b.0) / PX_PER_IN + self.margin
    }
    fn y(&self, y: f64) -> f64 {
        self.height - ((y - self.b.1) / PX_PER_IN + self.margin)
    }

    fn style_shapes(&mut self, id: &str, style: &Map<String, Value>) -> Result<(), String> {
        self.engine.edit(0, 10, &format!("{id}\n{}", Value::Object(style.clone())), 0.0, 0.0, 0.0, 0.0, 0)
    }

    /// `style(e, id, extra)`.
    fn style(&mut self, e: &Value, id: &str, extra: Map<String, Value>) -> Result<(), String> {
        let mut s = extra;
        let line = colour(e.get("strokeColor"));
        let fill = colour(e.get("backgroundColor"));
        if let Some(line) = line {
            s.insert("line".into(), json!(line));
        }
        let bg = e.get("backgroundColor");
        if bg.is_none() || bg == Some(&Value::Null) || bg.and_then(Value::as_str) == Some("transparent") {
            s.insert("fill".into(), json!("none"));
        } else if let Some(fill) = fill {
            s.insert("fill".into(), json!(fill));
        }
        let sw = number(e.get("strokeWidth"));
        if sw > 0.0 {
            s.insert("weight".into(), e["strokeWidth"].clone());
        }
        let pattern = match e.get("strokeStyle").and_then(Value::as_str) {
            Some("dashed") => 2,
            Some("dotted") => 3,
            _ => 1,
        };
        s.insert("pattern".into(), json!(pattern));
        if let Some(o) = e.get("opacity").and_then(Value::as_f64) {
            if o < 100.0 {
                let t = js_round((1.0 - o / 100.0) * 100.0) / 100.0;
                s.insert("lineTrans".into(), json!(t));
                if s.get("fill") != Some(&json!("none")) {
                    s.insert("fillTrans".into(), json!(t));
                }
            }
        }
        self.style_shapes(id, &s)
    }

    fn font_of(e: &Value) -> Map<String, Value> {
        let size = {
            let n = number(e.get("fontSize"));
            if n.is_nan() || n == 0.0 { 20.0 } else { n }
        };
        let mut m = Map::new();
        m.insert("fontPt".into(), json!(js_round(size * 0.75 * 10.0) / 10.0));
        m.insert("fontFamily".into(), json!(font_name(e.get("fontFamily"))));
        m.insert("textColor".into(), json!(colour(e.get("strokeColor")).unwrap_or_else(|| "#000000".to_string())));
        m
    }

    fn turn(&mut self, e: &Value, id: &str) -> Result<(), String> {
        let angle = number(e.get("angle"));
        if truthy(e.get("angle")) {
            let two_pi = 2.0 * std::f64::consts::PI;
            self.engine.edit(0, 7, id, ((-angle % two_pi) + two_pi) % two_pi, 0.0, 0.0, 0.0, 0)?;
        }
        Ok(())
    }

    fn set_text(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.engine.edit(0, 8, &format!("{id}\n{text}"), 0.0, 0.0, 0.0, 0.0, 0)
    }
}

/// Build a new drawing in `engine` from the scene text. The drawing replaces the engine's document.
pub fn import_scene(engine: &mut Engine, input: &str, margin: f64) -> Result<ImportSummary, String> {
    let scene = parse_scene(input)?;
    let all: Vec<&Value> = scene["elements"].as_array().unwrap().iter().filter(|e| e.is_object() && !truthy(e.get("isDeleted"))).collect();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut drawable: Vec<&Value> = Vec::new();
    for e in &all {
        let t = e.get("type").and_then(Value::as_str).unwrap_or("");
        let ok = kind_of(t).is_some() || matches!(t, "text" | "line" | "arrow" | "freedraw") || (t == "image" && picture_bytes(&scene, e).is_some());
        if ok {
            drawable.push(e);
        } else {
            *skipped.entry(key_of(e.get("type"))).or_insert(0) += 1;
        }
    }
    let b = bounds_of(&drawable);
    let width = 2.0f64.max((b.2 - b.0) / PX_PER_IN + 2.0 * margin);
    let height = 2.0f64.max((b.3 - b.1) / PX_PER_IN + 2.0 * margin);
    engine.new_blank(js_round(width * 1000.0) / 1000.0, js_round(height * 1000.0) / 1000.0, false, "Excalidraw")?;
    let mut imp = Importer { engine, b, margin, height };
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut shapes: Vec<String> = Vec::new();
    let mut bound_text: BTreeMap<String, &Value> = BTreeMap::new();
    for e in &drawable {
        if e.get("type").and_then(Value::as_str) == Some("text") && truthy(e.get("containerId")) {
            bound_text.insert(key_of(e.get("containerId")), e);
        }
    }

    // closed shapes first (connectors need them), then pictures, free text, then lines
    for e in drawable.iter().filter(|d| kind_of(d.get("type").and_then(Value::as_str).unwrap_or("")).is_some()) {
        let t = e.get("type").and_then(Value::as_str).unwrap_or("");
        let kind = if t == "rectangle" && truthy(e.get("roundness")) { 1 } else { kind_of(t).unwrap() };
        let (ex, ey, ew, eh) = (number(e.get("x")), number(e.get("y")), number(e.get("width")), number(e.get("height")));
        let id = imp.engine.draw(0, kind, imp.x(ex), imp.y(ey + eh), imp.x(ex + ew), imp.y(ey))?;
        map.insert(key_of(e.get("id")), id.clone());
        shapes.push(id.clone());
        imp.style(e, &id, Map::new())?;
        if let Some(bt) = bound_text.get(&key_of(e.get("id"))) {
            if truthy(bt.get("text")) {
                imp.set_text(&id, bt["text"].as_str().unwrap_or(""))?;
                imp.style_shapes(&id, &Importer::font_of(bt))?;
            }
        }
        imp.turn(e, &id)?;
        if matches!(t, "frame" | "magicframe") && truthy(e.get("name")) {
            let name = e.get("name").and_then(Value::as_str).unwrap_or("");
            let name_len = name.encode_utf16().count() as f64;
            let name_box = imp.engine.draw(0, 5, imp.x(ex), imp.y(ey), imp.x(ex + ew.max(60.0).min(name_len * 9.0 + 12.0)), imp.y(ey - 24.0))?;
            let mut st = Map::new();
            st.insert("fill".into(), json!("none"));
            st.insert("line".into(), json!("none"));
            st.insert("fontPt".into(), json!(11));
            st.insert("textColor".into(), json!("#666666"));
            st.insert("fontFamily".into(), json!("Arial"));
            imp.style_shapes(&name_box, &st)?;
            imp.set_text(&name_box, name)?;
            shapes.push(name_box);
        }
    }
    for e in drawable.iter().filter(|d| d.get("type").and_then(Value::as_str) == Some("image")) {
        let (ex, ey, ew, eh) = (number(e.get("x")), number(e.get("y")), number(e.get("width")), number(e.get("height")));
        let attempt = (|| -> Result<(), String> {
            let bytes = picture_bytes(&scene, e).ok_or("no picture")?;
            let id = imp.engine.insert_picture(0, &bytes, imp.x(ex), imp.y(ey + eh), imp.x(ex + ew), imp.y(ey))?;
            map.insert(key_of(e.get("id")), id.clone());
            shapes.push(id.clone());
            imp.turn(e, &id)
        })();
        if attempt.is_err() {
            *skipped.entry(key_of(e.get("type"))).or_insert(0) += 1;
        }
    }
    for e in drawable.iter().filter(|d| d.get("type").and_then(Value::as_str) == Some("text") && !truthy(d.get("containerId"))) {
        let (ex, ey, ew, eh) = (number(e.get("x")), number(e.get("y")), number(e.get("width")), number(e.get("height")));
        let id = imp.engine.draw(0, 5, imp.x(ex), imp.y(ey + eh), imp.x(ex + ew), imp.y(ey))?;
        map.insert(key_of(e.get("id")), id.clone());
        shapes.push(id.clone());
        let mut st = Importer::font_of(e);
        st.insert("fill".into(), json!("none"));
        st.insert("line".into(), json!("none"));
        imp.style_shapes(&id, &st)?;
        imp.set_text(&id, e.get("text").and_then(Value::as_str).unwrap_or(""))?;
        imp.style_shapes(&id, &Importer::font_of(e))?;
        imp.turn(e, &id)?;
    }
    let mut connectors = 0;
    for e in drawable.iter().filter(|d| matches!(d.get("type").and_then(Value::as_str), Some("line" | "arrow" | "freedraw"))) {
        let t = e.get("type").and_then(Value::as_str).unwrap_or("");
        let (ex, ey) = (number(e.get("x")), number(e.get("y")));
        let pts: Vec<(f64, f64)> = e.get("points").and_then(Value::as_array).map_or_else(Vec::new, |ps| ps.iter().map(|p| (imp.x(ex + number(p.get(0))), imp.y(ey + number(p.get(1))))).collect());
        if pts.len() < 2 {
            *skipped.entry(key_of(e.get("type"))).or_insert(0) += 1;
            continue;
        }
        let bound = |name: &str| -> Option<String> {
            let binding = e.get(name).filter(|b| truthy(Some(b)))?;
            map.get(&key_of(binding.get("elementId"))).cloned()
        };
        let (from, to) = (bound("startBinding"), bound("endBinding"));
        let (id, closed);
        match (t, &from, &to) {
            ("arrow", Some(f), Some(tt)) if f != tt => {
                id = imp.engine.add_connector(0, f, None, tt, None)?;
                connectors += 1;
                closed = false;
            }
            _ => {
                closed = t != "arrow" && pts.len() > 2 && hypot(pts[0].0 - pts[pts.len() - 1].0, pts[0].1 - pts[pts.len() - 1].1) < 0.02;
                id = imp.engine.draw_polyline(0, if closed { &pts[..pts.len() - 1] } else { &pts }, closed)?;
            }
        }
        map.insert(key_of(e.get("id")), id.clone());
        shapes.push(id.clone());
        let mut extra = Map::new();
        if t == "arrow" {
            if let Some(end) = arrow_head(e.get("endArrowhead")) {
                extra.insert("endArrow".into(), json!(end));
            }
            if let Some(begin) = arrow_head(e.get("startArrowhead")) {
                extra.insert("beginArrow".into(), json!(begin));
            }
        }
        if !closed {
            extra.insert("fill".into(), json!("none"));
        }
        imp.style(e, &id, extra)?;
    }
    Ok(ImportSummary { page: 0, shapes, connectors, skipped, map })
}

impl Engine {
    /// File > Open of an Excalidraw scene (the text of a .excalidraw file): a new drawing replaces the open one.
    pub fn import_excalidraw(&mut self, text: &str) -> Result<ImportSummary, String> {
        import_scene(self, text, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atob_follows_javascript() {
        assert_eq!(atob("aGk=").unwrap(), b"hi");
        assert_eq!(atob("aGk").unwrap(), b"hi");
        assert_eq!(atob(" aG\nk= ").unwrap(), b"hi");
        assert!(atob("a").is_none());
        assert!(atob("aG*k").is_none());
    }

    #[test]
    fn colours_are_lowercased_and_short_forms_expanded() {
        assert_eq!(colour(Some(&json!("#A5D8FF"))).as_deref(), Some("#a5d8ff"));
        assert_eq!(colour(Some(&json!("#ABC"))).as_deref(), Some("#aabbcc"));
        assert_eq!(colour(Some(&json!("transparent"))), None);
        assert_eq!(colour(Some(&json!("#12345"))), None);
    }

    #[test]
    fn bad_scenes_say_why() {
        assert_eq!(parse_scene("nope").unwrap_err(), "This is not an Excalidraw file (the JSON cannot be read)");
        assert!(parse_scene("{\"type\":\"excalidraw\"}").unwrap_err().contains("elements list"));
    }

    #[test]
    fn a_rectangle_with_bound_text_and_an_arrow_between_two_shapes_become_shapes_and_a_connector() {
        let scene = json!({"type": "excalidraw", "elements": [
            {"id": "a", "type": "rectangle", "x": 0, "y": 0, "width": 96, "height": 48, "strokeColor": "#000000", "backgroundColor": "#ff0000", "strokeWidth": 2},
            {"id": "ta", "type": "text", "x": 5, "y": 5, "width": 50, "height": 20, "text": "Hi", "containerId": "a", "fontSize": 20},
            {"id": "b", "type": "ellipse", "x": 300, "y": 0, "width": 96, "height": 48},
            {"id": "ar", "type": "arrow", "x": 96, "y": 24, "points": [[0, 0], [204, 0]], "startBinding": {"elementId": "a"}, "endBinding": {"elementId": "b"}, "endArrowhead": "triangle"},
            {"id": "x", "type": "embeddable", "x": 1, "y": 1}
        ]});
        let mut engine = Engine::new();
        let summary = import_scene(&mut engine, &scene.to_string(), 0.5).unwrap();
        assert_eq!(summary.shapes.len(), 3);
        assert_eq!(summary.connectors, 1);
        assert_eq!(summary.skipped.get("embeddable"), Some(&1));
        assert_eq!(summary.map.len(), 3);
        let model: Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        assert_eq!(model["pages"][0]["connectors"].as_array().unwrap().len(), 1);
        assert!(model["pages"][0]["shapes"].as_array().unwrap().iter().any(|s| s["label"] == "Hi"));
    }
}
