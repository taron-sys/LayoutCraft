//! File > Export > Excalidraw: a Rust port of `web/excalidraw-export.js` (docs/34, step N4.7). The page of the open drawing becomes an Excalidraw scene (the public
//! .excalidraw JSON format) built from the document model. Same scene as the web (`tests/excalidraw_export_oracle.rs` compares them on every page of 145 drawings).
use serde_json::{json, Map, Value};

use crate::svgutil::{is_js_space, js_round};
use crate::Engine;

fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// JavaScript `x || fallback` for a number field.
fn num_or(v: &Value, key: &str, fallback: f64) -> f64 {
    match v.get(key).and_then(Value::as_f64) {
        Some(n) if n != 0.0 && !n.is_nan() => n,
        _ => fallback,
    }
}

fn truthy(v: &Value, key: &str) -> bool {
    match v.get(key) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn text_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The id of a model shape as JavaScript writes it into a template string.
fn id_string(v: &Value) -> String {
    match v.get("visioId") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => "undefined".to_string(),
    }
}

fn family(name: &str) -> i32 {
    let l = name.to_lowercase();
    if ["consolas", "courier", "mono", "cascadia"].iter().any(|k| l.contains(k)) {
        3
    } else if ["print", "hand", "comic", "segoe script"].iter().any(|k| l.contains(k)) {
        1
    } else {
        2
    }
}

fn arrow_end(name: Option<&str>) -> Value {
    match name {
        Some("filled") | Some("open") => json!("arrow"),
        Some("triangle") => json!("triangle"),
        Some("dot") | Some("circle") => json!("dot"),
        Some("diamond") => json!("diamond"),
        Some("bar") => json!("bar"),
        _ => Value::Null,
    }
}

/// One flattened outline: its points, whether it was closed and whether it had an arc.
#[derive(Debug, Clone)]
pub struct Outline {
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    pub arc: bool,
}

/// The numbers of `m[2].match(/-?\d*\.?\d+(?:e-?\d+)?/gi)`.
fn numbers(text: &str) -> Vec<f64> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let start = i;
        let mut j = i;
        if c[j] == '-' {
            j += 1;
        }
        let d1 = j;
        while j < c.len() && c[j].is_ascii_digit() {
            j += 1;
        }
        let int_digits = j - d1;
        let mut ok = false;
        if j + 1 < c.len() && c[j] == '.' && c[j + 1].is_ascii_digit() {
            j += 1;
            while j < c.len() && c[j].is_ascii_digit() {
                j += 1;
            }
            ok = true;
        } else if int_digits > 0 {
            ok = true;
        }
        if !ok {
            i += 1;
            continue;
        }
        if j < c.len() && (c[j] == 'e' || c[j] == 'E') {
            let mut k = j + 1;
            if k < c.len() && c[k] == '-' {
                k += 1;
            }
            let ds = k;
            while k < c.len() && c[k].is_ascii_digit() {
                k += 1;
            }
            if k > ds {
                j = k;
            }
        }
        let token: String = c[start..j].iter().collect();
        out.push(token.parse::<f64>().unwrap_or(f64::NAN));
        i = j;
    }
    out
}

/// `flattenPath`: an SVG path (M L H V C Q A Z) to its outlines.
pub fn flatten_path(d: &str) -> Vec<Outline> {
    let mut out: Vec<Outline> = Vec::new();
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let (mut has_arc, mut closed) = (false, false);
    let mut cur = [f64::NAN, f64::NAN];
    let mut start: Option<[f64; 2]> = None;
    let flush = |pts: &mut Vec<[f64; 2]>, closed: &mut bool, has_arc: &mut bool, out: &mut Vec<Outline>| {
        if pts.len() > 1 {
            out.push(Outline { points: std::mem::take(pts), closed: *closed, arc: *has_arc });
        }
        pts.clear();
        *closed = false;
        *has_arc = false;
    };
    // /([MLHVCQAZ])([^MLHVCQAZ]*)/g
    let chars: Vec<char> = d.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !"MLHVCQAZ".contains(chars[i]) {
            i += 1;
            continue;
        }
        let cmd = chars[i];
        let mut j = i + 1;
        while j < chars.len() && !"MLHVCQAZ".contains(chars[j]) {
            j += 1;
        }
        let body: String = chars[i + 1..j].iter().collect();
        let v = numbers(&body);
        let at = |k: usize| v.get(k).copied().unwrap_or(f64::NAN);
        match cmd {
            'M' => {
                flush(&mut pts, &mut closed, &mut has_arc, &mut out);
                cur = [at(0), at(1)];
                start = Some(cur);
                pts = vec![cur];
            }
            'L' => {
                let mut k = 0;
                while k + 1 < v.len() {
                    cur = [v[k], v[k + 1]];
                    pts.push(cur);
                    k += 2;
                }
            }
            'H' => {
                cur = [at(0), cur[1]];
                pts.push(cur);
            }
            'V' => {
                cur = [cur[0], at(0)];
                pts.push(cur);
            }
            'C' => {
                let mut k = 0;
                while k + 5 < v.len() {
                    let p0 = cur;
                    for s in 1..=12 {
                        let t = s as f64 / 12.0;
                        let u = 1.0 - t;
                        pts.push([
                            u * u * u * p0[0] + 3.0 * u * u * t * v[k] + 3.0 * u * t * t * v[k + 2] + t * t * t * v[k + 4],
                            u * u * u * p0[1] + 3.0 * u * u * t * v[k + 1] + 3.0 * u * t * t * v[k + 3] + t * t * t * v[k + 5],
                        ]);
                    }
                    cur = [v[k + 4], v[k + 5]];
                    k += 6;
                }
            }
            'Q' => {
                let mut k = 0;
                while k + 3 < v.len() {
                    let p0 = cur;
                    for s in 1..=8 {
                        let t = s as f64 / 8.0;
                        let u = 1.0 - t;
                        pts.push([u * u * p0[0] + 2.0 * u * t * v[k] + t * t * v[k + 2], u * u * p0[1] + 2.0 * u * t * v[k + 1] + t * t * v[k + 3]]);
                    }
                    cur = [v[k + 2], v[k + 3]];
                    k += 4;
                }
            }
            'A' => {
                has_arc = true;
                let mut k = 0;
                while k + 6 < v.len() {
                    cur = [v[k + 5], v[k + 6]];
                    pts.push(cur);
                    k += 7;
                }
            }
            'Z' => {
                closed = true;
                if let Some(s) = start {
                    cur = s;
                }
                flush(&mut pts, &mut closed, &mut has_arc, &mut out);
                pts.clear();
            }
            _ => {}
        }
        i = j;
    }
    flush(&mut pts, &mut closed, &mut has_arc, &mut out);
    out
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// What an outline is.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Ellipse,
    Rectangle,
    Diamond,
    Polyline,
}

/// `classify`: the kind of a shape's outline and its flattened paths.
pub fn classify(parts: &[Value], w: f64, h: f64) -> (Kind, Vec<Outline>) {
    let paths: Vec<Outline> = parts.iter().flat_map(|p| flatten_path(text_of(p, "d"))).collect();
    if paths.len() != 1 {
        return (Kind::Polyline, paths);
    }
    let (points, closed, arc) = (&paths[0].points, paths[0].closed, paths[0].arc);
    let tol = 0.6f64.max(w.max(h) * 0.01);
    if arc && points.len() <= 6 && points.iter().any(|p| near(p[0], 0.0, tol)) && points.iter().any(|p| near(p[0], w, tol)) {
        return (Kind::Ellipse, paths);
    }
    let ring: &[[f64; 2]] = if points.len() > 1 && near(points[0][0], points[points.len() - 1][0], tol) && near(points[0][1], points[points.len() - 1][1], tol) { &points[..points.len() - 1] } else { points };
    if ring.len() == 4 && (closed || ring.len() != points.len()) {
        let corners = ring.iter().all(|p| (near(p[0], 0.0, tol) || near(p[0], w, tol)) && (near(p[1], 0.0, tol) || near(p[1], h, tol)));
        if corners {
            return (Kind::Rectangle, paths);
        }
        let mids = ring.iter().all(|p| (near(p[0], w / 2.0, tol) && (near(p[1], 0.0, tol) || near(p[1], h, tol))) || (near(p[1], h / 2.0, tol) && (near(p[0], 0.0, tol) || near(p[0], w, tol))));
        if mids {
            return (Kind::Diamond, paths);
        }
    }
    (Kind::Polyline, paths)
}

/// `/^#[0-9a-f]{6}$/i` colours only.
fn colour(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    (s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())).then(|| s.to_string())
}

fn paint(stroke: Option<&Value>, stroke_width: Option<&Value>, stroke_style: Option<&Value>, fill: Option<&Value>, fill_opacity: Option<&Value>) -> Map<String, Value> {
    let dash = stroke_style.and_then(Value::as_str).unwrap_or("");
    let sw = stroke_width.and_then(Value::as_f64);
    let width = match sw {
        Some(n) if n != 0.0 && !n.is_nan() => n,
        _ => 1.0,
    };
    let has_pat1_digit = dash.match_indices("pat1").any(|(i, _)| dash[i + 4..].chars().next().is_some_and(|c| c.is_ascii_digit()));
    let style = if ["pat2", "pat4", "pat5", "pat6", "pat7", "pat8"].iter().any(|p| dash.contains(p)) {
        "dashed"
    } else if dash.contains("pat3") || dash.contains("pat9") || has_pat1_digit {
        "dotted"
    } else {
        "solid"
    };
    let mut stroke_colour = colour(stroke).unwrap_or_else(|| "#000000".to_string());
    if colour(stroke).is_none() || sw == Some(0.0) {
        stroke_colour = if sw == Some(0.0) { "transparent".to_string() } else { stroke_colour };
    }
    let mut out = Map::new();
    out.insert("strokeColor".into(), json!(stroke_colour));
    out.insert("backgroundColor".into(), json!(colour(fill).unwrap_or_else(|| "transparent".to_string())));
    out.insert("strokeWidth".into(), json!(js_round(width).max(1.0)));
    out.insert("strokeStyle".into(), json!(style));
    if let Some(o) = fill_opacity.and_then(Value::as_f64) {
        if o < 1.0 {
            out.insert("opacity".into(), json!(js_round(o * 100.0)));
        }
    }
    out
}

struct Builder {
    seed: i64,
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    fn base(&mut self, kind: &str, id: &str, x: f64, y: f64, w: f64, h: f64, more: Map<String, Value>) -> Map<String, Value> {
        self.seed += 1;
        let seed = self.seed;
        self.seed += 1;
        let nonce = self.seed;
        let mut m = Map::new();
        for (k, v) in [
            ("id", json!(id)),
            ("type", json!(kind)),
            ("x", json!(x)),
            ("y", json!(y)),
            ("width", json!(w)),
            ("height", json!(h)),
            ("angle", json!(0)),
            ("strokeColor", json!("#1e1e1e")),
            ("backgroundColor", json!("transparent")),
            ("fillStyle", json!("solid")),
            ("strokeWidth", json!(2)),
            ("strokeStyle", json!("solid")),
            ("roughness", json!(0)),
            ("opacity", json!(100)),
            ("groupIds", json!([])),
            ("frameId", Value::Null),
            ("roundness", Value::Null),
            ("seed", json!(seed)),
            ("version", json!(1)),
            ("versionNonce", json!(nonce)),
            ("isDeleted", json!(false)),
            ("boundElements", Value::Null),
            ("updated", json!(1)),
            ("link", Value::Null),
            ("locked", json!(false)),
        ] {
            m.insert(k.to_string(), v);
        }
        for (k, v) in more {
            m.insert(k, v);
        }
        m
    }

    #[allow(clippy::too_many_arguments)]
    fn text_for(&mut self, s: &Value, container: Option<&str>, x: f64, y: f64, w: f64, h: f64) -> Map<String, Value> {
        let label = text_of(s, "label");
        let mut more = Map::new();
        more.insert("strokeColor".into(), json!(colour(s.get("textColor")).unwrap_or_else(|| "#000000".to_string())));
        more.insert("strokeWidth".into(), json!(1));
        more.insert("text".into(), json!(label));
        more.insert("originalText".into(), json!(label));
        more.insert("fontSize".into(), json!(js_round(num_or(s, "fontSize", 13.33) * 10.0) / 10.0));
        more.insert("fontFamily".into(), json!(family(text_of(s, "fontFamily"))));
        more.insert("textAlign".into(), json!(match text_of(s, "align") { "left" => "left", "right" => "right", _ => "center" }));
        more.insert("verticalAlign".into(), json!(if container.is_some() { "middle" } else { "top" }));
        more.insert("containerId".into(), container.map_or(Value::Null, |c| json!(c)));
        more.insert("lineHeight".into(), json!(1.25));
        more.insert("autoResize".into(), json!(true));
        more.insert("baseline".into(), json!(0));
        self.base("text", &format!("{}-t", id_string(s)), x, y, w, h, more)
    }
}

fn estimate(s: &Value) -> (f64, f64) {
    let label = text_of(s, "label");
    let fs = num_or(s, "fontSize", 13.33);
    let lines: Vec<&str> = label.split('\n').collect();
    let longest = lines.iter().map(|l| l.encode_utf16().count()).max().unwrap_or(0) as f64;
    (longest * fs * 0.55, lines.len() as f64 * fs * 1.25)
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

struct Ref {
    id: String,
    index: usize,
    shape: Value,
}

/// The scene of page `page_index` of `model` (`exportExcalidraw`); `page_svg` is the page's plain SVG (only read for pictures).
pub fn export_scene(model: &Value, page_index: usize, page_svg: &dyn Fn() -> String) -> Result<Value, String> {
    let page = model["pages"].get(page_index).ok_or("There is no such page to export")?;
    let shapes: &[Value] = page["shapes"].as_array().map_or(&[], Vec::as_slice);
    let mut b = Builder { seed: 1000 };
    let mut elements: Vec<Value> = Vec::new();
    let mut by_shape: std::collections::HashMap<String, Ref> = Default::default();

    for s in shapes {
        if text_of(s, "type") != "custom" || s["parts"].as_array().is_none_or(Vec::is_empty) {
            continue;
        }
        let vid = id_string(s);
        let id = format!("s{vid}");
        let (kind, paths) = classify(s["parts"].as_array().unwrap(), num(s, "w"), num(s, "h"));
        let angle = (num(s, "rot") * std::f64::consts::PI) / 180.0;
        let style = paint(s.get("stroke"), s.get("strokeWidth"), s.get("strokeStyle"), s.get("fill"), s.get("fillOpacity"));
        let bg_clear = style["backgroundColor"] == json!("transparent");
        let has_look = !bg_clear || (style["strokeColor"] != json!("transparent") && s.get("strokeWidth").and_then(Value::as_f64) != Some(0.0));
        let (sx, sy, sw, sh) = (num(s, "x"), num(s, "y"), num(s, "w"), num(s, "h"));
        if matches!(kind, Kind::Rectangle | Kind::Ellipse | Kind::Diamond) {
            if !has_look && truthy(s, "label") {
                let (tw, th) = estimate(s);
                let mut t = b.text_for(s, None, sx + (sw - tw) / 2.0, sy + (sh - th) / 2.0, tw, th);
                t.insert("angle".into(), json!(angle));
                let tid = t["id"].as_str().unwrap().to_string();
                elements.push(Value::Object(t));
                by_shape.insert(vid, Ref { id: tid, index: elements.len() - 1, shape: s.clone() });
                continue;
            }
            let kind_name = match kind {
                Kind::Rectangle => "rectangle",
                Kind::Ellipse => "ellipse",
                _ => "diamond",
            };
            let mut more = style.clone();
            more.insert("angle".into(), json!(angle));
            more.insert("roundness".into(), if kind == Kind::Rectangle && num(s, "rounding") > 0.0 { json!({"type": 3}) } else { Value::Null });
            let el = b.base(kind_name, &id, sx, sy, sw, sh, more);
            elements.push(Value::Object(el));
            let el_index = elements.len() - 1;
            by_shape.insert(vid, Ref { id: id.clone(), index: el_index, shape: s.clone() });
            if truthy(s, "label") {
                let (tw, th) = estimate(s);
                let t = b.text_for(s, Some(&id), sx + (sw - tw) / 2.0, sy + (sh - th) / 2.0, tw, th);
                let tid = t["id"].as_str().unwrap().to_string();
                elements.push(Value::Object(t));
                elements[el_index]["boundElements"] = json!([{"id": tid, "type": "text"}]);
            }
        } else {
            for (k, path) in paths.iter().enumerate() {
                let pts = &path.points;
                if pts.len() < 2 {
                    continue;
                }
                let x0 = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
                let y0 = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                let x1 = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
                let y1 = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
                let closed = path.closed && !bg_clear;
                let mut more = style.clone();
                more.insert("backgroundColor".into(), if path.closed { style["backgroundColor"].clone() } else { json!("transparent") });
                more.insert("angle".into(), json!(angle));
                let mut points: Vec<Value> = pts.iter().map(|p| json!([p[0] - pts[0][0], p[1] - pts[0][1]])).collect();
                if path.closed && pts.len() > 2 && (pts[0][0] != pts[pts.len() - 1][0] || pts[0][1] != pts[pts.len() - 1][1]) {
                    points.push(json!([0, 0]));
                }
                more.insert("points".into(), Value::Array(points));
                for key in ["lastCommittedPoint", "startBinding", "endBinding", "startArrowhead", "endArrowhead"] {
                    more.insert(key.into(), Value::Null);
                }
                if closed {
                    more.insert("polygon".into(), json!(true));
                }
                let line = b.base("line", &format!("{id}-{k}"), sx + pts[0][0], sy + pts[0][1], x1 - x0, y1 - y0, more);
                let lid = line["id"].as_str().unwrap().to_string();
                elements.push(Value::Object(line));
                if k == 0 {
                    by_shape.insert(vid.clone(), Ref { id: lid, index: elements.len() - 1, shape: s.clone() });
                }
            }
            if truthy(s, "label") {
                let (tw, th) = estimate(s);
                let t = b.text_for(s, None, sx + (sw - tw) / 2.0, sy + (sh - th) / 2.0, tw, th);
                elements.push(Value::Object(t));
            }
        }
    }

    // pictures: an `image` element and an entry of the scene's `files` with the picture as a data: URL (taken from the page SVG)
    let mut files = Map::new();
    let image_shapes: Vec<&Value> = shapes.iter().filter(|s| text_of(s, "type") == "image" && truthy(s, "image") && !truthy(s, "group")).collect();
    let svg = if image_shapes.is_empty() { String::new() } else { page_svg() };
    for s in image_shapes {
        let vid = id_string(s);
        let Some(at) = svg.find(&format!("<g data-vid=\"{vid}\"")) else { continue };
        let end = svg[at..].find("</g>").map_or(svg.len(), |e| at + e);
        let slice = &svg[at..end];
        let Some(h) = slice.find("href=\"data:image/") else { continue };
        let rest = &slice[h + 17..];
        let Some(semi) = rest.find(";base64,") else { continue };
        let kind = &rest[..semi];
        if !["png", "jpeg", "jpg", "gif", "bmp"].contains(&kind) {
            continue;
        }
        let data_start = semi + 8;
        let Some(quote) = rest[data_start..].find('"') else { continue };
        if quote == 0 {
            continue;
        }
        let data = &rest[data_start..data_start + quote];
        let type_ = if kind == "jpg" { "jpeg" } else { kind };
        let file_id = format!("file{vid}");
        files.insert(file_id.clone(), json!({"mimeType": format!("image/{type_}"), "id": file_id, "dataURL": format!("data:image/{type_};base64,{data}"), "created": 1}));
        let mut more = Map::new();
        more.insert("angle".into(), json!((num(s, "rot") * std::f64::consts::PI) / 180.0));
        more.insert("fileId".into(), json!(file_id));
        more.insert("status".into(), json!("saved"));
        more.insert("scale".into(), json!([1, 1]));
        let el = b.base("image", &format!("s{vid}"), num(s, "x"), num(s, "y"), num(s, "w"), num(s, "h"), more);
        let eid = el["id"].as_str().unwrap().to_string();
        elements.push(Value::Object(el));
        by_shape.insert(vid, Ref { id: eid, index: elements.len() - 1, shape: s.clone() });
    }

    // glued connectors become arrows bound at both ends; the route's bends are the arrow's inner points
    let end_id = |e: Option<&Value>| -> String {
        let raw = match e.and_then(|e| e.get("shapeId")) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        raw.replacen("shape:visio:", "", 1)
    };
    for c in page["connectors"].as_array().map_or(&[][..], Vec::as_slice) {
        let (from_key, to_key) = (end_id(c.get("from")), end_id(c.get("to")));
        if !by_shape.contains_key(&from_key) || !by_shape.contains_key(&to_key) {
            continue;
        }
        let edge = |end: Option<&Value>, key: &str| -> [f64; 2] {
            let bx = &by_shape[key].shape;
            let local = end.and_then(|e| e.get("local"));
            let lx = local.and_then(|l| l.get("x")).and_then(Value::as_f64).unwrap_or(0.0);
            let ly = local.and_then(|l| l.get("y")).and_then(Value::as_f64).unwrap_or(0.0);
            [num(bx, "x") + lx * num(bx, "w"), num(bx, "y") + num(bx, "h") * (1.0 - ly)]
        };
        let p0 = edge(c.get("from"), &from_key);
        let p1 = edge(c.get("to"), &to_key);
        let mut all = vec![p0];
        for w in c["waypoints"].as_array().map_or(&[][..], Vec::as_slice) {
            all.push([num(w, "x"), num(w, "y")]);
        }
        all.push(p1);
        let x0 = all.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let y0 = all.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let x1 = all.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        let y1 = all.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        let mut more = paint(c.get("stroke"), c.get("strokeWidth"), c.get("strokeStyle"), Some(&json!("none")), None);
        more.insert("points".into(), Value::Array(all.iter().map(|p| json!([p[0] - p0[0], p[1] - p0[1]])).collect()));
        more.insert("lastCommittedPoint".into(), Value::Null);
        more.insert("startBinding".into(), json!({"elementId": by_shape[&from_key].id, "focus": 0, "gap": 1}));
        more.insert("endBinding".into(), json!({"elementId": by_shape[&to_key].id, "focus": 0, "gap": 1}));
        more.insert("startArrowhead".into(), arrow_end(c.get("arrowStart").and_then(Value::as_str)));
        more.insert("endArrowhead".into(), arrow_end(c.get("arrowEnd").and_then(Value::as_str)));
        more.insert("roundness".into(), if all.len() > 2 { Value::Null } else { json!({"type": 2}) });
        let arrow = b.base("arrow", &format!("c{}", id_string(c)), p0[0], p0[1], x1 - x0, y1 - y0, more);
        let aid = arrow["id"].as_str().unwrap().to_string();
        elements.push(Value::Object(arrow));
        for key in [&from_key, &to_key] {
            let index = by_shape[key].index;
            let mut list = elements[index]["boundElements"].as_array().cloned().unwrap_or_default();
            list.push(json!({"id": aid, "type": "arrow"}));
            elements[index]["boundElements"] = Value::Array(list);
        }
    }
    let _ = is_js_space;
    let _ = obj;
    Ok(json!({"type": "excalidraw", "version": 2, "source": "LayoutCraft", "elements": elements, "appState": {"gridSize": null, "viewBackgroundColor": "#ffffff"}, "files": files}))
}

impl Engine {
    /// File > Export > Excalidraw: page `page` as an Excalidraw scene (the text of the .excalidraw file, 2-space indented JSON like `JSON.stringify(scene, null, 2)`).
    pub fn export_excalidraw(&self, page: usize) -> Result<Value, String> {
        let model: Value = serde_json::from_str(&self.json().ok_or("no document")?).map_err(|e| e.to_string())?;
        export_scene(&model, page, &|| self.svg(page, false).and_then(Result::ok).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_flattened_and_closed_rectangles_are_recognised() {
        let paths = flatten_path("M0 0 L40 0 L40 20 L0 20 Z");
        assert_eq!(paths.len(), 1);
        assert!(paths[0].closed && paths[0].points.len() == 4);
        let part = json!({"d": "M0 0 L40 0 L40 20 L0 20 Z"});
        assert_eq!(classify(&[part], 40.0, 20.0).0, Kind::Rectangle);
        let diamond = json!({"d": "M20 0 L40 10 L20 20 L0 10 Z"});
        assert_eq!(classify(&[diamond], 40.0, 20.0).0, Kind::Diamond);
        let curve = flatten_path("M0 0 C10 0 20 10 30 10");
        assert_eq!(curve[0].points.len(), 13);
    }

    #[test]
    fn numbers_follow_the_regex_of_the_web_code() {
        assert_eq!(numbers("10.5-3.25e-2 .5 7."), [10.5, -3.25e-2, 0.5, 7.0]);
        assert_eq!(numbers("-"), Vec::<f64>::new());
    }

    #[test]
    fn a_labelled_filled_rectangle_gets_a_bound_text_and_seeds_count_up() {
        let model = json!({"pages": [{"shapes": [
            {"visioId": "1", "type": "custom", "x": 10, "y": 20, "w": 40, "h": 20, "rot": 0, "fill": "#ff0000", "stroke": "#000000", "strokeWidth": 1.0, "label": "A", "fontSize": 12, "parts": [{"d": "M0 0 L40 0 L40 20 L0 20 Z"}]},
            {"visioId": "2", "type": "custom", "x": 100, "y": 20, "w": 40, "h": 20, "rot": 0, "fill": "#00ff00", "stroke": "#000000", "strokeWidth": 1.0, "label": "", "parts": [{"d": "M0 0 L40 0 L40 20 L0 20 Z"}]}
        ], "connectors": [{"visioId": "9", "from": {"shapeId": "shape:visio:1"}, "to": {"shapeId": "shape:visio:2"}, "waypoints": [], "arrowEnd": "filled"}]}]});
        let scene = export_scene(&model, 0, &String::new).unwrap();
        let els = scene["elements"].as_array().unwrap();
        assert_eq!(els.iter().map(|e| e["id"].as_str().unwrap()).collect::<Vec<_>>(), ["s1", "1-t", "s2", "c9"]);
        assert_eq!(els[0]["boundElements"], json!([{"id": "1-t", "type": "text"}, {"id": "c9", "type": "arrow"}]));
        assert_eq!(els[0]["seed"], json!(1001));
        assert_eq!(els[0]["versionNonce"], json!(1002));
        assert_eq!(els[3]["endArrowhead"], json!("arrow"));
        assert_eq!(els[3]["roundness"], json!({"type": 2}));
        assert!(export_scene(&model, 3, &String::new).is_err());
    }
}
