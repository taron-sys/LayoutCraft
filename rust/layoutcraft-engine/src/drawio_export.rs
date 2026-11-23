//! File > Export > draw.io: a Rust port of `web/drawio-export.js` (docs/34, step N4.8a). Every page of the open drawing becomes one `<diagram>` of an uncompressed
//! .drawio file, built from the document model. Same text as the web (`tests/drawio_export_oracle.rs` compares them on 145 drawings).
use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::drawio_shapes::{shape_geometry, KNOWN_SHAPES};
use crate::dxf::hypot;
use crate::excalidraw_export::{classify, Kind, Outline};
use crate::svgutil::{is_js_space, js_round, js_string};
use crate::Engine;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn num(n: f64) -> String {
    js_string(js_round(n * 100.0) / 100.0)
}

fn colour(v: Option<&Value>) -> Option<&str> {
    let s = v?.as_str()?;
    (s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())).then_some(s)
}

fn f(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn truthy(v: &Value, key: &str) -> bool {
    match v.get(key) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn id_of(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => js_string(n.as_f64().unwrap_or(f64::NAN)),
        _ => "undefined".to_string(),
    }
}

/// `String(e?.shapeId || '').replace('shape:visio:', '')`.
fn end_id(e: Option<&Value>) -> String {
    let raw = match e.and_then(|e| e.get("shapeId")) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => js_string(n.as_f64().unwrap_or(f64::NAN)),
        _ => String::new(),
    };
    raw.replacen("shape:visio:", "", 1)
}

/// Visio's arrowhead numbers back to draw.io names: (name, filled).
fn from_index(i: i64) -> Option<(&'static str, bool)> {
    Some(match i {
        1 | 3 | 7 | 12 => ("open", true),
        2 | 4 | 13 => ("block", true),
        5 | 6 | 8 => ("classic", true),
        9 | 24 => ("dash", true),
        10 => ("oval", true),
        11 => ("box", true),
        14..=16 => ("block", false),
        17..=19 => ("classic", false),
        20 => ("oval", false),
        21 => ("box", false),
        22 | 34 => ("diamond", false),
        23 => ("cross", true),
        25 | 26 => ("ERmandOne", true),
        27 => ("ERmany", true),
        28 => ("ERoneToMany", true),
        29 | 32 | 33 => ("ERzeroToMany", true),
        30 | 31 | 35 => ("ERzeroToOne", true),
        _ => return None,
    })
}

fn size_px(size: i64) -> Option<i64> {
    Some(match size {
        0 => 3,
        1 => 4,
        2 => 6,
        3 => 10,
        4 => 14,
        5 => 18,
        6 => 24,
        _ => return None,
    })
}

/// The draw.io arrow styles of a shape / connector from the engine's own numbers, None when the style info cannot be read.
fn arrow_styles(engine: &Engine, page: usize, id: &str) -> Option<String> {
    let info: Value = serde_json::from_str(&engine.style_info_json(page, id).ok()?).ok()?;
    let one = |index: Option<i64>, size: Option<i64>, prefix: &str| -> String {
        match index {
            None | Some(0) => format!("{prefix}Arrow=none;"),
            Some(i) => {
                let (name, fill) = from_index(i).unwrap_or(("classic", true));
                format!("{prefix}Arrow={name};{}{prefix}Size={};", if fill { String::new() } else { format!("{prefix}Fill=0;") }, size_px(size.unwrap_or(2)).unwrap_or(6))
            }
        }
    };
    let n = |k: &str| info.get(k).and_then(Value::as_f64).map(|x| x as i64);
    Some(format!("{}{}", one(n("endArrow"), n("endArrowSize"), "end"), one(n("beginArrow"), n("beginArrowSize"), "start")))
}

fn arrow_name(v: Option<&str>) -> &'static str {
    match v {
        Some("filled") => "classic",
        Some("open") => "open",
        Some("triangle") => "block",
        Some("dot") | Some("circle") => "oval",
        Some("diamond") => "diamond",
        Some("bar") => "dash",
        _ => "none",
    }
}

/// `/pat[2-9]/` (and with `pat1\d` when `teens`).
fn dashed(style: &str, teens: bool) -> bool {
    let d = style.as_bytes();
    (0..d.len().saturating_sub(3)).any(|i| {
        &d[i..i + 3] == b"pat" && d.get(i + 3).is_some_and(|c| (b'2'..=b'9').contains(c) || (teens && *c == b'1' && d.get(i + 4).is_some_and(u8::is_ascii_digit)))
    })
}

fn style_of(s: &Value, extra: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !extra.is_empty() {
        parts.push(extra.to_string());
    }
    parts.push("whiteSpace=wrap".into());
    parts.push("html=1".into());
    let fill = colour(s.get("fill"));
    parts.push(fill.map_or("fillColor=none".to_string(), |c| format!("fillColor={c}")));
    let line = colour(s.get("stroke"));
    let sw = s.get("strokeWidth").and_then(Value::as_f64);
    parts.push(if sw == Some(0.0) || line.is_none() { "strokeColor=none".to_string() } else { format!("strokeColor={}", line.unwrap()) });
    if sw.is_some_and(|w| w > 0.0) {
        parts.push(format!("strokeWidth={}", num(sw.unwrap())));
    }
    if dashed(text(s, "strokeStyle"), true) {
        parts.push("dashed=1".into());
    }
    if let Some(o) = s.get("fillOpacity").and_then(Value::as_f64) {
        if o < 1.0 {
            parts.push(format!("opacity={}", js_string(js_round(o * 100.0))));
        }
    }
    if truthy(s, "rot") {
        parts.push(format!("rotation={}", num(f(s, "rot"))));
    }
    if truthy(s, "fontSize") {
        parts.push(format!("fontSize={}", js_string(1.0f64.max(js_round(f(s, "fontSize"))))));
    }
    if let Some(c) = colour(s.get("textColor")) {
        parts.push(format!("fontColor={c}"));
    }
    if truthy(s, "fontFamily") {
        parts.push(format!("fontFamily={}", text(s, "fontFamily").replace([';', '='], "")));
    }
    let bits = (truthy(s, "bold") as i32) | ((truthy(s, "italic") as i32) << 1) | ((truthy(s, "underline") as i32) << 2);
    if bits != 0 {
        parts.push(format!("fontStyle={bits}"));
    }
    format!("{};", parts.join(";"))
}

fn label(s: &Value) -> String {
    esc(&text(s, "label").replace("\r\n", "<br>").replace('\n', "<br>"))
}

fn bbox_of(pts: &[[f64; 2]]) -> [f64; 4] {
    let xs = pts.iter().map(|p| p[0]);
    let ys = pts.iter().map(|p| p[1]);
    [xs.clone().fold(f64::INFINITY, f64::min), ys.clone().fold(f64::INFINITY, f64::min), xs.fold(f64::NEG_INFINITY, f64::max), ys.fold(f64::NEG_INFINITY, f64::max)]
}

/// The draw.io shape (a name of drawio-shapes) whose outline and inner lines are those of `main` and the group members `others`.
fn recognize(main: &Value, kind: &Kind, paths: &[Outline], others: &[&Value]) -> Result<Option<&'static str>, String> {
    let pts: Option<Vec<[f64; 2]>> = if *kind == Kind::Rectangle {
        None
    } else {
        let q = paths.first().ok_or("Cannot read properties of undefined (reading 'points')")?;
        let mut pts = q.points.clone();
        if pts.len() > 2 && hypot(pts[0][0] - pts[pts.len() - 1][0], pts[0][1] - pts[pts.len() - 1][1]) < 0.5 {
            pts.pop();
        }
        Some(pts)
    };
    if *kind == Kind::Polyline && !(paths.len() == 1 && paths[0].closed) {
        return Ok(None);
    }
    let (mx, my, mw, mh) = (f(main, "x"), f(main, "y"), f(main, "w"), f(main, "h"));
    for name in KNOWN_SHAPES {
        if matches!(name, "swimlane" | "bpmnGateway" | "bpmnEvent") {
            continue;
        }
        let Some(g) = shape_geometry(name, &HashMap::new(), mw, mh) else { continue };
        if *kind == Kind::Rectangle {
            if g.outline.len() != 4 || !g.outline.iter().all(|q| (q[0].abs() < 0.5 || (q[0] - mw).abs() < 0.5) && (q[1].abs() < 0.5 || (q[1] - mh).abs() < 0.5)) {
                continue;
            }
        } else {
            let Some(pts) = &pts else { continue };
            if g.outline.len() != pts.len() || g.outline.iter().enumerate().any(|(i, q)| hypot(q[0] - pts[i][0], q[1] - pts[i][1]) > 0.75) {
                continue;
            }
        }
        // the inner lines: one other member for each, with the same box
        let mut rest: Vec<&Value> = others.to_vec();
        let mut ok = true;
        for d in &g.details {
            let [x0, y0, x1, y1] = bbox_of(&d.pts);
            let at = rest.iter().position(|o| (f(o, "x") - (mx + x0)).abs() < 1.2 && (f(o, "y") - (my + y0)).abs() < 1.2 && (f(o, "w") - (x1 - x0).max(2.0)).abs() < 1.2 && (f(o, "h") - (y1 - y0).max(2.0)).abs() < 1.2);
            match at {
                Some(i) => {
                    rest.remove(i);
                }
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || !rest.is_empty() {
            continue;
        }
        return Ok(Some(name));
    }
    Ok(None)
}

fn custom(s: &Value) -> bool {
    text(s, "type") == "custom" && s.get("parts").and_then(Value::as_array).is_some_and(|p| !p.is_empty())
}

fn diagram(engine: &Engine, page: &Value, size: &Value, page_index: usize) -> Result<String, String> {
    let shapes: &[Value] = page["shapes"].as_array().map_or(&[], Vec::as_slice);
    let mut cells: Vec<String> = vec!["<mxCell id=\"0\"/>".to_string()];
    let mut ids: HashSet<String> = HashSet::new();
    let mut recog: HashMap<String, &'static str> = HashMap::new();
    let mut dropped: HashSet<String> = HashSet::new();
    let mut group_main: HashMap<String, String> = HashMap::new();
    let mut group_order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&Value>> = HashMap::new();
    for s in shapes {
        if let Some(root) = s.get("group").and_then(|g| g.get("rootId")).filter(|r| !r.is_null() && *r != &Value::String(String::new())) {
            let key = match root {
                Value::String(r) => r.clone(),
                other => other.to_string(),
            };
            if !groups.contains_key(&key) {
                group_order.push(key.clone());
            }
            groups.entry(key).or_default().push(s);
        }
    }
    for root in &group_order {
        let mut ms: Vec<&Value> = groups[root].iter().copied().filter(|s| custom(s)).collect();
        ms.sort_by(|a, b| (f(b, "w") * f(b, "h")).partial_cmp(&(f(a, "w") * f(a, "h"))).unwrap_or(std::cmp::Ordering::Equal));
        if ms.is_empty() {
            continue;
        }
        let main = ms[0];
        let (kind, paths) = classify(main["parts"].as_array().unwrap(), f(main, "w"), f(main, "h"));
        if let Some(name) = recognize(main, &kind, &paths, &ms[1..])? {
            recog.insert(id_of(main, "visioId"), name);
            group_main.insert(root.clone(), id_of(main, "visioId"));
            for o in &ms[1..] {
                dropped.insert(id_of(o, "visioId"));
            }
        }
    }
    for s in shapes {
        if !truthy(s, "group") && custom(s) {
            let (kind, paths) = classify(s["parts"].as_array().unwrap(), f(s, "w"), f(s, "h"));
            if kind == Kind::Polyline {
                if let Some(name) = recognize(s, &kind, &paths, &[])? {
                    recog.insert(id_of(s, "visioId"), name);
                }
            }
        }
    }
    for s in shapes {
        if !custom(s) || dropped.contains(&id_of(s, "visioId")) {
            continue;
        }
        let (kind, paths) = classify(s["parts"].as_array().unwrap(), f(s, "w"), f(s, "h"));
        let vid = id_of(s, "visioId");
        let id = format!("s{vid}");
        let geometry = format!("<mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/>", num(f(s, "x")), num(f(s, "y")), num(f(s, "w")), num(f(s, "h")));
        if let Some(name) = recog.get(&vid) {
            cells.push(format!("<mxCell id=\"{id}\" value=\"{}\" style=\"{}\" vertex=\"1\" parent=\"1\">{geometry}</mxCell>", label(s), esc(&style_of(s, &format!("shape={name}")))));
            ids.insert(vid);
            continue;
        }
        if matches!(kind, Kind::Rectangle | Kind::Ellipse | Kind::Diamond) {
            let extra = match kind {
                Kind::Ellipse => "ellipse",
                Kind::Diamond => "rhombus",
                _ => {
                    if f(s, "rounding") > 0.0 { "rounded=1" } else { "rounded=0" }
                }
            };
            let sw = s.get("strokeWidth").and_then(Value::as_f64);
            let text_only = colour(s.get("fill")).is_none() && (sw == Some(0.0) || colour(s.get("stroke")).is_none()) && truthy(s, "label");
            cells.push(format!("<mxCell id=\"{id}\" value=\"{}\" style=\"{}\" vertex=\"1\" parent=\"1\">{geometry}</mxCell>", label(s), esc(&style_of(s, if text_only { "text" } else { extra }))));
            ids.insert(vid);
        } else {
            for (k, path) in paths.iter().enumerate() {
                if path.points.len() < 2 {
                    continue;
                }
                let pts: Vec<[f64; 2]> = path.points.iter().map(|p| [f(s, "x") + p[0], f(s, "y") + p[1]]).collect();
                let mid: String = pts[1..pts.len() - 1].iter().map(|p| format!("<mxPoint x=\"{}\" y=\"{}\"/>", num(p[0]), num(p[1]))).collect();
                let (first, last) = (pts[0], pts[pts.len() - 1]);
                let own: Option<String> = if path.closed { None } else { arrow_styles(engine, page_index, &vid) };
                let has_arrow = own.as_deref().is_some_and(|o| o.match_indices("Arrow=").any(|(i, _)| !o[i + 6..].starts_with("none")));
                let extra = if has_arrow { own.clone().unwrap() } else { "endArrow=none;endFill=0;startArrow=none".to_string() };
                cells.push(format!(
                    "<mxCell id=\"{id}-{k}\" value=\"{}\" style=\"{}\" edge=\"1\" parent=\"1\"><mxGeometry relative=\"1\" as=\"geometry\"><mxPoint x=\"{}\" y=\"{}\" as=\"sourcePoint\"/><mxPoint x=\"{}\" y=\"{}\" as=\"targetPoint\"/>{}</mxGeometry></mxCell>",
                    if k == 0 { label(s) } else { String::new() },
                    esc(&style_of(s, &extra)),
                    num(first[0]),
                    num(first[1]),
                    num(last[0]),
                    num(last[1]),
                    if mid.is_empty() { String::new() } else { format!("<Array as=\"points\">{mid}</Array>") }
                ));
            }
        }
    }
    // pictures: shape=image with the picture as a data: URI in the style
    let image_shapes: Vec<&Value> = shapes.iter().filter(|s| text(s, "type") == "image" && truthy(s, "image") && !truthy(s, "group")).collect();
    let page_svg = if image_shapes.is_empty() { String::new() } else { engine.svg(page_index, false).and_then(Result::ok).unwrap_or_default() };
    for s in image_shapes {
        let vid = id_of(s, "visioId");
        let Some(at) = page_svg.find(&format!("<g data-vid=\"{vid}\"")) else { continue };
        let end = page_svg[at..].find("</g>").map_or(page_svg.len().saturating_sub(1), |e| at + e);
        let slice = page_svg.get(at..end.max(at)).unwrap_or("");
        let mut found: Option<(String, String)> = None;
        for (h, _) in slice.match_indices("href=\"data:image/") {
            let rest = &slice[h + 17..];
            let kind_len = rest.bytes().take_while(|b| b.is_ascii_lowercase() || matches!(b, b'+' | b'.' | b'-')).count();
            if kind_len == 0 || !rest[kind_len..].starts_with(";base64,") {
                continue;
            }
            let data = &rest[kind_len + 8..];
            let Some(q) = data.find('"') else { continue };
            if q == 0 {
                continue;
            }
            found = Some((rest[..kind_len].to_string(), data[..q].to_string()));
            break;
        }
        let Some((kind, data)) = found else { continue };
        cells.push(format!(
            "<mxCell id=\"s{vid}\" value=\"{}\" style=\"{}\" vertex=\"1\" parent=\"1\"><mxGeometry x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" as=\"geometry\"/></mxCell>",
            label(s),
            esc(&format!("shape=image;verticalLabelPosition=bottom;labelBackgroundColor=default;verticalAlign=top;aspect=fixed;imageAspect=0;image=data:image/{kind},{data};")),
            num(f(s, "x")),
            num(f(s, "y")),
            num(f(s, "w")),
            num(f(s, "h"))
        ));
        ids.insert(vid);
    }
    let connectors: &[Value] = page["connectors"].as_array().map_or(&[], Vec::as_slice);
    for c in connectors {
        let end_of = |e: Option<&Value>| -> String {
            let raw = end_id(e);
            group_main.get(&raw).cloned().unwrap_or(raw)
        };
        let (from, to) = (end_of(c.get("from")), end_of(c.get("to")));
        if !ids.contains(&from) || !ids.contains(&to) {
            continue;
        }
        let pts: String = c["waypoints"].as_array().map_or(&[][..], Vec::as_slice).iter().map(|w| format!("<mxPoint x=\"{}\" y=\"{}\"/>", num(f(w, "x")), num(f(w, "y")))).collect();
        let own = arrow_styles(engine, page_index, &id_of(c, "visioId"));
        let own = own.unwrap_or_else(|| format!("endArrow={};startArrow={};", arrow_name(c.get("arrowEnd").and_then(Value::as_str)), arrow_name(c.get("arrowStart").and_then(Value::as_str))));
        let width = {
            let w = f(c, "strokeWidth");
            if w == 0.0 || w.is_nan() { 1.0 } else { w }
        };
        let st = format!("{own}html=1;strokeColor={};strokeWidth={};{}", colour(c.get("stroke")).unwrap_or("#000000"), num(width), if dashed(text(c, "strokeStyle"), false) { "dashed=1;" } else { "" });
        cells.push(format!(
            "<mxCell id=\"c{}\" value=\"{}\" style=\"{}\" edge=\"1\" parent=\"1\" source=\"s{from}\" target=\"s{to}\"><mxGeometry relative=\"1\" as=\"geometry\">{}</mxGeometry></mxCell>",
            id_of(c, "visioId"),
            esc(text(c, "label")),
            esc(&st),
            if pts.is_empty() { String::new() } else { format!("<Array as=\"points\">{pts}</Array>") }
        ));
    }
    // layers: layer cells (parent 0); a shape cell sits on the layer of its first membership, an edge on the layer of the shape it starts at
    let layers: &[Value] = page["layers"].as_array().map_or(&[], Vec::as_slice);
    let layer_of_shape = |vid: &str| -> Option<String> {
        let s = shapes.iter().find(|q| id_of(q, "visioId") == vid)?;
        let k = s.get("layers").and_then(Value::as_array).and_then(|l| l.first()).and_then(Value::as_u64)? as usize;
        layers.get(k).map(|_| format!("L{}", k + 1))
    };
    if !layers.is_empty() {
        let defs: Vec<String> = layers
            .iter()
            .enumerate()
            .map(|(k, l)| {
                format!(
                    "<mxCell id=\"L{}\" value=\"{}\" parent=\"0\"{}{}/>",
                    k + 1,
                    esc(&{
                        let n = text(l, "name");
                        if n.is_empty() { format!("Layer {}", k + 1) } else { n.to_string() }
                    }),
                    if l.get("visible") == Some(&Value::Bool(false)) { " visible=\"0\"" } else { "" },
                    if truthy(l, "locked") { " style=\"locked=1;\"" } else { "" }
                )
            })
            .collect();
        for i in 1..cells.len() {
            // /^<mxCell id="(?:s(\d+)(?:-\d+)?|c(\d+))"/
            let Some(rest) = cells[i].strip_prefix("<mxCell id=\"") else { continue };
            let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
            let (shape_vid, conn_vid): (Option<String>, Option<String>) = if let Some(r) = rest.strip_prefix('s') {
                let n = digits(r);
                let after = &r[n..];
                let ok = n > 0 && (after.starts_with('"') || (after.starts_with('-') && digits(&after[1..]) > 0 && after[1 + digits(&after[1..])..].starts_with('"')));
                (ok.then(|| r[..n].to_string()), None)
            } else if let Some(r) = rest.strip_prefix('c') {
                let n = digits(r);
                (None, (n > 0 && r[n..].starts_with('"')).then(|| r[..n].to_string()))
            } else {
                (None, None)
            };
            let layer = if let Some(v) = shape_vid {
                layer_of_shape(&v)
            } else if let Some(v) = conn_vid {
                connectors.iter().find(|q| id_of(q, "visioId") == v).and_then(|c| layer_of_shape(&end_id(c.get("from"))))
            } else {
                continue;
            };
            if let Some(layer) = layer {
                cells[i] = cells[i].replacen(" parent=\"1\"", &format!(" parent=\"{layer}\""), 1);
            }
        }
        for (n, d) in defs.into_iter().enumerate() {
            cells.insert(1 + n, d);
        }
    }
    if layers.is_empty() || cells.iter().any(|c| c.contains(" parent=\"1\"")) {
        cells.insert(1 + layers.len(), "<mxCell id=\"1\" parent=\"0\"/>".to_string());
    }
    let _ = is_js_space;
    Ok(format!(
        "<mxGraphModel dx=\"0\" dy=\"0\" grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"1\" pageScale=\"1\" pageWidth=\"{}\" pageHeight=\"{}\" math=\"0\" shadow=\"0\"><root>{}</root></mxGraphModel>",
        js_string(js_round(f(size, "w") * 96.0)),
        js_string(js_round(f(size, "h") * 96.0)),
        cells.join("")
    ))
}

impl Engine {
    /// File > Export > draw.io: the whole drawing as an uncompressed .drawio file (text).
    pub fn export_drawio(&self) -> Result<String, String> {
        let model: Value = serde_json::from_str(&self.json().ok_or("no document")?).map_err(|e| e.to_string())?;
        let names: Value = serde_json::from_str(&self.page_names_json()?).map_err(|e| e.to_string())?;
        let mut body = String::new();
        for (i, p) in model["pages"].as_array().map_or(&[][..], Vec::as_slice).iter().enumerate() {
            let size: Value = serde_json::from_str(&self.page_size_json(i)?).map_err(|e| e.to_string())?;
            let name = names.get(i).and_then(Value::as_str).filter(|n| !n.is_empty()).or_else(|| p.get("name").and_then(Value::as_str).filter(|n| !n.is_empty())).map_or_else(|| format!("Page-{}", i + 1), str::to_string);
            body.push_str(&format!("<diagram id=\"page{}\" name=\"{}\">{}</diagram>", i + 1, esc(&name), diagram(self, p, &size, i)?));
        }
        Ok(format!("<mxfile host=\"LayoutCraft\" version=\"1\">{body}</mxfile>"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_styles_are_written_like_the_web() {
        assert_eq!(num(12.3456), "12.35");
        assert_eq!(num(-0.001), "0");
        let s = serde_json::json!({"fill": "#ff0000", "stroke": "#000000", "strokeWidth": 2.0, "strokeStyle": "pat3", "fillOpacity": 0.5, "rot": 30.0, "fontSize": 12.4, "textColor": "#111111", "fontFamily": "Segoe; UI=x", "bold": true, "italic": true});
        assert_eq!(style_of(&s, "rounded=1"), "rounded=1;whiteSpace=wrap;html=1;fillColor=#ff0000;strokeColor=#000000;strokeWidth=2;dashed=1;opacity=50;rotation=30;fontSize=12;fontColor=#111111;fontFamily=Segoe UIx;fontStyle=3;");
        assert_eq!(style_of(&serde_json::json!({"strokeWidth": 0.0}), ""), "whiteSpace=wrap;html=1;fillColor=none;strokeColor=none;");
    }

    #[test]
    fn dashes_and_arrows() {
        assert!(dashed("pat2", false) && dashed("pat9", false) && !dashed("pat1", true) && dashed("pat12", true) && !dashed("pat12", false));
        assert_eq!(from_index(5), Some(("classic", true)));
        assert_eq!(from_index(14), Some(("block", false)));
        assert_eq!(from_index(99), None);
        assert_eq!(esc("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
    }
}
