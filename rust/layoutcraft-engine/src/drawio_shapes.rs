//! draw.io shape fidelity: a Rust port of `web/drawio-shapes.js` (docs/34, step N4.8). The outlines of the draw.io built-in shapes the engine has no native shape for,
//! written from plain geometry. Used by the draw.io export (recognising shapes the importer drew) and, later, the import.
use std::collections::HashMap;

use crate::dxf::hypot;

const PI: f64 = std::f64::consts::PI;

/// The inner line of a shape (a cylinder's lid, a document's fold ...).
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub pts: Vec<[f64; 2]>,
    pub closed: bool,
}

/// The outline (closed, px, y down, inside 0..w x 0..h) and the inner lines of a shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeGeometry {
    pub outline: Vec<[f64; 2]>,
    pub details: Vec<Detail>,
    pub invisible_main: bool,
    pub hr: Option<f64>,
}

/// The shapes this module draws, in the order of the web's `KNOWN_SHAPES` set.
pub const KNOWN_SHAPES: [&str; 38] = [
    "parallelogram", "trapezoid", "trapezoidDown", "hexagon", "step", "process", "cylinder3", "cloud", "document", "note", "card", "folder", "tape", "actor", "umlActor", "delay", "display", "cube", "cross",
    "star", "pentagon", "octagon", "singleArrow", "doubleArrow", "callout", "terminator", "manualInput", "internalStorage", "offPageConnector", "loopLimit", "dataStorage", "sumEllipse", "orEllipse", "swimlane",
    "component", "umlLifeline", "bpmnGateway", "bpmnEvent",
];

/// The flowchart / basic stencil names of draw.io mapped to the base shapes.
pub fn canonical_name(name: &str) -> &str {
    match name {
        "mxgraph.flowchart.decision" => "rhombus",
        "mxgraph.flowchart.terminator" => "terminator",
        "mxgraph.flowchart.process" => "rect",
        "mxgraph.flowchart.alternate_process" => "rounded",
        "mxgraph.flowchart.document" | "mxgraph.flowchart.multi-document" => "document",
        "mxgraph.flowchart.data" => "parallelogram",
        "mxgraph.flowchart.predefined_process" => "process",
        "mxgraph.flowchart.database" => "cylinder3",
        "mxgraph.flowchart.stored_data" => "dataStorage",
        "mxgraph.flowchart.manual_input" => "manualInput",
        "mxgraph.flowchart.preparation" => "hexagon",
        "mxgraph.flowchart.start_1" => "terminator",
        "mxgraph.flowchart.start_2" | "mxgraph.flowchart.on-page_reference" => "ellipse",
        "mxgraph.flowchart.off-page_reference" => "offPageConnector",
        "mxgraph.flowchart.delay" => "delay",
        "mxgraph.flowchart.display" => "display",
        "mxgraph.flowchart.summing_function" => "sumEllipse",
        "mxgraph.flowchart.or" => "orEllipse",
        "mxgraph.flowchart.internal_storage" => "internalStorage",
        "mxgraph.flowchart.loop_limit" => "loopLimit",
        "mxgraph.flowchart.paper_tape" => "tape",
        "mxgraph.flowchart.manual_operation" => "trapezoidDown",
        "mxgraph.basic.star" => "star",
        "mxgraph.basic.pentagon" => "pentagon",
        "mxgraph.basic.octagon2" => "octagon",
        "mxgraph.basic.cross2" => "cross",
        "mxgraph.basic.cube" => "cube",
        "mxgraph.basic.document" => "document",
        "mxgraph.bpmn.gateway2" => "bpmnGateway",
        "mxgraph.bpmn.event" => "bpmnEvent",
        "mxgraph.bpmn.task" | "mxgraph.bpmn.activity" => "rounded",
        "lifeline" => "umlLifeline",
        "mxgraph.basic.cloud_callout" => "cloud",
        "umlActor" => "umlActor",
        "actor" => "actor",
        "cylinder" | "datastore" => "cylinder3",
        "plus" => "cross",
        "hexagon" => "hexagon",
        other => other,
    }
}

fn arc(cx: f64, cy: f64, rx: f64, ry: f64, a0: f64, a1: f64, n: usize) -> Vec<[f64; 2]> {
    (0..=n)
        .map(|k| {
            let t = a0 + ((a1 - a0) * k as f64) / n as f64;
            [cx + rx * t.cos(), cy + ry * t.sin()]
        })
        .collect()
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    hi.min(lo.max(v))
}

/// `num(v, d)`: `parseFloat` when finite, else the default.
fn num(style: &HashMap<String, String>, key: &str, d: f64) -> f64 {
    match style.get(key) {
        Some(v) => {
            let n = crate::svgutil::js_parse_float(v);
            if n.is_finite() { n } else { d }
        }
        None => d,
    }
}

/// Scale a point list to fill exactly 0..w x 0..h.
fn fit(points: &[[f64; 2]], w: f64, h: f64) -> Vec<[f64; 2]> {
    let x0 = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let x1 = points.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let y0 = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let y1 = points.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let sx = if x1 > x0 { w / (x1 - x0) } else { 1.0 };
    let sy = if y1 > y0 { h / (y1 - y0) } else { 1.0 };
    points.iter().map(|p| [(p[0] - x0) * sx, (p[1] - y0) * sy]).collect()
}

fn line(pts: &[[f64; 2]]) -> Detail {
    Detail { pts: pts.to_vec(), closed: false }
}

fn geom(outline: Vec<[f64; 2]>, details: Vec<Detail>) -> ShapeGeometry {
    ShapeGeometry { outline, details, invisible_main: false, hr: None }
}

/// `shapeGeometry(rawName, style, w, h)`.
pub fn shape_geometry(raw_name: &str, style: &HashMap<String, String>, w: f64, h: f64) -> Option<ShapeGeometry> {
    let name = canonical_name(raw_name);
    if !KNOWN_SHAPES.contains(&name) || !(w > 0.0) || !(h > 0.0) {
        return None;
    }
    let rect = vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
    let frac = |key: &str, d: f64, max: f64| clamp(num(style, key, d), 0.0, max);
    let px = |key: &str, d: f64, max: f64| clamp(num(style, key, d), 0.0, max);
    let fixed = style.get("fixedSize").map(String::as_str) == Some("1");
    Some(match name {
        "parallelogram" => {
            let s = if fixed { px("size", 20.0, w * 0.9) } else { frac("size", 0.25, 0.5) * w };
            geom(vec![[s, 0.0], [w, 0.0], [w - s, h], [0.0, h]], vec![])
        }
        "trapezoid" => {
            let s = if fixed { px("size", 20.0, w / 2.0) } else { frac("size", 0.25, 0.5) * w };
            geom(vec![[s, 0.0], [w - s, 0.0], [w, h], [0.0, h]], vec![])
        }
        "trapezoidDown" => {
            let s = frac("size", 0.25, 0.5) * w;
            geom(vec![[0.0, 0.0], [w, 0.0], [w - s, h], [s, h]], vec![])
        }
        "hexagon" => {
            let s = if fixed { px("size", 20.0, w / 2.0) } else { frac("size", 0.25, 0.5) * w };
            geom(vec![[s, 0.0], [w - s, 0.0], [w, h / 2.0], [w - s, h], [s, h], [0.0, h / 2.0]], vec![])
        }
        "step" => {
            let s = if fixed { px("size", 20.0, w * 0.9) } else { frac("size", 0.25, 0.5) * w };
            geom(vec![[0.0, 0.0], [w - s, 0.0], [w, h / 2.0], [w - s, h], [0.0, h], [s, h / 2.0]], vec![])
        }
        "process" => {
            let s = frac("size", 0.125, 0.5) * w;
            geom(rect, vec![line(&[[s, 0.0], [s, h]]), line(&[[w - s, 0.0], [w - s, h]])])
        }
        "internalStorage" => {
            let dx = frac("dx", 0.15, 0.5) * w;
            let dy = frac("dy", 0.15, 0.5) * h;
            geom(rect, vec![line(&[[dx, 0.0], [dx, h]]), line(&[[0.0, dy], [w, dy]])])
        }
        "cylinder3" => {
            let e = clamp(px("size", 15.0, h / 2.0), 1.0, h / 2.0);
            let top = arc(w / 2.0, e, w / 2.0, e, PI, 2.0 * PI, 24);
            let bottom = arc(w / 2.0, h - e, w / 2.0, e, 0.0, PI, 24);
            let mut outline = top;
            outline.extend(bottom);
            geom(outline, vec![line(&arc(w / 2.0, e, w / 2.0, e, 0.0, PI, 24))])
        }
        "cloud" => {
            let n = 8usize;
            let base: Vec<[f64; 2]> = (0..n).map(|k| [((2.0 * PI * k as f64) / n as f64).cos() * 0.8, ((2.0 * PI * k as f64) / n as f64).sin() * 0.62]).collect();
            let mut pts: Vec<[f64; 2]> = Vec::new();
            for k in 0..n {
                let (a, b) = (base[k], base[(k + 1) % n]);
                let (mx, my) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
                let r = hypot(b[0] - a[0], b[1] - a[1]) / 2.0;
                let a0 = (a[1] - my).atan2(a[0] - mx);
                let mut sweep = (b[1] - my).atan2(b[0] - mx) - a0;
                while sweep <= 0.0 {
                    sweep += 2.0 * PI;
                }
                let mid = a0 + sweep / 2.0;
                if mid.cos() * mx + mid.sin() * my < 0.0 {
                    sweep -= 2.0 * PI;
                }
                let part = arc(mx, my, r, r, a0, a0 + sweep, 10);
                pts.extend_from_slice(&part[..part.len() - 1]);
            }
            geom(fit(&pts, w, h), vec![])
        }
        "document" => {
            let f = h * clamp(num(style, "size", 0.15), 0.0, 0.3);
            let mut outline = vec![[0.0, 0.0], [w, 0.0]];
            for k in 0..=24 {
                let t = k as f64 / 24.0;
                outline.push([w * (1.0 - t), h - f + (t * 2.0 * PI).sin() * -f * 0.9 + f * 0.1]);
            }
            geom(outline, vec![])
        }
        "tape" => {
            let f = h * clamp(num(style, "size", 0.4), 0.0, 0.5) / 2.0;
            let mut pts = Vec::new();
            for k in 0..=24 {
                let t = k as f64 / 24.0;
                pts.push([w * t, f + (t * 2.0 * PI).sin() * f * 0.9]);
            }
            for k in (0..=24).rev() {
                let t = k as f64 / 24.0;
                pts.push([w * t, h - f + (t * 2.0 * PI).sin() * f * 0.9]);
            }
            geom(fit(&pts, w, h), vec![])
        }
        "note" => {
            let s = px("size", 30.0, w.min(h));
            geom(vec![[0.0, 0.0], [w - s, 0.0], [w, s], [w, h], [0.0, h]], vec![line(&[[w - s, 0.0], [w - s, s], [w, s]])])
        }
        "card" => {
            let s = px("size", 30.0, w.min(h));
            geom(vec![[s, 0.0], [w, 0.0], [w, h], [0.0, h], [0.0, s]], vec![])
        }
        "folder" => {
            let tw = px("tabWidth", 60.0, w * 0.9);
            let th = px("tabHeight", 20.0, h * 0.5);
            geom(vec![[0.0, 0.0], [tw, 0.0], [tw + th * 0.5, th], [w, th], [w, h], [0.0, h]], vec![])
        }
        "cube" => {
            let s = px("size", 20.0, w.min(h) * 0.9);
            geom(vec![[s, 0.0], [w, 0.0], [w, h - s], [w - s, h], [0.0, h], [0.0, s]], vec![line(&[[0.0, s], [w - s, s], [w, 0.0]]), line(&[[w - s, s], [w - s, h]])])
        }
        "delay" => {
            let mut outline = vec![[0.0, 0.0]];
            outline.extend(arc(w - h / 2.0, h / 2.0, (h / 2.0).min(w), h / 2.0, -PI / 2.0, PI / 2.0, 24).into_iter().map(|p| [p[0].min(w), p[1]]));
            outline.push([0.0, h]);
            geom(outline, vec![])
        }
        "display" => {
            let s = (h / 2.0).min(w / 3.0);
            let mut outline = vec![[0.0, h / 2.0], [s, 0.0]];
            outline.extend(arc(w - s, h / 2.0, s, h / 2.0, -PI / 2.0, PI / 2.0, 18));
            outline.push([s, h]);
            geom(outline, vec![])
        }
        "terminator" => {
            let r = (h / 2.0).min(w / 2.0);
            let mut outline = arc(w - r, h / 2.0, r, h / 2.0, -PI / 2.0, PI / 2.0, 18);
            outline.extend(arc(r, h / 2.0, r, h / 2.0, PI / 2.0, (3.0 * PI) / 2.0, 18));
            geom(outline, vec![])
        }
        "manualInput" => {
            let s = h * 0.2;
            geom(vec![[0.0, s], [w, 0.0], [w, h], [0.0, h]], vec![])
        }
        "offPageConnector" => geom(vec![[0.0, 0.0], [w, 0.0], [w, h * 0.75], [w / 2.0, h], [0.0, h * 0.75]], vec![]),
        "loopLimit" => {
            let s = px("size", 20.0, w.min(h) / 2.0);
            geom(vec![[s, 0.0], [w - s, 0.0], [w, s], [w, h], [0.0, h], [0.0, s]], vec![])
        }
        "dataStorage" => {
            let sd = frac("size", 0.12, 0.5) * w;
            let mut pts = vec![[sd, 0.0], [w, 0.0]];
            for k in 1..=12 {
                let t = k as f64 / 12.0;
                pts.push([w - sd * (PI * t).sin(), h * t]);
            }
            pts.push([sd, h]);
            for k in (1..=11).rev() {
                let t = k as f64 / 12.0;
                pts.push([sd - sd * (PI * t).sin(), h * t]);
            }
            geom(pts, vec![])
        }
        "cross" => {
            let t = clamp(num(style, "size", 0.33), 0.1, 0.9);
            let a = (w * (1.0 - t)) / 2.0;
            let b = (h * (1.0 - t)) / 2.0;
            geom(vec![[a, 0.0], [w - a, 0.0], [w - a, b], [w, b], [w, h - b], [w - a, h - b], [w - a, h], [a, h], [a, h - b], [0.0, h - b], [0.0, b], [a, b]], vec![])
        }
        "star" => {
            let mut pts = Vec::new();
            for k in 0..10 {
                let r = if k % 2 == 0 { 1.0 } else { clamp(num(style, "size", 0.38), 0.2, 0.8) };
                let t = -PI / 2.0 + (PI * k as f64) / 5.0;
                pts.push([t.cos() * r, t.sin() * r]);
            }
            geom(fit(&pts, w, h), vec![])
        }
        "pentagon" => {
            let pts: Vec<[f64; 2]> = (0..5).map(|k| [(-PI / 2.0 + (2.0 * PI * k as f64) / 5.0).cos(), (-PI / 2.0 + (2.0 * PI * k as f64) / 5.0).sin()]).collect();
            geom(fit(&pts, w, h), vec![])
        }
        "octagon" => {
            let s = 0.29;
            geom(vec![[w * s, 0.0], [w * (1.0 - s), 0.0], [w, h * s], [w, h * (1.0 - s)], [w * (1.0 - s), h], [w * s, h], [0.0, h * (1.0 - s)], [0.0, h * s]], vec![])
        }
        "singleArrow" | "doubleArrow" => {
            let aw = clamp(num(style, "arrowWidth", 0.5), 0.1, 1.0) * h;
            let hl = (clamp(num(style, "arrowSize", 0.4), 0.05, 1.0) * h).min(if name == "doubleArrow" { w / 2.0 } else { w });
            let m = (h - aw) / 2.0;
            if name == "singleArrow" {
                geom(vec![[0.0, m], [w - hl, m], [w - hl, 0.0], [w, h / 2.0], [w - hl, h], [w - hl, h - m], [0.0, h - m]], vec![])
            } else {
                geom(vec![[0.0, h / 2.0], [hl, 0.0], [hl, m], [w - hl, m], [w - hl, 0.0], [w, h / 2.0], [w - hl, h], [w - hl, h - m], [hl, h - m], [hl, h]], vec![])
            }
        }
        "callout" => {
            let t = (h * 0.25).min(25.0);
            let tx = clamp(num(style, "position", 0.3), 0.1, 0.9) * w;
            geom(vec![[0.0, 0.0], [w, 0.0], [w, h - t], [tx + t, h - t], [tx - t * 0.3, h], [tx, h - t], [0.0, h - t]], vec![])
        }
        "actor" => {
            let hr = w.min(h) * 0.22;
            let unit = [[0.0, 1.0], [0.0, 0.62], [0.2, 0.5], [0.34, 0.46], [0.3, 0.36], [0.28, 0.2], [0.35, 0.05], [0.5, 0.0], [0.65, 0.05], [0.72, 0.2], [0.7, 0.36], [0.66, 0.46], [0.8, 0.5], [1.0, 0.62], [1.0, 1.0]];
            let scaled: Vec<[f64; 2]> = unit.iter().map(|p| [p[0] * w, p[1] * h]).collect();
            ShapeGeometry { outline: fit(&scaled, w, h), details: vec![], invisible_main: false, hr: Some(hr) }
        }
        "umlActor" => {
            let cx = w / 2.0;
            let hr = (w * 0.25).min(h * 0.12);
            ShapeGeometry {
                outline: rect,
                invisible_main: true,
                hr: None,
                details: vec![
                    Detail { pts: arc(cx, hr, hr, hr, 0.0, 2.0 * PI, 24), closed: true },
                    line(&[[cx, hr * 2.0], [cx, h * 0.62]]),
                    line(&[[0.0, h * 0.34], [w, h * 0.34]]),
                    line(&[[0.0, h], [cx, h * 0.62], [w, h]]),
                ],
            }
        }
        "sumEllipse" | "orEllipse" => {
            let mut el = arc(w / 2.0, h / 2.0, w / 2.0, h / 2.0, 0.0, 2.0 * PI, 48);
            el.pop();
            let (k, cx, cy) = (0.7071, w / 2.0, h / 2.0);
            let lines: Vec<[[f64; 2]; 2]> = if name == "orEllipse" {
                vec![[[cx, 0.0], [cx, h]], [[0.0, cy], [w, cy]]]
            } else {
                vec![[[cx - (w / 2.0) * k, cy - (h / 2.0) * k], [cx + (w / 2.0) * k, cy + (h / 2.0) * k]], [[cx - (w / 2.0) * k, cy + (h / 2.0) * k], [cx + (w / 2.0) * k, cy - (h / 2.0) * k]]]
            };
            geom(el, lines.iter().map(|l| line(l)).collect())
        }
        "component" => {
            let tw = 20.0f64.min(w / 4.0);
            let th = 10.0f64.min(h / 5.0);
            let tab = |cy: f64| Detail { pts: vec![[0.0, cy - th / 2.0], [tw, cy - th / 2.0], [tw, cy + th / 2.0], [0.0, cy + th / 2.0]], closed: true };
            geom(rect, vec![tab(h * 0.3), tab(h * 0.7)])
        }
        "umlLifeline" => {
            let hs = clamp(num(style, "size", 40.0), 8.0, h);
            let cx = w / 2.0;
            geom(vec![[0.0, 0.0], [w, 0.0], [w, hs], [cx, hs], [cx, h], [cx, hs], [0.0, hs]], vec![])
        }
        "bpmnGateway" => {
            let (cx, cy, kx, ky) = (w / 2.0, h / 2.0, w * 0.17, h * 0.17);
            let sym = style.get("symbol").map(String::as_str).unwrap_or("");
            let ring = |r: f64| Detail { pts: arc(cx, cy, w * r, h * r, 0.0, 2.0 * PI, 32), closed: true };
            let x = vec![line(&[[cx - kx, cy - ky], [cx + kx, cy + ky]]), line(&[[cx - kx, cy + ky], [cx + kx, cy - ky]])];
            let plus = vec![line(&[[cx - kx * 1.4, cy], [cx + kx * 1.4, cy]]), line(&[[cx, cy - ky * 1.4], [cx, cy + ky * 1.4]])];
            let details = match sym {
                "parallelGw" => plus,
                "inclusiveGw" => vec![ring(0.22)],
                "eventGw" => vec![ring(0.24), ring(0.18)],
                "complexGw" => x.into_iter().chain(plus).collect(),
                _ => x,
            };
            geom(vec![[cx, 0.0], [w, cy], [cx, h], [0.0, cy]], details)
        }
        "bpmnEvent" => {
            let kind = style.get("outline").map(String::as_str).filter(|k| !k.is_empty()).unwrap_or("standard");
            let (cx, cy) = (w / 2.0, h / 2.0);
            let ring = |k: f64| Detail { pts: arc(cx, cy, (w / 2.0) * k, (h / 2.0) * k, 0.0, 2.0 * PI, 32), closed: true };
            let mut details = Vec::new();
            if ["catching", "throwing", "boundInt", "boundNonint", "eventNonint"].contains(&kind) {
                details.push(ring(0.82));
            }
            if kind == "end" {
                details.push(ring(0.9));
                details.push(ring(0.8));
            }
            let mut outline = arc(cx, cy, w / 2.0, h / 2.0, 0.0, 2.0 * PI, 48);
            outline.pop();
            geom(outline, details)
        }
        "swimlane" => {
            let hs = clamp(num(style, "startSize", 23.0), 8.0, h * 0.9);
            geom(rect, vec![line(&[[0.0, hs], [w, hs]])])
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parallelogram_is_four_corners_and_a_cylinder_has_a_lid() {
        let g = shape_geometry("parallelogram", &HashMap::new(), 100.0, 40.0).unwrap();
        assert_eq!(g.outline, vec![[25.0, 0.0], [100.0, 0.0], [75.0, 40.0], [0.0, 40.0]]);
        let c = shape_geometry("cylinder", &HashMap::new(), 80.0, 100.0).unwrap();
        assert_eq!(c.outline.len(), 50);
        assert_eq!(c.details.len(), 1);
        assert!(shape_geometry("nothing", &HashMap::new(), 10.0, 10.0).is_none());
        assert!(shape_geometry("star", &HashMap::new(), 0.0, 10.0).is_none());
    }

    #[test]
    fn styles_change_the_proportions() {
        let mut style = HashMap::new();
        style.insert("size".to_string(), "10".to_string());
        style.insert("fixedSize".to_string(), "1".to_string());
        let g = shape_geometry("mxgraph.flowchart.data", &style, 100.0, 40.0).unwrap();
        assert_eq!(g.outline[0], [10.0, 0.0]);
        let process = shape_geometry("process", &HashMap::new(), 80.0, 40.0).unwrap();
        assert_eq!(process.details.len(), 2);
        assert_eq!(process.details[0].pts, vec![[10.0, 0.0], [10.0, 40.0]]);
    }

    #[test]
    fn every_known_shape_has_a_geometry() {
        for name in KNOWN_SHAPES {
            assert!(shape_geometry(name, &HashMap::new(), 90.0, 60.0).is_some(), "{name}");
        }
    }
}
