//! The small SVG reading helpers the exporters share: a Rust port of the first part of `web/svgpdf.js` (a tree of the engine's SVG, transforms, colours, arcs)
//! with the same results, quirks included (docs/34, step N4.5; the PDF export will use them too).
use std::collections::HashMap;

/// `Unsupported` of the web code: markup the converters do not handle.
#[derive(Debug, Clone, PartialEq)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn unsupported<T>(what: impl Into<String>) -> Result<T, Unsupported> {
    Err(Unsupported(what.into()))
}

/// An element of the tree: attributes by name (a repeated name keeps the last value) and children in order.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub tag: String,
    pub attrs: HashMap<String, String>,
    pub children: Vec<Child>,
}

#[derive(Debug, Clone)]
pub enum Child {
    Text(String),
    Element(Node),
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
    pub fn elements(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(|c| if let Child::Element(n) = c { Some(n) } else { None })
    }
    /// The text children joined.
    pub fn own_text(&self) -> String {
        self.children.iter().filter_map(|c| if let Child::Text(t) = c { Some(t.as_str()) } else { None }).collect()
    }
}

fn build(node: roxmltree::Node) -> Node {
    let mut out = Node { tag: node.tag_name().name().to_string(), ..Default::default() };
    for a in node.attributes() {
        out.attrs.insert(a.name().to_string(), a.value().to_string());
    }
    for child in node.children() {
        if child.is_element() {
            out.children.push(Child::Element(build(child)));
        } else if child.is_text() {
            out.children.push(Child::Text(child.text().unwrap_or("").to_string()));
        }
    }
    out
}

/// The `svg` element of `source` (`parseXml`).
pub fn parse_xml(source: &str) -> Result<Node, Unsupported> {
    let doc = roxmltree::Document::parse(source).map_err(|e| Unsupported(format!("xml: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return unsupported("no svg element");
    }
    Ok(build(root))
}

/// JavaScript `Math.round`.
pub fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// JavaScript `String(number)` for the finite numbers the exporters write.
pub fn js_string(r: f64) -> String {
    if r.is_nan() {
        "NaN".to_string()
    } else if r.is_infinite() {
        if r > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if r == 0.0 {
        "0".to_string()
    } else {
        js_number_string(r)
    }
}

/// `Number.prototype.toString` for a finite non-zero number: shortest round-trip digits, decimal for exponents -7 < e < 21, else exponential.
fn js_number_string(r: f64) -> String {
    let sci = format!("{:e}", r.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let k = digits.len() as i32;
    let n = exp + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e1 = n - 1;
        let sign = if e1 < 0 { '-' } else { '+' };
        if k == 1 { format!("{digits}e{sign}{}", e1.abs()) } else { format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e1.abs()) }
    };
    if r < 0.0 { format!("-{body}") } else { body }
}

/// JavaScript white space (`\s`).
pub fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// JavaScript `Number(text)` (NaN when it is not a number); the common forms: decimal, exponent, empty = 0.
pub fn js_number(text: &str) -> f64 {
    let t = text.trim_matches(is_js_space);
    if t.is_empty() {
        return 0.0;
    }
    if t.chars().any(|c| !(c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))) {
        return match t {
            "Infinity" | "+Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => f64::NAN,
        };
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// JavaScript `parseFloat`: the longest decimal prefix.
pub fn js_parse_float(text: &str) -> f64 {
    let t = text.trim_start_matches(is_js_space);
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut seen = i > digits_start;
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > i + 1 || seen {
            seen = true;
            i = j;
        }
    }
    if !seen {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let ds = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > ds {
            i = j;
        }
    }
    t[..i].parse::<f64>().unwrap_or(f64::NAN)
}

/// `text.split(/[\s,]+/)` with JavaScript's empty first / last pieces.
pub fn split_space_comma(text: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut in_sep = false;
    for c in text.chars() {
        if is_js_space(c) || c == ',' {
            if !in_sep {
                parts.push(String::new());
            }
            in_sep = true;
        } else {
            in_sep = false;
            parts.last_mut().unwrap().push(c);
        }
    }
    parts
}

pub type Matrix = [f64; 6];
pub const IDENT: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `mul(A, B)`: A applied first, then B.
pub fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// `parseTransform`: the list of `name(args)` transforms.
pub fn parse_transform(text: Option<&str>) -> Result<Matrix, Unsupported> {
    let mut m = IDENT;
    let Some(text) = text else { return Ok(m) };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // (\w+)\s*\(([^)]*)\)
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        if i == start {
            i += 1;
            continue;
        }
        let name: String = chars[start..i].iter().collect();
        let mut j = i;
        while j < chars.len() && is_js_space(chars[j]) {
            j += 1;
        }
        if j >= chars.len() || chars[j] != '(' {
            continue;
        }
        let Some(close) = chars[j + 1..].iter().position(|c| *c == ')') else {
            break;
        };
        let args: String = chars[j + 1..j + 1 + close].iter().collect();
        let whole: String = chars[start..j + 1 + close + 1].iter().collect();
        i = j + 1 + close + 1;
        let v: Vec<f64> = split_space_comma(&args).iter().filter(|s| !s.is_empty()).map(|s| js_number(s)).collect();
        if v.iter().any(|x| !x.is_finite()) {
            return unsupported(format!("transform {whole}"));
        }
        let at = |k: usize| v.get(k).copied().unwrap_or(f64::NAN);
        let n: Matrix = match name.as_str() {
            "translate" => [1.0, 0.0, 0.0, 1.0, at(0), v.get(1).copied().unwrap_or(0.0)],
            "scale" => [at(0), 0.0, 0.0, v.get(1).copied().unwrap_or(at(0)), 0.0, 0.0],
            "rotate" => {
                let a = (at(0) * std::f64::consts::PI) / 180.0;
                let (c, s) = (a.cos(), a.sin());
                let mut n = [c, s, -s, c, 0.0, 0.0];
                if v.len() >= 3 {
                    n = mul(&mul(&[1.0, 0.0, 0.0, 1.0, -v[1], -v[2]], &n), &[1.0, 0.0, 0.0, 1.0, v[1], v[2]]);
                }
                n
            }
            "matrix" => {
                if v.len() < 6 {
                    return unsupported(format!("transform {whole}"));
                }
                [v[0], v[1], v[2], v[3], v[4], v[5]]
            }
            "skewX" => [1.0, 0.0, ((at(0) * std::f64::consts::PI) / 180.0).tan(), 1.0, 0.0, 0.0],
            "skewY" => [1.0, ((at(0) * std::f64::consts::PI) / 180.0).tan(), 0.0, 1.0, 0.0, 0.0],
            other => return unsupported(format!("transform {other}")),
        };
        m = mul(&n, &m);
    }
    Ok(m)
}

/// `parseColour`: [r, g, b] in 0..1.
pub fn parse_colour(value: &str) -> Result<[f64; 3], Unsupported> {
    let v = value.trim_matches(is_js_space).to_lowercase();
    let hex = |s: &str| u8::from_str_radix(s, 16).ok().map(|n| n as f64);
    if let Some(h) = v.strip_prefix('#') {
        if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok([hex(&h[0..2]).unwrap() / 255.0, hex(&h[2..4]).unwrap() / 255.0, hex(&h[4..6]).unwrap() / 255.0]);
        }
        if h.len() == 3 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            let d = |i: usize| hex(&format!("{0}{0}", &h[i..i + 1])).unwrap() / 255.0;
            return Ok([d(0), d(1), d(2)]);
        }
    }
    if let Some(body) = v.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        // \s*([\d.]+%?)[ ,]+([\d.]+%?)[ ,]+([\d.]+%?)\s*
        let body = body.trim_matches(is_js_space);
        let parts: Vec<&str> = body.split([' ', ',']).filter(|s| !s.is_empty()).collect();
        let ok = |s: &str| {
            let core = s.strip_suffix('%').unwrap_or(s);
            !core.is_empty() && core.chars().all(|c| c.is_ascii_digit() || c == '.')
        };
        if parts.len() == 3 && parts.iter().all(|p| ok(p)) {
            let c = |s: &str| if s.ends_with('%') { js_parse_float(s) / 100.0 } else { js_parse_float(s) / 255.0 };
            return Ok([c(parts[0]), c(parts[1]), c(parts[2])]);
        }
    }
    let named: Option<[f64; 3]> = match v.as_str() {
        "black" => Some([0.0, 0.0, 0.0]),
        "white" => Some([1.0, 1.0, 1.0]),
        "red" => Some([1.0, 0.0, 0.0]),
        "green" => Some([0.0, 0.5, 0.0]),
        "blue" => Some([0.0, 0.0, 1.0]),
        "yellow" => Some([1.0, 1.0, 0.0]),
        "gray" | "grey" => Some([0.5, 0.5, 0.5]),
        "silver" => Some([0.75, 0.75, 0.75]),
        "orange" => Some([1.0, 0.647, 0.0]),
        "purple" => Some([0.5, 0.0, 0.5]),
        _ => None,
    };
    named.ok_or_else(|| Unsupported(format!("colour {value}")))
}

/// One piece of an arc: a line to a point, or a cubic Bezier (c1, c2, end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line(f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
}

/// `arcToBeziers`.
#[allow(clippy::too_many_arguments)]
pub fn arc_to_beziers(x1: f64, y1: f64, rx: f64, ry: f64, phi_deg: f64, large: bool, sweep: bool, x2: f64, y2: f64) -> Vec<Seg> {
    if rx == 0.0 || ry == 0.0 {
        return vec![Seg::Line(x2, y2)];
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    let phi = (phi_deg * std::f64::consts::PI) / 180.0;
    let (cos, sin) = (phi.cos(), phi.sin());
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let (x1p, y1p) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let sign = if large == sweep { -1.0 } else { 1.0 };
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let co = sign * (num / if den == 0.0 { 1.0 } else { den }).max(0.0).sqrt();
    let (cxp, cyp) = ((co * rx * y1p) / ry, (-co * ry * x1p) / rx);
    let (cx, cy) = (cos * cxp - sin * cyp + (x1 + x2) / 2.0, sin * cxp + cos * cyp + (y1 + y2) / 2.0);
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let th1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dth = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && dth > 0.0 {
        dth -= 2.0 * std::f64::consts::PI;
    }
    if sweep && dth < 0.0 {
        dth += 2.0 * std::f64::consts::PI;
    }
    let n = ((dth.abs() / (std::f64::consts::PI / 2.0) - 1e-9).ceil()).max(1.0) as usize;
    let step = dth / n as f64;
    let k = (4.0 / 3.0) * (step / 4.0).tan();
    let mut out = Vec::new();
    for i in 0..n {
        let a1 = th1 + i as f64 * step;
        let a2 = a1 + step;
        let p = |a: f64, d: f64| {
            [
                cos * rx * (a.cos() - d * a.sin()) - sin * ry * (a.sin() + d * a.cos()) + cx,
                sin * rx * (a.cos() - d * a.sin()) + cos * ry * (a.sin() + d * a.cos()) + cy,
            ]
        };
        let (c1, c2, e) = (p(a1, k), p(a2, -k), p(a2, 0.0));
        out.push(Seg::Cubic(c1[0], c1[1], c2[0], c2[1], if i == n - 1 { x2 } else { e[0] }, if i == n - 1 { y2 } else { e[1] }));
    }
    out
}

/// The tokens of SVG path data: `/[MmLlHhVvCcSsQqTtAaZz]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g`.
pub fn path_tokens(d: &str) -> Vec<String> {
    let c: Vec<char> = d.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if "MmLlHhVvCcSsQqTtAaZz".contains(c[i]) {
            out.push(c[i].to_string());
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i;
        if c[j] == '+' || c[j] == '-' {
            j += 1;
        }
        let mut ok = false;
        let int_start = j;
        while j < c.len() && c[j].is_ascii_digit() {
            j += 1;
        }
        if j > int_start {
            ok = true;
            if j < c.len() && c[j] == '.' {
                j += 1;
                while j < c.len() && c[j].is_ascii_digit() {
                    j += 1;
                }
            }
        } else if j < c.len() && c[j] == '.' && j + 1 < c.len() && c[j + 1].is_ascii_digit() {
            ok = true;
            j += 1;
            while j < c.len() && c[j].is_ascii_digit() {
                j += 1;
            }
        }
        if !ok {
            i += 1;
            continue;
        }
        if j < c.len() && (c[j] == 'e' || c[j] == 'E') {
            let mut k = j + 1;
            if k < c.len() && (c[k] == '+' || c[k] == '-') {
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
        out.push(c[start..j].iter().collect());
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_javascript() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
        assert_eq!(js_string(-0.0), "0");
        assert_eq!(js_string(0.000001), "0.000001");
        assert_eq!(js_string(1.2e-14), "1.2e-14");
        assert_eq!(js_string(1e-7), "1e-7");
        assert_eq!(js_string(-123456789012345680000.0), "-123456789012345680000");
        assert_eq!(js_string(1e21), "1e+21");
        assert_eq!(js_string(1234.5), "1234.5");
        assert_eq!(js_string(0.5), "0.5");
        assert_eq!(js_string(100.0), "100");
        assert!(js_number("10px").is_nan());
        assert_eq!(js_number(" 5 "), 5.0);
        assert_eq!(js_number(""), 0.0);
        assert_eq!(js_parse_float("12.5%"), 12.5);
    }

    #[test]
    fn tokens_transforms_and_colours() {
        assert_eq!(path_tokens("M10,20L-5.5.5z"), ["M", "10", "20", "L", "-5.5", ".5", "z"]);
        assert_eq!(parse_transform(Some("translate(10 20) scale(2)")).unwrap(), [2.0, 0.0, 0.0, 2.0, 10.0, 20.0]);
        assert!(parse_transform(Some("wobble(1)")).is_err());
        assert_eq!(parse_colour("#f00").unwrap(), [1.0, 0.0, 0.0]);
        assert_eq!(parse_colour("rgb(255, 0, 51)").unwrap(), [1.0, 0.0, 0.2]);
        assert!(parse_colour("papayawhip").is_err());
        assert_eq!(split_space_comma(" 0 0,10  10"), ["", "0", "0", "10", "10"]);
    }

    #[test]
    fn an_svg_becomes_a_tree() {
        let svg = parse_xml("<svg viewBox=\"0 0 1 1\"><g a=\"1\"><text>x &amp; y</text></g></svg>").unwrap();
        assert_eq!(svg.attr("viewBox"), Some("0 0 1 1"));
        let g = svg.elements().next().unwrap();
        assert_eq!(g.elements().next().unwrap().own_text(), "x & y");
    }
}
