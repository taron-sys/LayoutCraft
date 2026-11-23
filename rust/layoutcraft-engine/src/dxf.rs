//! File > Export > DXF: a Rust port of `web/svgdxf.js` (docs/34, step N4.5). The page's SVG (the engine's own drawing) becomes an ASCII DXF of release 12 (AC1009):
//! outlines as POLYLINE / VERTEX / SEQEND, circles as CIRCLE, text as TEXT; inches, y up, layers from the Visio layers. The same numbers and texts as the web
//! (`tests/dxf_oracle.rs` compares them on every example drawing).
use std::collections::HashMap;

use serde_json::Value;

use crate::svgutil::*;
use crate::Engine;

/// The result of the export (`svgToDxf`): the text, the counts, the DXF layers used and the extents.
#[derive(Debug, Clone, PartialEq)]
pub struct Dxf {
    pub dxf: String,
    pub polylines: usize,
    pub circles: usize,
    pub texts: usize,
    pub layers: Vec<String>,
    pub extents: [f64; 4],
}

/// The options of `svgToDxf`.
#[derive(Debug, Clone, Default)]
pub struct DxfOptions {
    pub px_per_inch: Option<f64>,
    pub height_in: Option<f64>,
    /// {shape id (the SVG's data-vid): layer name}
    pub layers: HashMap<String, String>,
    /// {layer name: 1 frozen | 4 locked}
    pub layer_flags: HashMap<String, i64>,
}

/// `f`: a number rounded to 1e-6, written as JavaScript writes it.
fn f(n: f64) -> String {
    js_string(js_round(n * 1e6) / 1e6)
}

fn aci([r, g, b]: [f64; 3]) -> i32 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max - min < 0.2 {
        return if max > 0.85 || max < 0.12 { 7 } else { 8 };
    }
    let d = max - min;
    let mut h = if max == r { ((g - b) / d) % 6.0 } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    h *= 60.0;
    let hue = if h < 0.0 { h + 360.0 } else { h };
    let slots: [(f64, i32); 7] = [(0.0, 1), (60.0, 2), (120.0, 3), (180.0, 4), (240.0, 5), (300.0, 6), (360.0, 1)];
    let mut best = slots[0];
    for s in slots {
        if (s.0 - hue).abs() < (best.0 - hue).abs() {
            best = s;
        }
    }
    best.1
}

struct Sub {
    pts: Vec<(f64, f64)>,
    closed: bool,
}

struct Flat {
    subs: Vec<Sub>,
    cur: Option<usize>,
    x: f64,
    y: f64,
    lc: Option<(f64, f64)>,
    lq: Option<(f64, f64)>,
}

impl Flat {
    fn add(&mut self, px: f64, py: f64) {
        let at = match self.cur {
            Some(at) => at,
            None => {
                self.subs.push(Sub { pts: vec![(self.x, self.y)], closed: false });
                self.cur = Some(self.subs.len() - 1);
                self.subs.len() - 1
            }
        };
        self.subs[at].pts.push((px, py));
        self.x = px;
        self.y = py;
    }

    fn cubic(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        let (x0, y0) = (self.x, self.y);
        for k in 1..=16 {
            let t = k as f64 / 16.0;
            let u = 1.0 - t;
            self.add(
                u * u * u * x0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x3,
                u * u * u * y0 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y3,
            );
        }
        self.lc = Some((x2, y2));
        self.lq = None;
    }
}

fn args_of(up: char) -> Option<usize> {
    Some(match up {
        'M' | 'L' | 'T' => 2,
        'H' | 'V' => 1,
        'C' => 6,
        'S' | 'Q' => 4,
        'A' => 7,
        'Z' => 0,
        _ => return None,
    })
}

/// Flatten SVG path data into subpaths (`flattenPath`).
fn flatten_path(d: &str) -> Result<Vec<Sub>, Unsupported> {
    let tokens = path_tokens(d);
    let mut st = Flat { subs: Vec::new(), cur: None, x: 0.0, y: 0.0, lc: None, lq: None };
    let (mut i, mut cmd) = (0usize, String::new());
    let (mut sx, mut sy) = (0.0, 0.0);
    while i < tokens.len() {
        if tokens[i].chars().any(|c| c.is_ascii_alphabetic()) {
            cmd = tokens[i].clone();
            i += 1;
        } else if cmd == "M" {
            cmd = "L".into();
        } else if cmd == "m" {
            cmd = "l".into();
        }
        let up_s = cmd.to_uppercase();
        let rel = cmd != up_s;
        let single = up_s.chars().next().filter(|_| up_s.chars().count() == 1);
        let Some(up) = single else { return unsupported(format!("path command {cmd}")) };
        let Some(k) = args_of(up) else { return unsupported(format!("path command {cmd}")) };
        let a: Vec<f64> = tokens[i.min(tokens.len())..(i + k).min(tokens.len())].iter().map(|t| js_number(t)).collect();
        if a.len() < k || a.iter().any(|t| !t.is_finite()) {
            return unsupported("path data");
        }
        i += k;
        let (ox, oy) = if rel { (st.x, st.y) } else { (0.0, 0.0) };
        match up {
            'M' => {
                st.x = a[0] + ox;
                st.y = a[1] + oy;
                sx = st.x;
                sy = st.y;
                st.cur = None;
                st.lc = None;
                st.lq = None;
            }
            'L' => {
                st.add(a[0] + ox, a[1] + oy);
                st.lc = None;
                st.lq = None;
            }
            'H' => {
                let y = st.y;
                st.add(a[0] + ox, y);
                st.lc = None;
                st.lq = None;
            }
            'V' => {
                let x = st.x;
                st.add(x, a[0] + oy);
                st.lc = None;
                st.lq = None;
            }
            'C' => st.cubic(a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy, a[4] + ox, a[5] + oy),
            'S' => {
                let c1 = match st.lc {
                    Some(lc) => (2.0 * st.x - lc.0, 2.0 * st.y - lc.1),
                    None => (st.x, st.y),
                };
                st.cubic(c1.0, c1.1, a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy);
            }
            'Q' | 'T' => {
                let q = if up == 'Q' {
                    (a[0] + ox, a[1] + oy)
                } else {
                    match st.lq {
                        Some(lq) => (2.0 * st.x - lq.0, 2.0 * st.y - lq.1),
                        None => (st.x, st.y),
                    }
                };
                let e = if up == 'Q' { (a[2] + ox, a[3] + oy) } else { (a[0] + ox, a[1] + oy) };
                let (x, y) = (st.x, st.y);
                st.cubic(x + (2.0 / 3.0) * (q.0 - x), y + (2.0 / 3.0) * (q.1 - y), e.0 + (2.0 / 3.0) * (q.0 - e.0), e.1 + (2.0 / 3.0) * (q.1 - e.1), e.0, e.1);
                st.lq = Some(q);
            }
            'A' => {
                let (ex, ey) = (a[5] + ox, a[6] + oy);
                for seg in arc_to_beziers(st.x, st.y, a[0], a[1], a[2], a[3] != 0.0, a[4] != 0.0, ex, ey) {
                    match seg {
                        Seg::Line(x, y) => st.add(x, y),
                        Seg::Cubic(a1, a2, a3, a4, a5, a6) => st.cubic(a1, a2, a3, a4, a5, a6),
                    }
                }
                st.add(ex, ey);
                st.lc = None;
                st.lq = None;
            }
            'Z' => {
                if let Some(at) = st.cur {
                    let sub = &mut st.subs[at];
                    sub.closed = true;
                    let (a0, z0) = (sub.pts[0], sub.pts[sub.pts.len() - 1]);
                    if sub.pts.len() > 2 && (a0.0 - z0.0).abs() < 1e-9 && (a0.1 - z0.1).abs() < 1e-9 {
                        sub.pts.pop();
                    }
                }
                st.x = sx;
                st.y = sy;
                st.cur = None;
                st.lc = None;
                st.lq = None;
            }
            _ => {}
        }
    }
    Ok(st.subs.into_iter().filter(|s| s.pts.len() >= 2).collect())
}

fn ellipse_points(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<(f64, f64)> {
    (0..48)
        .map(|k| {
            let t = (std::f64::consts::PI * 2.0 * k as f64) / 48.0;
            (cx + rx * t.cos(), cy + ry * t.sin())
        })
        .collect()
}

/// V8's `Math.hypot` for two arguments (scaled, so its last bits are those of JavaScript).
pub(crate) fn hypot(a: f64, b: f64) -> f64 {
    let (a, b) = (a.abs(), b.abs());
    let max = a.max(b);
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for v in [a, b] {
        let n = v / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

#[derive(Clone, Default)]
struct Style {
    fill: Option<String>,
    stroke: Option<String>,
    stroke_width: Option<String>,
    font_size: Option<String>,
    text_anchor: Option<String>,
    dxf_layer: Option<String>,
    dxf_layer_raw: Option<String>,
}

fn real(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|v| !v.is_empty() && *v != "none" && *v != "transparent")
}

struct Out<'a> {
    out: Vec<String>,
    box_: [f64; 4],
    polylines: usize,
    circles: usize,
    texts: usize,
    used: Vec<(String, i64)>,
    opts: &'a DxfOptions,
}

impl Out<'_> {
    fn put(&mut self, code: i32, value: impl ToString) {
        self.out.push(code.to_string());
        self.out.push(value.to_string());
    }
    fn grow(&mut self, x: f64, y: f64) {
        self.box_ = [self.box_[0].min(x), self.box_[1].min(y), self.box_[2].max(x), self.box_[3].max(y)];
    }
    fn layer_of(&mut self, st: &Style) -> String {
        let n = st.dxf_layer.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| "0".to_string());
        if n != "0" && !self.used.iter().any(|(u, _)| *u == n) {
            let flags = st.dxf_layer_raw.as_ref().and_then(|raw| self.opts.layer_flags.get(raw)).copied().unwrap_or(0);
            self.used.push((n.clone(), flags));
        }
        n
    }
    fn colour_of(st: &Style, outline: bool) -> i32 {
        let v: Option<&str> = if outline { real(&st.stroke).or(st.fill.as_deref()) } else { st.fill.as_deref() };
        match v {
            None | Some("") | Some("none") | Some("transparent") => 7,
            Some(v) if v.starts_with("url(") => 7,
            Some(v) => parse_colour(v).map(aci).unwrap_or(7),
        }
    }
    fn polyline(&mut self, pts: &[(f64, f64)], closed: bool, colour: i32, m: &Matrix, st: &Style) {
        if pts.len() < 2 {
            return;
        }
        let layer = self.layer_of(st);
        let sw = st.stroke_width.as_deref().map_or(0.0, js_number);
        let stroke_on = real(&st.stroke).is_some();
        let width = if stroke_on && sw.is_finite() && sw > 0.0 { sw * hypot(m[0], m[1]) } else { 0.0 };
        self.put(0, "POLYLINE");
        self.put(8, &layer);
        self.put(62, colour);
        self.put(66, 1);
        self.put(70, if closed { 1 } else { 0 });
        self.put(10, 0);
        self.put(20, 0);
        self.put(30, 0);
        if width > 0.0005 {
            self.put(40, f(width));
            self.put(41, f(width));
        }
        for (px, py) in pts {
            let x = px * m[0] + py * m[2] + m[4];
            let y = px * m[1] + py * m[3] + m[5];
            self.grow(x, y);
            self.put(0, "VERTEX");
            self.put(8, &layer);
            self.put(10, f(x));
            self.put(20, f(y));
            self.put(30, 0);
        }
        self.put(0, "SEQEND");
        self.put(8, &layer);
        self.polylines += 1;
    }
}

fn visible(st: &Style, can_fill: bool) -> bool {
    real(&st.stroke).is_some() || (can_fill && real(&st.fill).is_some())
}

fn dxf_name(name: &str) -> String {
    // `toUpperCase().replace(/[^A-Z0-9$_-]/g, '_').slice(0, 31)`: per UTF-16 unit (a character outside the BMP gives two underscores)
    let t: String = name
        .to_uppercase()
        .encode_utf16()
        .take(31)
        .map(|u| match u8::try_from(u) {
            Ok(b) if b.is_ascii_uppercase() || b.is_ascii_digit() || matches!(b, b'$' | b'_' | b'-') => b as char,
            _ => '_',
        })
        .collect();
    if t.is_empty() { "0".to_string() } else { t }
}

fn walk(o: &mut Out, node: &Node, parent: &Style, ctm: &Matrix) -> Result<(), Unsupported> {
    const SKIP: [&str; 12] = ["title", "desc", "defs", "linearGradient", "radialGradient", "stop", "filter", "clipPath", "mask", "image", "style", "pattern"];
    if SKIP.contains(&node.tag.as_str()) || node.tag == "marker" {
        return Ok(());
    }
    if node.attr("display") == Some("none") || node.attr("visibility") == Some("hidden") {
        return Ok(());
    }
    let mut st = parent.clone();
    for (k, slot) in [("fill", 0), ("stroke", 1), ("stroke-width", 2), ("font-size", 3), ("text-anchor", 4)] {
        if let Some(v) = node.attr(k) {
            let v = Some(v.to_string());
            match slot {
                0 => st.fill = v,
                1 => st.stroke = v,
                2 => st.stroke_width = v,
                3 => st.font_size = v,
                _ => st.text_anchor = v,
            }
        }
    }
    // (font-family is inherited by the web code as well but never read)
    if node.tag == "g" {
        if let Some(vid) = node.attr("data-vid") {
            if let Some(name) = o.opts.layers.get(vid).filter(|n| !n.is_empty()) {
                st.dxf_layer_raw = Some(name.clone());
                st.dxf_layer = Some(dxf_name(name));
            }
        }
    }
    let m = mul(&parse_transform(node.attr("transform"))?, ctm);
    let n = |k: &str, d: f64| -> f64 {
        let v = node.attr(k).map_or(d, js_number);
        if v.is_finite() { v } else { d }
    };
    match node.tag.as_str() {
        "svg" | "g" | "a" => {
            for c in node.elements() {
                walk(o, c, &st, &m)?;
            }
        }
        "rect" => {
            let (w, h) = (n("width", 0.0), n("height", 0.0));
            if w <= 0.0 || h <= 0.0 || !visible(&st, true) {
                return Ok(());
            }
            let (x, y) = (n("x", 0.0), n("y", 0.0));
            let colour = Out::colour_of(&st, true);
            o.polyline(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true, colour, &m, &st);
        }
        "path" => {
            if visible(&st, true) {
                for s in flatten_path(node.attr("d").unwrap_or(""))? {
                    let colour = Out::colour_of(&st, true);
                    o.polyline(&s.pts, s.closed, colour, &m, &st);
                }
            }
        }
        "polygon" | "polyline" => {
            let v: Vec<f64> = split_space_comma(node.attr("points").unwrap_or("")).iter().filter(|s| !s.is_empty()).map(|s| js_number(s)).collect();
            if v.len() < 4 || v.iter().any(|q| !q.is_finite()) || !visible(&st, node.tag == "polygon") {
                return Ok(());
            }
            let pts: Vec<(f64, f64)> = v.chunks_exact(2).map(|c| (c[0], c[1])).collect();
            let colour = Out::colour_of(&st, true);
            o.polyline(&pts, node.tag == "polygon", colour, &m, &st);
        }
        "line" => {
            if visible(&st, false) {
                let colour = Out::colour_of(&st, true);
                o.polyline(&[(n("x1", 0.0), n("y1", 0.0)), (n("x2", 0.0), n("y2", 0.0))], false, colour, &m, &st);
            }
        }
        "circle" | "ellipse" => {
            let (rx, ry) = if node.tag == "circle" { (n("r", 0.0), n("r", 0.0)) } else { (n("rx", 0.0), n("ry", 0.0)) };
            if rx <= 0.0 || ry <= 0.0 || !visible(&st, true) {
                return Ok(());
            }
            let (cx, cy) = (n("cx", 0.0), n("cy", 0.0));
            let (sxs, sys) = (hypot(m[0], m[1]), hypot(m[2], m[3]));
            let colour = Out::colour_of(&st, true);
            if rx == ry && (sxs - sys).abs() < 1e-9 * sxs {
                let x = cx * m[0] + cy * m[2] + m[4];
                let y = cx * m[1] + cy * m[3] + m[5];
                let layer = o.layer_of(&st);
                o.put(0, "CIRCLE");
                o.put(8, layer);
                o.put(62, colour);
                o.put(10, f(x));
                o.put(20, f(y));
                o.put(30, 0);
                o.put(40, f(rx * sxs));
                o.grow(x - rx * sxs, y - rx * sxs);
                o.grow(x + rx * sxs, y + rx * sxs);
                o.circles += 1;
            } else {
                o.polyline(&ellipse_points(cx, cy, rx, ry), true, colour, &m, &st);
            }
        }
        "text" => {
            let (x, y) = (n("x", 0.0), n("y", 0.0));
            let own = node.own_text();
            let spans: String = node.elements().filter(|c| c.tag == "tspan").map(|c| c.own_text()).collect::<Vec<_>>().join("");
            let replaced: String = format!("{own}{spans}").chars().map(|c| if matches!(c, '\t' | '\n' | '\r' | ' ') { ' ' } else { c }).collect();
            let content = replaced.trim_matches(is_js_space).to_string();
            if content.is_empty() || st.fill.as_deref().is_none_or(|f| f.is_empty() || f == "none") {
                return Ok(());
            }
            let size = st.font_size.as_deref().map_or(16.0, js_number);
            let px = x * m[0] + y * m[2] + m[4];
            let py = x * m[1] + y * m[3] + m[5];
            let scale = hypot(m[0], m[1]);
            let rotation = (m[1].atan2(m[0]) * 180.0) / std::f64::consts::PI;
            let anchor = st.text_anchor.clone().unwrap_or_else(|| "start".to_string());
            let height = {
                let v = size * scale;
                if v.is_nan() { v } else { v.max(0.01) }
            };
            let layer = o.layer_of(&st);
            let colour = Out::colour_of(&st, false);
            o.put(0, "TEXT");
            o.put(8, layer);
            o.put(62, colour);
            o.put(10, f(px));
            o.put(20, f(py));
            o.put(30, 0);
            o.put(40, f(height));
            // `replace(/[^\x20-\x7e]/g, '?')` works on UTF-16 units: a character outside the BMP gives two question marks
            let ascii: String = content.chars().map(|c| if (' '..='~').contains(&c) { c.to_string() } else { "?".repeat(c.len_utf16()) }).collect();
            o.put(1, ascii);
            o.put(50, f(rotation));
            o.put(7, "STANDARD");
            if anchor != "start" {
                o.put(72, if anchor == "middle" { 1 } else { 2 });
                o.put(11, f(px));
                o.put(21, f(py));
                o.put(31, 0);
            }
            o.grow(px, py);
            o.texts += 1;
        }
        _ => {}
    }
    Ok(())
}

/// `svgToDxf(svgText, options)`.
pub fn svg_to_dxf(svg_text: &str, opts: &DxfOptions) -> Result<Dxf, Unsupported> {
    let root = parse_xml(svg_text)?;
    let ppi = opts.px_per_inch.unwrap_or(96.0);
    let vb: Vec<f64> = split_space_comma(root.attr("viewBox").unwrap_or("")).iter().map(|s| js_number(s)).collect();
    if vb.len() != 4 || vb.iter().any(|v| !v.is_finite()) {
        return unsupported("viewBox");
    }
    let base: Matrix = [1.0 / ppi, 0.0, 0.0, -1.0 / ppi, 0.0, opts.height_in.unwrap_or((vb[1] + vb[3]) / ppi)];
    let mut o = Out { out: Vec::new(), box_: [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], polylines: 0, circles: 0, texts: 0, used: Vec::new(), opts };
    walk(&mut o, &root, &Style { fill: Some("#000000".to_string()), ..Default::default() }, &base)?;
    let mut box_ = o.box_;
    if !box_[0].is_finite() {
        box_ = [0.0, 0.0, vb[2] / ppi, vb[3] / ppi];
    }
    let mut head: Vec<String> = Vec::new();
    let mut h = |code: i32, value: String| {
        head.push(code.to_string());
        head.push(value);
    };
    h(0, "SECTION".into());
    h(2, "HEADER".into());
    h(9, "$ACADVER".into());
    h(1, "AC1009".into());
    h(9, "$MEASUREMENT".into());
    h(70, "0".into());
    h(9, "$INSUNITS".into());
    h(70, "1".into());
    h(9, "$EXTMIN".into());
    h(10, f(box_[0]));
    h(20, f(box_[1]));
    h(30, "0".into());
    h(9, "$EXTMAX".into());
    h(10, f(box_[2]));
    h(20, f(box_[3]));
    h(30, "0".into());
    h(0, "ENDSEC".into());
    h(0, "SECTION".into());
    h(2, "TABLES".into());
    h(0, "TABLE".into());
    h(2, "LTYPE".into());
    h(70, "1".into());
    h(0, "LTYPE".into());
    h(2, "CONTINUOUS".into());
    h(70, "0".into());
    h(3, "Solid line".into());
    h(72, "65".into());
    h(73, "0".into());
    h(40, "0".into());
    h(0, "ENDTAB".into());
    h(0, "TABLE".into());
    h(2, "LAYER".into());
    h(70, (1 + o.used.len()).to_string());
    h(0, "LAYER".into());
    h(2, "0".into());
    h(70, "0".into());
    h(62, "7".into());
    h(6, "CONTINUOUS".into());
    for (name, flags) in &o.used {
        h(0, "LAYER".into());
        h(2, name.clone());
        h(70, flags.to_string());
        h(62, "7".into());
        h(6, "CONTINUOUS".into());
    }
    h(0, "ENDTAB".into());
    h(0, "TABLE".into());
    h(2, "STYLE".into());
    h(70, "1".into());
    h(0, "STYLE".into());
    h(2, "STANDARD".into());
    h(70, "0".into());
    h(40, "0".into());
    h(41, "1".into());
    h(50, "0".into());
    h(71, "0".into());
    h(42, "0.2".into());
    h(3, "txt".into());
    h(4, String::new());
    h(0, "ENDTAB".into());
    h(0, "ENDSEC".into());
    h(0, "SECTION".into());
    h(2, "ENTITIES".into());
    let mut all = head;
    all.extend(o.out.iter().cloned());
    all.extend(["0", "ENDSEC", "0", "EOF"].map(String::from));
    Ok(Dxf { dxf: all.join("\r\n") + "\r\n", polylines: o.polylines, circles: o.circles, texts: o.texts, layers: o.used.iter().map(|(n, _)| n.clone()).collect(), extents: box_ })
}

impl Engine {
    /// File > Export > DXF for page `page`: the page's print SVG becomes a DXF; the Visio layers of the shapes become DXF layers (a shape on several layers goes
    /// on its first), hidden layers are frozen, locked ones locked.
    pub fn export_dxf(&self, page: usize) -> Result<Dxf, String> {
        let svg = self.svg(page, true).ok_or("no document")??;
        let size: Value = serde_json::from_str(&self.page_size_json(page)?).map_err(|e| e.to_string())?;
        let model: Value = serde_json::from_str(&self.json().ok_or("no document")?).map_err(|e| e.to_string())?;
        let page_model = &model["pages"][page];
        let names = page_model["layers"].as_array().cloned().unwrap_or_default();
        let mut opts = DxfOptions { px_per_inch: Some(96.0), height_in: size["h"].as_f64(), ..Default::default() };
        for sh in page_model["shapes"].as_array().map_or(&[][..], Vec::as_slice) {
            let first = sh["layers"].as_array().and_then(|l| l.first()).and_then(Value::as_u64);
            if let Some(layer) = first.and_then(|i| names.get(i as usize)) {
                let id = if sh.get("id").is_some_and(|v| !v.is_null()) { &sh["id"] } else { &sh["visioId"] };
                let key = match id {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if let Some(name) = layer["name"].as_str() {
                    opts.layers.insert(key, name.to_string());
                }
            }
        }
        for l in &names {
            if let Some(name) = l["name"].as_str() {
                let visible = l["visible"].as_bool().unwrap_or(true);
                let locked = l["locked"].as_bool().unwrap_or(false);
                opts.layer_flags.insert(name.to_string(), (if visible { 0 } else { 1 }) | (if locked { 4 } else { 0 }));
            }
        }
        svg_to_dxf(&svg, &opts).map_err(|e| e.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = "<svg viewBox=\"0 0 192 96\"><g data-vid=\"7\"><rect x=\"10\" y=\"10\" width=\"96\" height=\"48\" fill=\"#ff0000\" stroke=\"#000000\" stroke-width=\"2\"/><circle cx=\"150\" cy=\"48\" r=\"10\" fill=\"none\" stroke=\"blue\"/><text x=\"20\" y=\"40\" font-size=\"12\" fill=\"#000\" text-anchor=\"middle\">Hi \u{e9}</text></g></svg>";

    #[test]
    fn a_rect_a_circle_and_a_text_become_entities_on_the_layer_of_their_shape() {
        let mut opts = DxfOptions { height_in: Some(1.0), ..Default::default() };
        opts.layers.insert("7".into(), "Net work".into());
        opts.layer_flags.insert("Net work".into(), 4);
        let d = svg_to_dxf(SVG, &opts).unwrap();
        assert_eq!((d.polylines, d.circles, d.texts), (1, 1, 1));
        assert_eq!(d.layers, ["NET_WORK"]);
        assert!(d.dxf.contains("\r\n8\r\nNET_WORK\r\n62\r\n7\r\n66\r\n1\r\n"));
        assert!(d.dxf.contains("\r\n1\r\nHi ?\r\n"), "non-ASCII text becomes ?");
        assert!(d.dxf.contains("\r\n70\r\n4\r\n62\r\n7\r\n6\r\nCONTINUOUS\r\n"), "the layer is locked");
        assert!(d.dxf.ends_with("0\r\nENDSEC\r\n0\r\nEOF\r\n"));
    }

    #[test]
    fn a_missing_viewbox_is_unsupported() {
        assert!(svg_to_dxf("<svg></svg>", &DxfOptions::default()).is_err());
    }

    #[test]
    fn path_data_is_flattened() {
        let subs = flatten_path("M0 0 L10 0 L10 10 Z M20 20 L30 30").unwrap();
        assert_eq!(subs.len(), 2);
        assert!(subs[0].closed && subs[0].pts.len() == 3);
        assert!(flatten_path("M0 0 X5").is_err());
    }
}
