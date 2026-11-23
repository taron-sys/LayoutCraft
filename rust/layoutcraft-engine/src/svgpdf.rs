//! File > Export > PDF, first half: a Rust port of the converter of `web/svgpdf.js` (docs/34, step N4.9a). The drawing's own SVG (what the engine renders) is turned into PDF
//! drawing operators (real paths, text and gradients, not a picture). Handled: svg, g, rect (also rounded), path, polygon, polyline, line, circle, ellipse, text / tspan (the 14
//! standard fonts), solid fills and strokes with opacity, linear gradients in user space, dashes, caps, joins, fill rules. Anything else is `Unsupported` and the caller keeps a raster
//! page. Embedded TrueType subsets (`ttf.js`, the web's `fontSource`) are not ported: the native app has no font files to embed, it uses the standard fonts.
use std::collections::HashMap;

use crate::svgutil::*;

fn num(n: f64) -> String {
    js_string(js_round(n * 10000.0) / 10000.0)
}

fn nums(v: &[f64]) -> String {
    v.iter().map(|x| num(*x)).collect::<Vec<_>>().join(" ")
}

const ELLIPSE_K: f64 = 0.5522847498;

fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> String {
    let k = ELLIPSE_K;
    format!(
        "{} {} m\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\nh\n",
        num(cx + rx), num(cy),
        num(cx + rx), num(cy + k * ry), num(cx + k * rx), num(cy + ry), num(cx), num(cy + ry),
        num(cx - k * rx), num(cy + ry), num(cx - rx), num(cy + k * ry), num(cx - rx), num(cy),
        num(cx - rx), num(cy - k * ry), num(cx - k * rx), num(cy - ry), num(cx), num(cy - ry),
        num(cx + k * rx), num(cy - ry), num(cx + rx), num(cy - k * ry), num(cx + rx), num(cy)
    )
}

fn rounded_rect(x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64) -> String {
    let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
    let k = ELLIPSE_K;
    format!(
        "{} {} m\n{} {} l\n{} {} {} {} {} {} c\n{} {} l\n{} {} {} {} {} {} c\n{} {} l\n{} {} {} {} {} {} c\n{} {} l\n{} {} {} {} {} {} c\nh\n",
        num(x + rx), num(y),
        num(x + w - rx), num(y),
        num(x + w - rx + k * rx), num(y), num(x + w), num(y + ry - k * ry), num(x + w), num(y + ry),
        num(x + w), num(y + h - ry),
        num(x + w), num(y + h - ry + k * ry), num(x + w - rx + k * rx), num(y + h), num(x + w - rx), num(y + h),
        num(x + rx), num(y + h),
        num(x + rx - k * rx), num(y + h), num(x), num(y + h - ry + k * ry), num(x), num(y + h - ry),
        num(x), num(y + ry),
        num(x), num(y + ry - k * ry), num(x + rx - k * rx), num(y), num(x + rx), num(y)
    )
}

fn path_arg_count(up: char) -> Option<usize> {
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

/// Path data -> PDF path operators (absolute coordinates).
fn path_ops(d: &str) -> Result<String, Unsupported> {
    let tokens = path_tokens(d);
    let (mut i, mut cmd) = (0usize, String::new());
    let (mut x, mut y, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut lc, mut lq): (Option<(f64, f64)>, Option<(f64, f64)>) = (None, None);
    let mut out = String::new();
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
        let single = if up_s.chars().count() == 1 { up_s.chars().next() } else { None };
        let Some(up) = single.filter(|u| path_arg_count(*u).is_some()) else { return unsupported(format!("path command {cmd}")) };
        let k = path_arg_count(up).unwrap();
        let a: Vec<f64> = tokens[i.min(tokens.len())..(i + k).min(tokens.len())].iter().map(|t| js_number(t)).collect();
        if a.len() < k || a.iter().any(|t| !t.is_finite()) {
            return unsupported("path data");
        }
        i += k;
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        let mut cubic = |x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64, out: &mut String, x: &mut f64, y: &mut f64, lc: &mut Option<(f64, f64)>, lq: &mut Option<(f64, f64)>| {
            out.push_str(&format!("{} {} {} {} {} {} c\n", num(x1), num(y1), num(x2), num(y2), num(x3), num(y3)));
            *lc = Some((x2, y2));
            *lq = None;
            *x = x3;
            *y = y3;
        };
        match up {
            'M' => {
                x = a[0] + ox;
                y = a[1] + oy;
                sx = x;
                sy = y;
                out.push_str(&format!("{} {} m\n", num(x), num(y)));
                lc = None;
                lq = None;
            }
            'L' => {
                x = a[0] + ox;
                y = a[1] + oy;
                out.push_str(&format!("{} {} l\n", num(x), num(y)));
                lc = None;
                lq = None;
            }
            'H' => {
                x = a[0] + ox;
                out.push_str(&format!("{} {} l\n", num(x), num(y)));
                lc = None;
                lq = None;
            }
            'V' => {
                y = a[0] + oy;
                out.push_str(&format!("{} {} l\n", num(x), num(y)));
                lc = None;
                lq = None;
            }
            'C' => cubic(a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy, a[4] + ox, a[5] + oy, &mut out, &mut x, &mut y, &mut lc, &mut lq),
            'S' => {
                let c1 = match lc {
                    Some(l) => (2.0 * x - l.0, 2.0 * y - l.1),
                    None => (x, y),
                };
                cubic(c1.0, c1.1, a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy, &mut out, &mut x, &mut y, &mut lc, &mut lq);
            }
            'Q' | 'T' => {
                let q = if up == 'Q' {
                    (a[0] + ox, a[1] + oy)
                } else {
                    match lq {
                        Some(l) => (2.0 * x - l.0, 2.0 * y - l.1),
                        None => (x, y),
                    }
                };
                let e = if up == 'Q' { (a[2] + ox, a[3] + oy) } else { (a[0] + ox, a[1] + oy) };
                let (px, py) = (x, y);
                cubic(px + (2.0 / 3.0) * (q.0 - px), py + (2.0 / 3.0) * (q.1 - py), e.0 + (2.0 / 3.0) * (q.0 - e.0), e.1 + (2.0 / 3.0) * (q.1 - e.1), e.0, e.1, &mut out, &mut x, &mut y, &mut lc, &mut lq);
                lq = Some(q);
            }
            'A' => {
                let (ex, ey) = (a[5] + ox, a[6] + oy);
                for seg in arc_to_beziers(x, y, a[0], a[1], a[2], a[3] != 0.0, a[4] != 0.0, ex, ey) {
                    match seg {
                        Seg::Line(sx_, sy_) => out.push_str(&format!("{} {} l\n", num(sx_), num(sy_))),
                        Seg::Cubic(c1, c2, c3, c4, c5, c6) => out.push_str(&format!("{} c\n", nums(&[c1, c2, c3, c4, c5, c6]))),
                    }
                }
                x = ex;
                y = ey;
                lc = None;
                lq = None;
            }
            'Z' => {
                out.push_str("h\n");
                x = sx;
                y = sy;
                lc = None;
                lq = None;
            }
            _ => {}
        }
    }
    Ok(out)
}

// ---- Helvetica / Times widths (1/1000 em) for the characters 32..126 ----
const HELV: [u32; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556,
    833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500,
    334, 260, 334, 584,
];
const TIMES: [u32; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444, 921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611,
    889, 722, 722, 556, 722, 667, 556, 611, 722, 722, 944, 722, 722, 611, 333, 278, 333, 469, 500, 333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500, 500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444,
    480, 200, 480, 541,
];

fn cp1252(c: u32) -> Option<u32> {
    Some(match c {
        0x20ac => 0x80,
        0x201a => 0x82,
        0x192 => 0x83,
        0x201e => 0x84,
        0x2026 => 0x85,
        0x2020 => 0x86,
        0x2021 => 0x87,
        0x2c6 => 0x88,
        0x2030 => 0x89,
        0x160 => 0x8a,
        0x2039 => 0x8b,
        0x152 => 0x8c,
        0x17d => 0x8e,
        0x2018 => 0x91,
        0x2019 => 0x92,
        0x201c => 0x93,
        0x201d => 0x94,
        0x2022 => 0x95,
        0x2013 => 0x96,
        0x2014 => 0x97,
        0x2dc => 0x98,
        0x2122 => 0x99,
        0x161 => 0x9a,
        0x203a => 0x9b,
        0x153 => 0x9c,
        0x17e => 0x9e,
        0x178 => 0x9f,
        _ => return None,
    })
}

fn win_ansi(text: &str) -> Result<Vec<u32>, Unsupported> {
    let mut codes = Vec::new();
    for ch in text.replace('\u{a0}', " ").chars() {
        let c = ch as u32;
        if (32..127).contains(&c) || (160..256).contains(&c) {
            codes.push(c);
        } else if let Some(m) = cp1252(c) {
            codes.push(m);
        } else if matches!(c, 9 | 10 | 13) {
            codes.push(32);
        } else {
            return unsupported(format!("character U+{c:x}"));
        }
    }
    Ok(codes)
}

/// Width of the text in the standard font `base` (its regular metrics; bold faces are about 6 % wider).
fn text_width(codes: &[u32], size: f64, base: &str) -> f64 {
    let times = base.starts_with("Times");
    let courier = base.starts_with("Courier");
    let wide = if base.contains("Bold") { 1.06 } else { 1.0 };
    let total: u32 = codes
        .iter()
        .map(|c| {
            if courier {
                600
            } else if (32..127).contains(c) {
                if times { TIMES[(*c - 32) as usize] } else { HELV[(*c - 32) as usize] }
            } else if times {
                500
            } else {
                556
            }
        })
        .sum();
    (total as f64 * size * wide) / 1000.0
}

fn hex(codes: &[u32]) -> String {
    format!("<{}>", codes.iter().map(|c| format!("{c:02x}")).collect::<String>())
}

fn standard_font(family: &str, weight: &str, style: &str) -> String {
    let f = family.to_lowercase();
    let bold = ["600", "700", "800", "900"].iter().any(|w| weight.contains(w)) || weight.contains("bold");
    let italic = style.contains("italic") || style.contains("oblique");
    if f.contains("courier") || f.contains("consolas") || f.contains("mono") {
        return format!("Courier{}{}{}", if bold || italic { "-" } else { "" }, if bold { "Bold" } else { "" }, if italic { "Oblique" } else { "" });
    }
    let serif = ["times", "georgia", "cambria", "garamond", "serif", "palatino", "book"].iter().any(|k| f.contains(k)) && !f.contains("sans");
    if serif {
        return if bold && italic { "Times-BoldItalic" } else if bold { "Times-Bold" } else if italic { "Times-Italic" } else { "Times-Roman" }.to_string();
    }
    format!("Helvetica{}{}{}", if bold || italic { "-" } else { "" }, if bold { "Bold" } else { "" }, if italic { "Oblique" } else { "" })
}

const INHERITED: [&str; 15] = [
    "fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "stroke-dasharray", "stroke-miterlimit", "fill-opacity", "stroke-opacity", "fill-rule", "font-family", "font-size", "font-weight", "font-style",
    "text-anchor",
];
const UNSUPPORTED_TAGS: [&str; 12] = ["pattern", "filter", "mask", "clipPath", "image", "use", "symbol", "radialGradient", "foreignObject", "marker", "style", "script"];

type Style = HashMap<String, String>;

/// One converted page.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfPage {
    pub content: String,
    /// standard font name -> resource name (F1, F2 ...), in order of first use
    pub fonts: Vec<(String, String)>,
    /// (key, resource name, dictionary), in order of creation
    pub gstates: Vec<(String, String, String)>,
    /// (resource name, dictionary)
    pub patterns: Vec<(String, String)>,
    pub width_pt: f64,
    pub height_pt: f64,
}

struct Conv<'a> {
    out: String,
    fonts: Vec<(String, String)>,
    gstates: Vec<(String, String, String)>,
    patterns: Vec<(String, String)>,
    gradients: HashMap<String, &'a Node>,
}

fn attr_num(node: &Node, k: &str, d: f64) -> Result<f64, Unsupported> {
    let v = node.attr(k).map_or(d, js_number);
    if v.is_finite() { Ok(v) } else { unsupported(format!("attribute {k}")) }
}

fn style_num(st: &Style, k: &str, d: f64) -> f64 {
    st.get(k).map_or(d, |v| js_number(v))
}

fn url_ref(v: &str) -> Option<&str> {
    // /^url\(#([^)]+)\)$/
    let inner = v.strip_prefix("url(#")?.strip_suffix(')')?;
    (!inner.is_empty() && !inner.contains(')')).then_some(inner)
}

impl<'a> Conv<'a> {
    fn gstate(&mut self, fo: f64, so: f64) -> Option<String> {
        if fo >= 0.9999 && so >= 0.9999 {
            return None;
        }
        let key = format!("{}/{}", num(fo), num(so));
        if let Some((_, name, _)) = self.gstates.iter().find(|(k, _, _)| *k == key) {
            return Some(name.clone());
        }
        let name = format!("GS{}", self.gstates.len() + 1);
        self.gstates.push((key, name.clone(), format!("<< /Type /ExtGState /ca {} /CA {} >>", num(fo), num(so))));
        Some(name)
    }

    fn shading(&mut self, id: &str, ctm: &Matrix) -> Result<String, Unsupported> {
        let g = *self.gradients.get(id).ok_or_else(|| Unsupported(format!("gradient {id}")))?;
        if g.attr("href").is_some_and(|h| !h.is_empty()) || g.attr("xlink:href").is_some_and(|h| !h.is_empty()) {
            return unsupported("gradient href");
        }
        if g.attr("gradientUnits") != Some("userSpaceOnUse") {
            return unsupported("gradient units");
        }
        let c: Vec<f64> = ["x1", "y1", "x2", "y2"].iter().map(|k| g.attr(k).map_or(0.0, js_number)).collect();
        if c.iter().any(|v| !v.is_finite()) {
            return unsupported("gradient coordinates");
        }
        let mut stops: Vec<(f64, [f64; 3])> = Vec::new();
        for s in g.elements().filter(|s| s.tag == "stop") {
            if s.attr("stop-opacity").map_or(1.0, js_number) < 0.9999 {
                return unsupported("transparent gradient stop");
            }
            let offset = s.attr("offset");
            let raw = offset.map_or(0.0, js_parse_float);
            let at = 1.0f64.min(0.0f64.max(raw / if offset.is_some_and(|o| o.ends_with('%')) { 100.0 } else { 1.0 }));
            stops.push((at, parse_colour(s.attr("stop-color").unwrap_or("#000000"))?));
        }
        if stops.len() < 2 {
            return unsupported("gradient stops");
        }
        for i in 1..stops.len() {
            stops[i].0 = stops[i].0.max(stops[i - 1].0);
        }
        let func_of = |a: &[f64; 3], b: &[f64; 3]| format!("<< /FunctionType 2 /Domain [0 1] /C0 [{}] /C1 [{}] /N 1 >>", nums(a), nums(b));
        let func = if stops.len() == 2 && stops[0].0 == 0.0 && stops[1].0 == 1.0 {
            func_of(&stops[0].1, &stops[1].1)
        } else {
            let mut all = vec![(0.0, stops[0].1)];
            all.extend(stops.iter().cloned());
            all.push((1.0, stops[stops.len() - 1].1));
            let (mut segs, mut bounds) = (Vec::new(), Vec::new());
            for i in 0..all.len() - 1 {
                segs.push(func_of(&all[i].1, &all[i + 1].1));
                if i + 2 < all.len() {
                    bounds.push(all[i + 1].0);
                }
            }
            format!("<< /FunctionType 3 /Domain [0 1] /Functions [{}] /Bounds [{}] /Encode [{}] >>", segs.join(" "), nums(&bounds), segs.iter().map(|_| "0 1").collect::<Vec<_>>().join(" "))
        };
        let m = mul(&parse_transform(g.attr("gradientTransform"))?, ctm);
        let name = format!("P{}", self.patterns.len() + 1);
        self.patterns.push((name.clone(), format!("<< /Type /Pattern /PatternType 2 /Matrix [{}] /Shading << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [{}] /Function {func} /Extend [true true] >> >>", nums(&m), nums(&c))));
        Ok(name)
    }

    fn font_of(&mut self, st: &Style) -> String {
        let name = standard_font(st.get("font-family").map_or("", String::as_str), st.get("font-weight").map_or("", String::as_str), st.get("font-style").map_or("", String::as_str));
        if let Some((_, res)) = self.fonts.iter().find(|(n, _)| *n == name) {
            return res.clone();
        }
        let res = format!("F{}", self.fonts.len() + 1);
        self.fonts.push((name, res.clone()));
        res
    }

    fn fill_ops(&mut self, fill: &str, ctm: &Matrix) -> Result<(), Unsupported> {
        if let Some(id) = url_ref(fill) {
            let name = self.shading(id, ctm)?;
            self.out.push_str(&format!("/Pattern cs /{name} scn\n"));
        } else {
            self.out.push_str(&format!("{} rg\n", nums(&parse_colour(fill)?)));
        }
        Ok(())
    }

    fn paint(&mut self, path: &str, st: &Style, ctm: &Matrix, can_fill: bool) -> Result<(), Unsupported> {
        let fill_v = if can_fill { st.get("fill").map_or("#000000", String::as_str) } else { "none" };
        let stroke_v = st.get("stroke").map_or("none", String::as_str);
        let do_fill = fill_v != "none" && fill_v != "transparent";
        let do_stroke = stroke_v != "none" && stroke_v != "transparent";
        if !do_fill && !do_stroke {
            return Ok(());
        }
        self.out.push_str("q\n");
        let gs = self.gstate(if do_fill { style_num(st, "fill-opacity", 1.0) } else { 1.0 }, if do_stroke { style_num(st, "stroke-opacity", 1.0) } else { 1.0 });
        if let Some(gs) = gs {
            self.out.push_str(&format!("/{gs} gs\n"));
        }
        if do_fill {
            self.fill_ops(fill_v, ctm)?;
        }
        if do_stroke {
            if stroke_v.starts_with("url(") {
                return unsupported("gradient stroke");
            }
            self.out.push_str(&format!("{} RG\n{} w\n", nums(&parse_colour(stroke_v)?), num(style_num(st, "stroke-width", 1.0))));
            let cap = match st.get("stroke-linecap").map_or("butt", String::as_str) {
                "round" => 1,
                "square" => 2,
                _ => 0,
            };
            let join = match st.get("stroke-linejoin").map_or("miter", String::as_str) {
                "round" => 1,
                "bevel" => 2,
                _ => 0,
            };
            self.out.push_str(&format!("{cap} J {join} j\n"));
            if st.get("stroke-miterlimit").is_some_and(|v| !v.is_empty()) {
                self.out.push_str(&format!("{} M\n", num(style_num(st, "stroke-miterlimit", 1.0))));
            }
            let dash = st.get("stroke-dasharray").map_or("none", String::as_str);
            if dash != "none" {
                let v: Vec<f64> = split_space_comma(dash).iter().filter(|s| !s.is_empty()).map(|s| js_number(s)).collect();
                if !v.is_empty() && v.iter().all(|x| x.is_finite() && *x >= 0.0) && v.iter().any(|x| *x > 0.0) {
                    let list: Vec<f64> = if v.len() % 2 == 1 { v.iter().chain(v.iter()).cloned().collect() } else { v };
                    self.out.push_str(&format!("[{}] 0 d\n", nums(&list)));
                }
            }
        }
        let evenodd = st.get("fill-rule").map(String::as_str) == Some("evenodd");
        let op = if do_fill && do_stroke {
            if evenodd { "B*" } else { "B" }
        } else if do_fill {
            if evenodd { "f*" } else { "f" }
        } else {
            "S"
        };
        self.out.push_str(&format!("{path}{op}\nQ\n"));
        Ok(())
    }

    fn text_run(&mut self, s: &str, x: f64, y: f64, st: &Style, node: &Node, ctm: &Matrix) -> Result<f64, Unsupported> {
        let codes = win_ansi(s)?;
        if codes.is_empty() {
            return Ok(0.0);
        }
        let size = style_num(st, "font-size", 16.0);
        let fill_v = st.get("fill").map_or("#000000", String::as_str);
        if fill_v == "none" {
            return Ok(0.0);
        }
        let base = standard_font(st.get("font-family").map_or("", String::as_str), st.get("font-weight").map_or("", String::as_str), st.get("font-style").map_or("", String::as_str));
        let natural = text_width(&codes, size, &base);
        let want = node.attr("textLength").map_or(natural, js_number);
        let width = if want.is_finite() && want > 0.0 { want } else { natural };
        let anchor = st.get("text-anchor").map_or("start", String::as_str);
        let x0 = if anchor == "middle" { x - width / 2.0 } else if anchor == "end" { x - width } else { x };
        let tz = if natural > 0.0 { js_round((width / natural) * 10000.0) / 100.0 } else { 100.0 };
        self.out.push_str("q\n");
        if let Some(gs) = self.gstate(style_num(st, "fill-opacity", 1.0), 1.0) {
            self.out.push_str(&format!("/{gs} gs\n"));
        }
        self.fill_ops(fill_v, ctm)?;
        let font = self.font_of(st);
        self.out.push_str(&format!("BT /{font} {} Tf {} Tz 1 0 0 -1 {} {} Tm {} Tj ET\nQ\n", num(size), num(tz), num(x0), num(y), hex(&codes)));
        Ok(width)
    }

    fn draw(&mut self, node: &Node, parent: &Style, ctm: &Matrix) -> Result<(), Unsupported> {
        if ["title", "desc", "defs", "linearGradient", "stop"].contains(&node.tag.as_str()) {
            return Ok(());
        }
        if UNSUPPORTED_TAGS.contains(&node.tag.as_str()) {
            return unsupported(format!("<{}>", node.tag));
        }
        if node.attr("display") == Some("none") || node.attr("visibility") == Some("hidden") {
            return Ok(());
        }
        if node.attr("opacity").is_some_and(|o| js_number(o) < 0.9999) {
            return unsupported("group opacity");
        }
        if ["filter", "mask", "clip-path"].iter().any(|k| node.attr(k).is_some_and(|v| !v.is_empty())) {
            return unsupported("filter / mask / clip");
        }
        let mut st = parent.clone();
        for k in INHERITED {
            if let Some(v) = node.attr(k) {
                st.insert(k.to_string(), v.to_string());
            }
        }
        let local = parse_transform(node.attr("transform"))?;
        let here = mul(&local, ctm);
        let has_transform = node.attr("transform").is_some();
        if has_transform {
            self.out.push_str(&format!("q {} cm\n", nums(&local)));
        }
        match node.tag.as_str() {
            "svg" | "g" => {
                for c in node.elements() {
                    self.draw(c, &st, &here)?;
                }
            }
            "rect" => {
                let (w, h) = (attr_num(node, "width", 0.0)?, attr_num(node, "height", 0.0)?);
                if w > 0.0 && h > 0.0 {
                    let rx = if node.attr("rx").is_some() { attr_num(node, "rx", 0.0)? } else { attr_num(node, "ry", 0.0)? };
                    let ry = if node.attr("ry").is_some() { attr_num(node, "ry", 0.0)? } else { rx };
                    let (x, y) = (attr_num(node, "x", 0.0)?, attr_num(node, "y", 0.0)?);
                    let path = if rx > 0.0 || ry > 0.0 { rounded_rect(x, y, w, h, rx, ry) } else { format!("{} {} {} {} re\n", num(x), num(y), num(w), num(h)) };
                    self.paint(&path, &st, &here, true)?;
                }
            }
            "path" => {
                let path = path_ops(node.attr("d").unwrap_or(""))?;
                self.paint(&path, &st, &here, true)?;
            }
            "polygon" | "polyline" => {
                let v: Vec<f64> = split_space_comma(node.attr("points").unwrap_or("")).iter().filter(|s| !s.is_empty()).map(|s| js_number(s)).collect();
                if !(v.len() < 4 || v.iter().any(|q| !q.is_finite())) {
                    let mut p = format!("{} {} m\n", num(v[0]), num(v[1]));
                    let mut i = 2;
                    while i + 1 < v.len() {
                        p.push_str(&format!("{} {} l\n", num(v[i]), num(v[i + 1])));
                        i += 2;
                    }
                    let polygon = node.tag == "polygon";
                    self.paint(&if polygon { format!("{p}h\n") } else { p }, &st, &here, polygon)?;
                }
            }
            "line" => {
                let path = format!("{} {} m\n{} {} l\n", num(attr_num(node, "x1", 0.0)?), num(attr_num(node, "y1", 0.0)?), num(attr_num(node, "x2", 0.0)?), num(attr_num(node, "y2", 0.0)?));
                self.paint(&path, &st, &here, false)?;
            }
            "circle" => {
                let (cx, cy, r) = (attr_num(node, "cx", 0.0)?, attr_num(node, "cy", 0.0)?, attr_num(node, "r", 0.0)?);
                self.paint(&ellipse_path(cx, cy, r, r), &st, &here, true)?;
            }
            "ellipse" => {
                let (cx, cy, rx, ry) = (attr_num(node, "cx", 0.0)?, attr_num(node, "cy", 0.0)?, attr_num(node, "rx", 0.0)?, attr_num(node, "ry", 0.0)?);
                self.paint(&ellipse_path(cx, cy, rx, ry), &st, &here, true)?;
            }
            "text" => self.draw_text(node, &st, &here)?,
            other => return unsupported(format!("<{other}>")),
        }
        if has_transform {
            self.out.push_str("Q\n");
        }
        Ok(())
    }

    fn draw_text(&mut self, node: &Node, st: &Style, here: &Matrix) -> Result<(), Unsupported> {
        let (x, y) = (attr_num(node, "x", 0.0)?, attr_num(node, "y", 0.0)?);
        let own = node.own_text();
        let spans: Vec<&Node> = node.elements().collect();
        for c in &spans {
            if c.tag != "tspan" {
                return unsupported(format!("<{}> in text", c.tag));
            }
            if c.attr("dx").is_some_and(|v| !v.is_empty()) || c.attr("dy").is_some_and(|v| !v.is_empty()) {
                return unsupported("tspan dx / dy");
            }
            if c.elements().next().is_some() {
                return unsupported("nested tspan");
            }
        }
        let placed = spans.iter().any(|c| c.attr("x").is_some() || c.attr("y").is_some());
        if spans.is_empty() {
            if !own.trim_matches(is_js_space).is_empty() {
                self.text_run(&own, x, y, st, node, here)?;
            }
            return Ok(());
        }
        let styled = |c: &Node, st: &Style| -> Style {
            let mut cs = st.clone();
            for k in INHERITED {
                if let Some(v) = c.attr(k) {
                    cs.insert(k.to_string(), v.to_string());
                }
            }
            cs
        };
        if !placed {
            if spans.iter().all(|c| INHERITED.iter().all(|k| c.attr(k).is_none())) {
                let whole: String = format!("{own}{}", spans.iter().map(|c| c.own_text()).collect::<String>());
                self.text_run(&whole, x, y, st, node, here)?;
                return Ok(());
            }
            let mut cursor = x;
            for c in &spans {
                let mut cs = styled(c, st);
                cs.insert("text-anchor".into(), "start".into());
                cursor += self.text_run(&c.own_text(), cursor, y, &cs, c, here)?;
            }
            return Ok(());
        }
        let (mut cx, mut cy) = (x, y);
        for c in &spans {
            let cs = styled(c, st);
            if let Some(v) = c.attr("x") {
                cx = js_number(v);
            }
            if let Some(v) = c.attr("y") {
                cy = js_number(v);
            }
            if !cx.is_finite() || !cy.is_finite() {
                return unsupported("tspan position");
            }
            self.text_run(&c.own_text(), cx, cy, &cs, c, here)?;
        }
        Ok(())
    }
}

fn collect_gradients<'a>(node: &'a Node, gradients: &mut HashMap<String, &'a Node>) -> Result<(), Unsupported> {
    if UNSUPPORTED_TAGS.contains(&node.tag.as_str()) && node.tag != "radialGradient" && node.tag != "image" {
        return unsupported(format!("<{}>", node.tag));
    }
    if node.tag == "linearGradient" {
        if let Some(id) = node.attr("id").filter(|i| !i.is_empty()) {
            gradients.insert(id.to_string(), node);
        }
    }
    for c in node.elements() {
        collect_gradients(c, gradients)?;
    }
    Ok(())
}

/// `svgToPdfPage(svgText, {paper})` without embedded fonts: the PDF operators of one page.
pub fn svg_to_pdf_page(svg_text: &str, paper: Option<&str>) -> Result<PdfPage, Unsupported> {
    let root = parse_xml(svg_text)?;
    let vb: Vec<f64> = split_space_comma(root.attr("viewBox").unwrap_or("")).iter().map(|s| js_number(s)).collect();
    let (w_px, h_px) = (root.attr("width").map_or(f64::NAN, js_parse_float), root.attr("height").map_or(f64::NAN, js_parse_float));
    if vb.len() != 4 || vb.iter().any(|v| !v.is_finite()) || !(w_px > 0.0) || !(h_px > 0.0) {
        return unsupported("viewBox");
    }
    let (width_pt, height_pt) = (w_px * 0.75, h_px * 0.75);
    let (sx, sy) = (width_pt / vb[2], height_pt / vb[3]);
    let base: Matrix = [sx, 0.0, 0.0, -sy, -vb[0] * sx, (vb[1] + vb[3]) * sy];
    let mut gradients: HashMap<String, &Node> = HashMap::new();
    collect_gradients(&root, &mut gradients)?;
    let mut conv = Conv { out: String::new(), fonts: Vec::new(), gstates: Vec::new(), patterns: Vec::new(), gradients };
    if let Some(p) = paper {
        conv.out.push_str(&format!("q {} rg 0 0 {} {} re f Q\n", nums(&parse_colour(p)?), num(width_pt), num(height_pt)));
    }
    conv.out.push_str(&format!("q {} cm\n", nums(&base)));
    conv.draw(&root, &Style::new(), &base)?;
    conv.out.push_str("Q\n");
    Ok(PdfPage { content: conv.out, fonts: conv.fonts, gstates: conv.gstates, patterns: conv.patterns, width_pt, height_pt })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_a_line_and_a_text_become_operators() {
        let svg = "<svg width=\"192\" height=\"96\" viewBox=\"0 0 192 96\"><g fill=\"#ff0000\" stroke=\"#000000\" stroke-width=\"2\"><rect x=\"10\" y=\"10\" width=\"50\" height=\"20\"/></g><line x1=\"0\" y1=\"0\" x2=\"5\" y2=\"5\" stroke=\"blue\"/><text x=\"20\" y=\"60\" font-size=\"12\" font-family=\"Arial\" fill=\"#000\" text-anchor=\"middle\">Hi</text></svg>";
        let page = svg_to_pdf_page(svg, Some("#ffffff")).unwrap();
        assert_eq!((page.width_pt, page.height_pt), (144.0, 72.0));
        assert!(page.content.starts_with("q 1 1 1 rg 0 0 144 72 re f Q\nq 0.75 0 0 -0.75 0 72 cm\n"));
        assert!(page.content.contains("1 0 0 rg\n0 0 0 RG\n2 w\n0 J 0 j\n10 10 50 20 re\nB\n"));
        assert!(page.content.contains("BT /F1 12 Tf 100 Tz 1 0 0 -1 14.336 60 Tm <4869> Tj ET"));
        assert_eq!(page.fonts, vec![("Helvetica".to_string(), "F1".to_string())]);
    }

    #[test]
    fn what_cannot_be_drawn_is_unsupported() {
        let wrap = |inner: &str| format!("<svg width=\"96\" height=\"96\" viewBox=\"0 0 96 96\">{inner}</svg>");
        assert_eq!(svg_to_pdf_page(&wrap("<use/>"), None).unwrap_err().0, "<use>");
        assert_eq!(svg_to_pdf_page(&wrap("<g opacity=\"0.5\"/>"), None).unwrap_err().0, "group opacity");
        assert_eq!(svg_to_pdf_page(&wrap("<text x=\"1\" y=\"1\">\u{4e2d}</text>"), None).unwrap_err().0, "character U+4e2d");
        assert_eq!(svg_to_pdf_page(&wrap("<rect width=\"5\" height=\"5\" fill=\"url(#nope)\"/>"), None).unwrap_err().0, "gradient nope");
        assert_eq!(svg_to_pdf_page("<svg viewBox=\"0 0 1 1\"/>", None).unwrap_err().0, "viewBox");
    }

    #[test]
    fn gradients_dashes_and_fonts() {
        let svg = "<svg width=\"96\" height=\"96\" viewBox=\"0 0 96 96\"><defs><linearGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"96\" y2=\"0\"><stop offset=\"0\" stop-color=\"#ff0000\"/><stop offset=\"100%\" stop-color=\"#0000ff\"/></linearGradient></defs><path d=\"M0 0 L10 0 L10 10 Z\" fill=\"url(#g)\" stroke=\"#000\" stroke-dasharray=\"4 2 1\" fill-opacity=\"0.5\"/><text x=\"1\" y=\"2\" font-family=\"Courier New\" font-weight=\"bold\">A</text></svg>";
        let page = svg_to_pdf_page(svg, None).unwrap();
        assert_eq!(page.patterns.len(), 1);
        assert!(page.patterns[0].1.contains("/FunctionType 2"));
        assert_eq!(page.gstates[0].1, "GS1");
        assert!(page.content.contains("[4 2 1 4 2 1] 0 d"));
        assert_eq!(page.fonts[0].0, "Courier-Bold");
        assert_eq!(standard_font("Times New Roman", "", "italic"), "Times-Italic");
        assert_eq!(text_width(&[72, 105], 10.0, "Helvetica"), 10.0 * (722.0 + 222.0) / 1000.0);
    }
}

impl crate::Engine {
    /// One page of the drawing as PDF operators (the plain-view SVG of the page through `svg_to_pdf_page`, on white paper).
    pub fn pdf_page(&self, page: usize) -> Result<Result<PdfPage, Unsupported>, String> {
        let svg = crate::webpage::page_svg(self, page)?;
        Ok(svg_to_pdf_page(&svg, Some("#ffffff")))
    }
}
