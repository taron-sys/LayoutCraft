//! Enhanced Metafile (EMF) to SVG conversion.
//!
//! Format: the record layouts of the public Open Specifications [MS-EMF] (Enhanced Metafile Format) and [MS-WMF] (Windows Metafile Format).
//!
//! Visio stores pictures from other tools as EMF. This converts the GDI
//! drawing records to an SVG document so every renderer can draw them as an
//! ordinary image. Supported: polygons/polylines/Béziers (16- and 32-bit),
//! poly-polygons, lines, rectangles, round rectangles, ellipses, paths
//! (fill/stroke/clip), pens, brushes, world transforms, window/viewport
//! mapping, fixed map modes, clipping rectangles/regions, DC save/restore and
//! simple text, DIB bitmaps and a basic EMF+ player (see `plus`). Not supported (skipped,
//! never an error): arcs/pies/chords, most raster operations, hatch brushes, palettes.
//!
//! Parsing never panics: every read is bounds-checked and a malformed record
//! ends the conversion with what was drawn so far.
use std::fmt::Write as _;

mod plus;

/// Result of a conversion.
#[derive(Debug, Clone)]
pub struct EmfSvg {
    pub svg: String,
    /// Picture size in CSS pixels (96 dpi), from the header's frame.
    pub width_px: f64,
    pub height_px: f64,
}

const SIGNATURE: u32 = 0x464D_4520; // " EMF"
const MAX_RECORDS: usize = 2_000_000;
/// Device coordinates are clamped to this magnitude (the picture itself may not be wider than 1e7 either, see `emf_to_svg_at`).
const MAX_DEVICE: f64 = 1e7;

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn i32_at(b: &[u8], o: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn i16_at(b: &[u8], o: usize) -> Option<i16> {
    Some(i16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn f32_at(b: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// An EMF+ ARGB colour (`0xAARRGGBB`) as `#rrggbb` and an alpha in 0..=1.
fn color_hex_alpha(raw: u32) -> (String, f64) {
    (
        format!("#{:02x}{:02x}{:02x}", (raw >> 16) & 0xff, (raw >> 8) & 0xff, raw & 0xff),
        (raw >> 24) as f64 / 255.0,
    )
}

fn png_uri_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    png_data_uri(&Dib { width, height, rgba: rgba.to_vec() })
}

fn color(raw: u32) -> String {
    format!("#{:02x}{:02x}{:02x}", raw & 0xff, (raw >> 8) & 0xff, (raw >> 16) & 0xff)
}

#[derive(Clone, Debug)]
struct Pen {
    style: u32,
    width: f64,
    color: String,
}

#[derive(Clone, Debug)]
enum Brush {
    Solid(String),
    None,
}

#[derive(Clone, Debug)]
struct Font {
    height: f64,
    escapement: f64,
    bold: bool,
    italic: bool,
    underline: bool,
    face: String,
}

#[derive(Clone, Debug)]
enum Object {
    Pen(Pen),
    Brush(Brush),
    Font(Font),
}

/// A 2x3 affine matrix: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Clone, Copy, Debug)]
struct Xf {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Xf {
    const IDENTITY: Xf = Xf { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
    /// `self` applied first, then `outer`.
    fn then(&self, outer: &Xf) -> Xf {
        Xf {
            a: outer.a * self.a + outer.c * self.b,
            b: outer.b * self.a + outer.d * self.b,
            c: outer.a * self.c + outer.c * self.d,
            d: outer.b * self.c + outer.d * self.d,
            e: outer.a * self.e + outer.c * self.f + outer.e,
            f: outer.b * self.e + outer.d * self.f + outer.f,
        }
    }
}

#[derive(Clone, Debug)]
struct Dc {
    world: Xf,
    map_mode: u32,
    win_org: (f64, f64),
    win_ext: (f64, f64),
    vp_org: (f64, f64),
    vp_ext: (f64, f64),
    pen: Pen,
    brush: Brush,
    font: Option<Font>,
    fill_mode: u32,
    text_color: String,
    text_align: u32,
    clip: Option<usize>,
    pos: (f64, f64),
}

impl Dc {
    fn new() -> Dc {
        Dc {
            world: Xf::IDENTITY,
            map_mode: 1,
            win_org: (0.0, 0.0),
            win_ext: (1.0, 1.0),
            vp_org: (0.0, 0.0),
            vp_ext: (1.0, 1.0),
            pen: Pen { style: 0, width: 1.0, color: "#000000".to_string() },
            brush: Brush::Solid("#ffffff".to_string()),
            font: None,
            fill_mode: 1,
            text_color: "#000000".to_string(),
            text_align: 0,
            clip: None,
            pos: (0.0, 0.0),
        }
    }
}

struct Converter {
    dc: Dc,
    stack: Vec<Dc>,
    objects: Vec<Option<Object>>,
    /// Pixels per millimetre of the reference device.
    px_per_mm: (f64, f64),
    /// Narrowest stroke, in device units: GDI draws any pen thinner than one
    /// output pixel exactly one pixel wide.
    min_stroke: f64,
    body: String,
    defs: String,
    clips: usize,
    path: Option<String>,
    path_open: bool,
}

impl Converter {
    /// Logical point to device pixels, kept within `MAX_DEVICE` (a record can name any 32-bit coordinate, and a path that far out
    /// makes the rasteriser panic).
    fn map(&self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = self.map_unbounded(x, y);
        let bound = |v: f64| if v.is_finite() { v.clamp(-MAX_DEVICE, MAX_DEVICE) } else { 0.0 };
        (bound(x), bound(y))
    }

    fn map_unbounded(&self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = self.dc.world.apply(x, y);
        let dc = &self.dc;
        match dc.map_mode {
            // MM_ISOTROPIC / MM_ANISOTROPIC: window to viewport.
            7 | 8 => {
                let sx = if dc.win_ext.0 != 0.0 { dc.vp_ext.0 / dc.win_ext.0 } else { 1.0 };
                let sy = if dc.win_ext.1 != 0.0 { dc.vp_ext.1 / dc.win_ext.1 } else { 1.0 };
                ((x - dc.win_org.0) * sx + dc.vp_org.0, (y - dc.win_org.1) * sy + dc.vp_org.1)
            }
            // Fixed metric/English modes: y grows upward, units relative to mm.
            2..=6 => {
                let per_mm = match dc.map_mode {
                    2 => 0.1,             // LOMETRIC: 0.1 mm
                    3 => 0.01,            // HIMETRIC: 0.01 mm
                    4 => 0.254,           // LOENGLISH: 0.01 in
                    5 => 0.0254,          // HIENGLISH: 0.001 in
                    _ => 25.4 / 1440.0,   // TWIPS
                };
                (
                    (x - dc.win_org.0) * per_mm * self.px_per_mm.0 + dc.vp_org.0,
                    -(y - dc.win_org.1) * per_mm * self.px_per_mm.1 + dc.vp_org.1,
                )
            }
            // MM_TEXT and anything unknown: one logical unit per device pixel.
            _ => (x - dc.win_org.0 + dc.vp_org.0, y - dc.win_org.1 + dc.vp_org.1),
        }
    }

    /// Average device pixels per logical unit.
    fn unit_scale(&self) -> f64 {
        let (x0, y0) = self.map(0.0, 0.0);
        let (x1, y1) = self.map(1.0, 0.0);
        let (x2, y2) = self.map(0.0, 1.0);
        let sx = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        let sy = ((x2 - x0).powi(2) + (y2 - y0).powi(2)).sqrt();
        ((sx * sy).sqrt()).max(1e-9)
    }

    fn fill_attr(&self) -> String {
        match &self.dc.brush {
            Brush::Solid(c) => format!("fill=\"{c}\""),
            Brush::None => "fill=\"none\"".to_string(),
        }
    }

    fn stroke_attr(&self) -> String {
        let pen = &self.dc.pen;
        if pen.style & 0xf == 5 {
            return "stroke=\"none\"".to_string();
        }
        // Cosmetic (width 0) pens and pens thinner than one output pixel are
        // one output pixel wide, as GDI draws them.
        let width = (pen.width * self.unit_scale()).max(self.min_stroke);
        let dash = match pen.style & 0xf {
            1 => format!(" stroke-dasharray=\"{:.2} {:.2}\"", width * 3.0, width),
            2 => format!(" stroke-dasharray=\"{:.2} {:.2}\"", width, width),
            3 => format!(" stroke-dasharray=\"{:.2} {:.2} {:.2} {:.2}\"", width * 3.0, width, width, width),
            4 => format!(
                " stroke-dasharray=\"{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}\"",
                width * 3.0, width, width, width, width, width
            ),
            _ => String::new(),
        };
        format!(
            "stroke=\"{}\" stroke-width=\"{:.3}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"{dash}",
            pen.color, width
        )
    }

    fn clip_attr(&self) -> String {
        self.dc
            .clip
            .map(|id| format!(" clip-path=\"url(#c{id})\""))
            .unwrap_or_default()
    }

    fn rule_attr(&self) -> &'static str {
        if self.dc.fill_mode == 2 { "nonzero" } else { "evenodd" }
    }

    /// Emit (or append to the open path) a shape given as device-space path data.
    fn emit(&mut self, d: &str, fill: bool, stroke: bool) {
        if let Some(path) = self.path.as_mut() {
            path.push_str(d);
            return;
        }
        let fill_attr = if fill { self.fill_attr() } else { "fill=\"none\"".to_string() };
        let stroke_attr = if stroke { self.stroke_attr() } else { "stroke=\"none\"".to_string() };
        let _ = write!(
            self.body,
            "<path d=\"{d}\" {fill_attr} fill-rule=\"{}\" {stroke_attr}{}/>",
            self.rule_attr(),
            self.clip_attr()
        );
    }

    /// `PolyLineTo` / `PolyBezierTo` continue the figure from the current point. Inside a path
    /// whose figure is already open the leading `M` must go, or every record would start its own
    /// subpath and a fill or clip would close each piece separately (a wedge across the shape).
    fn continue_open_figure(&mut self, d: String) -> String {
        if self.path.is_none() {
            return d;
        }
        let out = match (self.path_open, d.strip_prefix('M')) {
            (true, Some(rest)) => {
                let at = rest.find(|ch: char| ch.is_ascii_alphabetic()).unwrap_or(rest.len());
                rest[at..].to_string()
            }
            _ => d,
        };
        self.path_open = true;
        out
    }

    fn poly_d(&self, points: &[(f64, f64)], close: bool) -> String {
        let mut d = String::new();
        for (i, (x, y)) in points.iter().enumerate() {
            let (x, y) = self.map(*x, *y);
            let _ = write!(d, "{}{:.2} {:.2}", if i == 0 { "M" } else { "L" }, x, y);
        }
        if close && !points.is_empty() {
            d.push('Z');
        }
        d
    }

    fn bezier_d(&self, start: Option<(f64, f64)>, points: &[(f64, f64)]) -> String {
        let mut d = String::new();
        let mut rest = points;
        if let Some((x, y)) = start {
            let (x, y) = self.map(x, y);
            let _ = write!(d, "M{x:.2} {y:.2}");
        } else if let Some((first, tail)) = points.split_first() {
            let (x, y) = self.map(first.0, first.1);
            let _ = write!(d, "M{x:.2} {y:.2}");
            rest = tail;
        }
        for chunk in rest.chunks_exact(3) {
            let a = self.map(chunk[0].0, chunk[0].1);
            let b = self.map(chunk[1].0, chunk[1].1);
            let c = self.map(chunk[2].0, chunk[2].1);
            let _ = write!(
                d,
                "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
                a.0, a.1, b.0, b.1, c.0, c.1
            );
        }
        d
    }

    fn ellipse_d(&self, l: f64, t: f64, r: f64, b: f64) -> String {
        const K: f64 = 0.552_284_749_830_793_4;
        let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
        let (rx, ry) = ((r - l) / 2.0, (b - t) / 2.0);
        let p = |x: f64, y: f64| self.map(x, y);
        let (n, e, s, w) = (
            p(cx, cy - ry),
            p(cx + rx, cy),
            p(cx, cy + ry),
            p(cx - rx, cy),
        );
        let c = |x1: f64, y1: f64, x2: f64, y2: f64, x: (f64, f64)| {
            let a = p(x1, y1);
            let bb = p(x2, y2);
            format!("C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}", a.0, a.1, bb.0, bb.1, x.0, x.1)
        };
        format!(
            "M{:.2} {:.2}{}{}{}{}Z",
            n.0,
            n.1,
            c(cx + rx * K, cy - ry, cx + rx, cy - ry * K, e),
            c(cx + rx, cy + ry * K, cx + rx * K, cy + ry, s),
            c(cx - rx * K, cy + ry, cx - rx, cy + ry * K, w),
            c(cx - rx, cy - ry * K, cx - rx * K, cy - ry, n),
        )
    }

    fn rect_d(&self, l: f64, t: f64, r: f64, b: f64) -> String {
        self.poly_d(&[(l, t), (r, t), (r, b), (l, b)], true)
    }

    fn round_rect_d(&self, l: f64, t: f64, r: f64, b: f64, cw: f64, ch: f64) -> String {
        let (rx, ry) = ((cw / 2.0).min((r - l).abs() / 2.0), (ch / 2.0).min((b - t).abs() / 2.0));
        if rx <= 0.0 || ry <= 0.0 {
            return self.rect_d(l, t, r, b);
        }
        const K: f64 = 0.552_284_749_830_793_4;
        let p = |x: f64, y: f64| self.map(x, y);
        let mut d = String::new();
        let m = p(l + rx, t);
        let _ = write!(d, "M{:.2} {:.2}", m.0, m.1);
        let line = |d: &mut String, x: f64, y: f64| {
            let q = p(x, y);
            let _ = write!(d, "L{:.2} {:.2}", q.0, q.1);
        };
        let curve = |d: &mut String, a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
            let (a, b, c) = (p(a.0, a.1), p(b.0, b.1), p(c.0, c.1));
            let _ = write!(d, "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}", a.0, a.1, b.0, b.1, c.0, c.1);
        };
        line(&mut d, r - rx, t);
        curve(&mut d, (r - rx + rx * K, t), (r, t + ry - ry * K), (r, t + ry));
        line(&mut d, r, b - ry);
        curve(&mut d, (r, b - ry + ry * K), (r - rx + rx * K, b), (r - rx, b));
        line(&mut d, l + rx, b);
        curve(&mut d, (l + rx - rx * K, b), (l, b - ry + ry * K), (l, b - ry));
        line(&mut d, l, t + ry);
        curve(&mut d, (l, t + ry - ry * K), (l + rx - rx * K, t), (l + rx, t));
        d.push('Z');
        d
    }

    /// Intersect the current clip with a device-space shape. A `clip-path` on
    /// the clipping path itself intersects it with the outer clip.
    fn intersect_clip(&mut self, d: &str, rule: &str) {
        self.clips += 1;
        let id = self.clips;
        let outer = self
            .dc
            .clip
            .map(|outer| format!(" clip-path=\"url(#c{outer})\""))
            .unwrap_or_default();
        let _ = write!(
            self.defs,
            "<clipPath id=\"c{id}\"><path d=\"{d}\" clip-rule=\"{rule}\"{outer}/></clipPath>"
        );
        self.dc.clip = Some(id);
    }

    fn handle(&mut self, handle: u32) -> Option<&Object> {
        self.objects.get(handle as usize)?.as_ref()
    }

    fn set_object(&mut self, handle: u32, object: Object) {
        let index = handle as usize;
        if index > 65_536 {
            return;
        }
        if self.objects.len() <= index {
            self.objects.resize(index + 1, None);
        }
        self.objects[index] = Some(object);
    }

    fn select(&mut self, handle: u32) {
        if handle & 0x8000_0000 != 0 {
            match handle & 0x7fff_ffff {
                0 => self.dc.brush = Brush::Solid("#ffffff".into()),
                1 => self.dc.brush = Brush::Solid("#c0c0c0".into()),
                2 => self.dc.brush = Brush::Solid("#808080".into()),
                3 => self.dc.brush = Brush::Solid("#404040".into()),
                4 => self.dc.brush = Brush::Solid("#000000".into()),
                5 => self.dc.brush = Brush::None,
                6 => self.dc.pen = Pen { style: 0, width: 0.0, color: "#ffffff".into() },
                7 => self.dc.pen = Pen { style: 0, width: 0.0, color: "#000000".into() },
                8 => self.dc.pen = Pen { style: 5, width: 0.0, color: "#000000".into() },
                _ => {}
            }
            return;
        }
        match self.handle(handle).cloned() {
            Some(Object::Pen(pen)) => self.dc.pen = pen,
            Some(Object::Brush(brush)) => self.dc.brush = brush,
            Some(Object::Font(font)) => self.dc.font = Some(font),
            None => {}
        }
    }
}

fn read_points16(rec: &[u8], offset: usize, count: usize) -> Option<Vec<(f64, f64)>> {
    let mut out = Vec::with_capacity(count.min(1 << 20));
    for i in 0..count {
        out.push((
            i16_at(rec, offset + i * 4)? as f64,
            i16_at(rec, offset + i * 4 + 2)? as f64,
        ));
    }
    Some(out)
}

fn read_points32(rec: &[u8], offset: usize, count: usize) -> Option<Vec<(f64, f64)>> {
    let mut out = Vec::with_capacity(count.min(1 << 20));
    for i in 0..count {
        out.push((
            i32_at(rec, offset + i * 8)? as f64,
            i32_at(rec, offset + i * 8 + 4)? as f64,
        ));
    }
    Some(out)
}

fn read_rect(rec: &[u8], offset: usize) -> Option<(f64, f64, f64, f64)> {
    Some((
        i32_at(rec, offset)? as f64,
        i32_at(rec, offset + 4)? as f64,
        i32_at(rec, offset + 8)? as f64,
        i32_at(rec, offset + 12)? as f64,
    ))
}

/// The CSS font list for a face named in an EMF: the face itself, then a generic family so a face this machine does not have
/// (the PDU labels ask for "Gotham Medium") falls back to a sans-serif like Visio's renderer does, not to the browser's serif.
fn font_stack(face: &str) -> String {
    let lower = face.to_ascii_lowercase();
    if lower.contains("sans-serif") || lower == "serif" || lower == "monospace" || face.contains(',') {
        return face.to_string();
    }
    let generic = if ["courier", "consolas", "lucida console", "monaco", "mono"].iter().any(|m| lower.contains(m)) {
        "monospace"
    } else if ["times", "georgia", "garamond", "cambria", "palatino", "book antiqua", "century", "serif"].iter().any(|m| lower.contains(m)) && !lower.contains("sans") {
        "serif"
    } else {
        "Helvetica, Arial, sans-serif"
    };
    format!("{face}, {generic}")
}

fn escape_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Convert an EMF to SVG. Returns `None` when the data is not an EMF or has no
/// drawable extent.
pub fn emf_to_svg(bytes: &[u8]) -> Option<EmfSvg> {
    emf_to_svg_at(bytes, None)
}

/// Like [`emf_to_svg`], but with the width (in CSS pixels) the picture will be
/// shown at, so hairlines can be kept one output pixel wide like GDI does.
/// Without it the natural size from the header is assumed.
/// Whether the bytes are an enhanced metafile (record type 1 header with the ` EMF` signature), as `metafile_to_svg_at` decides.
pub fn is_enhanced_metafile(bytes: &[u8]) -> bool {
    u32_at(bytes, 0) == Some(1) && u32_at(bytes, 40) == Some(SIGNATURE)
}

/// The natural size in pixels of an EMF, from its header alone: exactly the `width_px` / `height_px` that `emf_to_svg_at` returns
/// (the conversion cannot fail once the header is accepted), without converting the drawing.
pub fn emf_natural_size(bytes: &[u8]) -> Option<(f64, f64)> {
    if u32_at(bytes, 0)? != 1 || u32_at(bytes, 40)? != SIGNATURE {
        return None;
    }
    let bounds = read_rect(bytes, 8)?;
    let frame = read_rect(bytes, 24)?;
    let dev = (i32_at(bytes, 72)? as f64, i32_at(bytes, 76)? as f64);
    let mm = (i32_at(bytes, 80)? as f64, i32_at(bytes, 84)? as f64);
    let px_per_mm = (
        if mm.0 > 0.0 { dev.0 / mm.0 } else { 3.78 },
        if mm.1 > 0.0 { dev.1 / mm.1 } else { 3.78 },
    );
    let (mut left, mut top, mut right, mut bottom) = bounds;
    if right < left || bottom < top {
        left = frame.0 / 100.0 * px_per_mm.0;
        top = frame.1 / 100.0 * px_per_mm.1;
        right = frame.2 / 100.0 * px_per_mm.0;
        bottom = frame.3 / 100.0 * px_per_mm.1;
    } else {
        right += 1.0;
        bottom += 1.0;
    }
    let (vw, vh) = (right - left, bottom - top);
    if !(vw > 0.0 && vh > 0.0) || vw > 1e7 || vh > 1e7 {
        return None;
    }
    let frame_w_mm = (frame.2 - frame.0) / 100.0;
    let frame_h_mm = (frame.3 - frame.1) / 100.0;
    Some(if frame_w_mm > 0.0 && frame_h_mm > 0.0 { (frame_w_mm * 96.0 / 25.4, frame_h_mm * 96.0 / 25.4) } else { (vw, vh) })
}

pub fn emf_to_svg_at(bytes: &[u8], display_width_px: Option<f64>) -> Option<EmfSvg> {
    if u32_at(bytes, 0)? != 1 || u32_at(bytes, 40)? != SIGNATURE {
        return None;
    }
    let bounds = read_rect(bytes, 8)?;
    let frame = read_rect(bytes, 24)?;
    let dev = (i32_at(bytes, 72)? as f64, i32_at(bytes, 76)? as f64);
    let mm = (i32_at(bytes, 80)? as f64, i32_at(bytes, 84)? as f64);
    let px_per_mm = (
        if mm.0 > 0.0 { dev.0 / mm.0 } else { 3.78 },
        if mm.1 > 0.0 { dev.1 / mm.1 } else { 3.78 },
    );

    // The drawing extent in device units. Bounds are inclusive; an inverted
    // bounds rectangle means "unknown", so fall back to the frame.
    let (mut left, mut top, mut right, mut bottom) = bounds;
    if right < left || bottom < top {
        left = frame.0 / 100.0 * px_per_mm.0;
        top = frame.1 / 100.0 * px_per_mm.1;
        right = frame.2 / 100.0 * px_per_mm.0;
        bottom = frame.3 / 100.0 * px_per_mm.1;
    } else {
        right += 1.0;
        bottom += 1.0;
        // The picture starts at the frame's corner, not at the corner of what it draws (a 15 px wide drawing that starts at pixel 1 keeps that pixel of margin).
        if frame.2 > frame.0 && frame.3 > frame.1 {
            left = left.min(frame.0 / 100.0 * px_per_mm.0);
            top = top.min(frame.1 / 100.0 * px_per_mm.1);
        }
    }
    let (vw, vh) = (right - left, bottom - top);
    if !(vw > 0.0 && vh > 0.0) || vw > 1e7 || vh > 1e7 {
        return None;
    }
    let frame_w_mm = (frame.2 - frame.0) / 100.0;
    let frame_h_mm = (frame.3 - frame.1) / 100.0;
    let (width_px, height_px) = if frame_w_mm > 0.0 && frame_h_mm > 0.0 {
        (frame_w_mm * 96.0 / 25.4, frame_h_mm * 96.0 / 25.4)
    } else {
        (vw, vh)
    };

    let handles = (bytes.get(56..58).map_or(1, |h| u16::from_le_bytes([h[0], h[1]])) as usize)
        .clamp(1, 65_536);
    let mut conv = Converter {
        dc: Dc::new(),
        stack: Vec::new(),
        objects: vec![None; handles],
        px_per_mm,
        min_stroke: vw / (display_width_px.filter(|w| *w > 0.0).unwrap_or(width_px) * HAIRLINE_OVERSAMPLE).max(1.0),
        body: String::new(),
        defs: String::new(),
        clips: 0,
        path: None,
        path_open: false,
    };

    // GDI+ pictures: play the EMF+ records when we can play all of them (the GDI records of
    // a dual file are only a crude fallback); GDI records are then used just where a GetDC
    // record asks for them (text, bitmaps).
    // LAYOUTCRAFT_NO_EMFPLUS=1 forces the GDI records (debugging aid for comparing the two).
    let mut plus_state = if std::env::var("LAYOUTCRAFT_NO_EMFPLUS").is_ok() {
        None
    } else {
        plus::prescan(bytes).map(plus::Plus::new)
    };
    let mut offset = 0usize;
    let mut records = 0usize;
    while let (Some(kind), Some(size)) = (u32_at(bytes, offset), u32_at(bytes, offset + 4)) {
        let size = size as usize;
        if size < 8 || size % 4 != 0 || offset + size > bytes.len() || records >= MAX_RECORDS {
            break;
        }
        records += 1;
        let rec = &bytes[offset..offset + size];
        offset += size;
        if kind == 14 {
            break; // EMR_EOF
        }
        if let Some(state) = plus_state.as_mut() {
            if kind == 70 && rec.get(12..16) == Some(b"EMF+") {
                plus::handle_comment(&mut conv, state, rec);
                continue;
            }
            if !state.gdi_live && kind != 1 {
                continue; // fallback GDI record of an EMF+ picture
            }
        }
        if process(&mut conv, kind, rec).is_none() {
            // A truncated or malformed record: skip it, keep going.
            continue;
        }
    }

    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\" width=\"{:.2}\" height=\"{:.2}\" preserveAspectRatio=\"none\"><defs>{}</defs>{}</svg>",
        left, top, vw, vh, width_px, height_px, conv.defs, conv.body
    );
    Some(EmfSvg { svg, width_px, height_px })
}

fn process(c: &mut Converter, kind: u32, rec: &[u8]) -> Option<()> {
    match kind {
        // --- state ---
        17 => c.dc.map_mode = u32_at(rec, 8)?,
        19 => c.dc.fill_mode = u32_at(rec, 8)?,
        22 => c.dc.text_align = u32_at(rec, 8)?,
        24 => c.dc.text_color = color(u32_at(rec, 8)?),
        9 => c.dc.win_ext = (i32_at(rec, 8)? as f64, i32_at(rec, 12)? as f64),
        10 => c.dc.win_org = (i32_at(rec, 8)? as f64, i32_at(rec, 12)? as f64),
        11 => c.dc.vp_ext = (i32_at(rec, 8)? as f64, i32_at(rec, 12)? as f64),
        12 => c.dc.vp_org = (i32_at(rec, 8)? as f64, i32_at(rec, 12)? as f64),
        33 => c.stack.push(c.dc.clone()),
        34 => {
            restore_dc(&mut c.stack, &mut c.dc, i32_at(rec, 8)?);
        }
        35 | 36 => {
            let xf = Xf {
                a: f32_at(rec, 8)? as f64,
                b: f32_at(rec, 12)? as f64,
                c: f32_at(rec, 16)? as f64,
                d: f32_at(rec, 20)? as f64,
                e: f32_at(rec, 24)? as f64,
                f: f32_at(rec, 28)? as f64,
            };
            if kind == 35 {
                c.dc.world = xf;
            } else {
                c.dc.world = match u32_at(rec, 32)? {
                    1 => Xf::IDENTITY,
                    2 => xf.then(&c.dc.world), // left multiply: new = xf x current
                    3 => c.dc.world.then(&xf),
                    4 => xf,
                    _ => c.dc.world,
                };
            }
        }
        // --- objects ---
        38 => {
            let handle = u32_at(rec, 8)?;
            let pen = Pen {
                style: u32_at(rec, 12)?,
                width: i32_at(rec, 16)? as f64,
                color: color(u32_at(rec, 24)?),
            };
            c.set_object(handle, Object::Pen(pen));
        }
        95 => {
            let handle = u32_at(rec, 8)?;
            let pen = Pen {
                style: u32_at(rec, 28)?,
                width: u32_at(rec, 32)? as f64,
                color: color(u32_at(rec, 40)?),
            };
            c.set_object(handle, Object::Pen(pen));
        }
        39 => {
            let handle = u32_at(rec, 8)?;
            let brush = match u32_at(rec, 12)? {
                0 => Brush::Solid(color(u32_at(rec, 16)?)),
                1 => Brush::None,
                // Hatched and pattern brushes: approximate with the colour.
                2 => Brush::Solid(color(u32_at(rec, 16)?)),
                _ => Brush::Solid("#808080".into()),
            };
            c.set_object(handle, Object::Brush(brush));
        }
        // CREATEDIBPATTERNBRUSHPT: a brush tiled from a small DIB. Stencil artwork uses
        // 1x1 or tiny uniform patterns for solid fills (header bars, buttons); the average
        // colour stands in for the pattern.
        94 => {
            let handle = u32_at(rec, 8)?;
            let (off_bmi, cb_bmi) = (u32_at(rec, 16)? as usize, u32_at(rec, 20)? as usize);
            let (off_bits, cb_bits) = (u32_at(rec, 24)? as usize, u32_at(rec, 28)? as usize);
            let info = rec.get(off_bmi..off_bmi.checked_add(cb_bmi)?)?;
            let bits = rec.get(off_bits..off_bits.checked_add(cb_bits)?)?;
            let dib = decode_dib(info, bits)?;
            let pixels = (dib.width as usize * dib.height as usize).max(1);
            let mut sum = [0u64; 3];
            for px in dib.rgba.chunks_exact(4) {
                sum[0] += px[0] as u64;
                sum[1] += px[1] as u64;
                sum[2] += px[2] as u64;
            }
            let avg = |v: u64| (v / pixels as u64) as u8;
            let hex = format!("#{:02x}{:02x}{:02x}", avg(sum[0]), avg(sum[1]), avg(sum[2]));
            c.set_object(handle, Object::Brush(Brush::Solid(hex)));
        }
        82 => {
            let handle = u32_at(rec, 8)?;
            let face: Vec<u16> = (0..32)
                .filter_map(|i| {
                    Some(u16::from_le_bytes([*rec.get(40 + i * 2)?, *rec.get(41 + i * 2)?]))
                })
                .take_while(|unit| *unit != 0)
                .collect();
            let font = Font {
                height: (i32_at(rec, 12)? as f64).abs(),
                escapement: i32_at(rec, 20)? as f64 / 10.0,
                bold: i32_at(rec, 28)? >= 600,
                italic: *rec.get(32)? != 0,
                underline: *rec.get(33)? != 0,
                face: String::from_utf16_lossy(&face),
            };
            c.set_object(handle, Object::Font(font));
        }
        37 => c.select(u32_at(rec, 8)?),
        40 => {
            let handle = u32_at(rec, 8)? as usize;
            if let Some(slot) = c.objects.get_mut(handle) {
                *slot = None;
            }
        }
        // --- drawing ---
        27 => {
            c.dc.pos = (i32_at(rec, 8)? as f64, i32_at(rec, 12)? as f64);
            if c.path.is_some() {
                let (x, y) = c.map(c.dc.pos.0, c.dc.pos.1);
                if let Some(path) = c.path.as_mut() {
                    let _ = write!(path, "M{x:.2} {y:.2}");
                }
                c.path_open = true;
            }
        }
        54 => {
            let to = (i32_at(rec, 8)? as f64, i32_at(rec, 12)? as f64);
            let from = c.dc.pos;
            c.dc.pos = to;
            if c.path.is_some() {
                // Inside a path the figure continues from the current point.
                let (fx, fy) = c.map(from.0, from.1);
                let (tx, ty) = c.map(to.0, to.1);
                let open = c.path_open;
                if let Some(path) = c.path.as_mut() {
                    if !open {
                        let _ = write!(path, "M{fx:.2} {fy:.2}");
                    }
                    let _ = write!(path, "L{tx:.2} {ty:.2}");
                }
                c.path_open = true;
            } else {
                let d = c.poly_d(&[from, to], false);
                c.emit(&d, false, true);
            }
        }
        3 | 86 => {
            let count = u32_at(rec, 24)? as usize;
            let pts = if kind == 86 { read_points16(rec, 28, count)? } else { read_points32(rec, 28, count)? };
            let d = c.poly_d(&pts, true);
            c.emit(&d, true, true);
        }
        4 | 87 => {
            let count = u32_at(rec, 24)? as usize;
            let pts = if kind == 87 { read_points16(rec, 28, count)? } else { read_points32(rec, 28, count)? };
            let d = c.poly_d(&pts, false);
            c.emit(&d, false, true);
        }
        6 | 89 => {
            let count = u32_at(rec, 24)? as usize;
            let pts = if kind == 89 { read_points16(rec, 28, count)? } else { read_points32(rec, 28, count)? };
            let mut all = vec![c.dc.pos];
            all.extend(pts.iter().copied());
            let d = c.poly_d(&all, false);
            if let Some(last) = pts.last() {
                c.dc.pos = *last;
            }
            let d = c.continue_open_figure(d);
            c.emit(&d, false, true);
        }
        2 | 85 => {
            let count = u32_at(rec, 24)? as usize;
            let pts = if kind == 85 { read_points16(rec, 28, count)? } else { read_points32(rec, 28, count)? };
            let d = c.bezier_d(None, &pts);
            if let Some(last) = pts.last() {
                c.dc.pos = *last;
            }
            c.emit(&d, false, true);
        }
        5 | 88 => {
            let count = u32_at(rec, 24)? as usize;
            let pts = if kind == 88 { read_points16(rec, 28, count)? } else { read_points32(rec, 28, count)? };
            let d = c.bezier_d(Some(c.dc.pos), &pts);
            if let Some(last) = pts.last() {
                c.dc.pos = *last;
            }
            let d = c.continue_open_figure(d);
            c.emit(&d, false, true);
        }
        7 | 8 | 90 | 91 => {
            // POLYPOLYLINE / POLYPOLYGON (32-bit and 16-bit).
            let narrow = kind >= 90;
            let polygon = kind == 8 || kind == 91;
            let polys = u32_at(rec, 24)? as usize;
            let total = u32_at(rec, 28)? as usize;
            if polys > 1 << 20 {
                return None;
            }
            let mut counts = Vec::with_capacity(polys);
            for i in 0..polys {
                counts.push(u32_at(rec, 32 + i * 4)? as usize);
            }
            let start = 32 + polys * 4;
            let pts = if narrow { read_points16(rec, start, total)? } else { read_points32(rec, start, total)? };
            let mut d = String::new();
            let mut at = 0usize;
            for count in counts {
                let end = (at + count).min(pts.len());
                d.push_str(&c.poly_d(&pts[at..end], polygon));
                at = end;
            }
            c.emit(&d, polygon, true);
        }
        43 => {
            let (l, t, r, b) = read_rect(rec, 8)?;
            let d = c.rect_d(l, t, r, b);
            c.emit(&d, true, true);
        }
        44 => {
            let (l, t, r, b) = read_rect(rec, 8)?;
            let d = c.round_rect_d(l, t, r, b, i32_at(rec, 24)? as f64, i32_at(rec, 28)? as f64);
            c.emit(&d, true, true);
        }
        42 => {
            let (l, t, r, b) = read_rect(rec, 8)?;
            let d = c.ellipse_d(l, t, r, b);
            c.emit(&d, true, true);
        }
        // --- paths ---
        59 => {
            c.path = Some(String::new());
            c.path_open = false;
        }
        60 => c.path_open = false,
        61 => {
            if let Some(path) = c.path.as_mut() {
                path.push('Z');
            }
        }
        62 | 63 | 64 => {
            if let Some(d) = c.path.take() {
                let fill = kind != 64;
                let stroke = kind != 62;
                c.emit(&d, fill, stroke);
            }
        }
        67 => {
            if let Some(d) = c.path.take() {
                let rule = c.rule_attr();
                c.intersect_clip(&d, rule);
            }
        }
        68 => c.path = None,
        // --- clipping ---
        30 => {
            let (l, t, r, b) = read_rect(rec, 8)?;
            let d = c.rect_d(l, t, r, b);
            c.intersect_clip(&d, "nonzero");
        }
        75 => {
            let bytes_of_region = u32_at(rec, 8)? as usize;
            let mode = u32_at(rec, 12)?;
            if bytes_of_region == 0 {
                if mode == 5 {
                    c.dc.clip = None;
                }
                return Some(());
            }
            let count = u32_at(rec, 16 + 8)? as usize;
            // Region rectangles are in device units, so they are used as they are;
            // only INTERSECTCLIPRECT carries logical coordinates.
            let mut d = String::new();
            for i in 0..count.min(1 << 16) {
                let (l, t, r, b) = read_rect(rec, 16 + 32 + i * 16)?;
                let _ = write!(d, "M{l:.2} {t:.2}L{r:.2} {t:.2}L{r:.2} {b:.2}L{l:.2} {b:.2}Z");
            }
            if d.is_empty() {
                return Some(());
            }
            if mode == 5 {
                c.dc.clip = None;
            }
            c.intersect_clip(&d, "nonzero");
        }
        // --- bitmaps ---
        76 | 77 | 81 => {
            let _ = emf_bitmap(c, kind, rec);
        }
        // --- text ---
        84 => {
            let (rx, ry) = (i32_at(rec, 36)? as f64, i32_at(rec, 40)? as f64);
            let chars = u32_at(rec, 44)? as usize;
            let off = u32_at(rec, 48)? as usize;
            if chars == 0 || chars > 1 << 16 {
                return Some(());
            }
            let units: Vec<u16> = (0..chars)
                .filter_map(|i| Some(u16::from_le_bytes([*rec.get(off + i * 2)?, *rec.get(off + i * 2 + 1)?])))
                .collect();
            let text = String::from_utf16_lossy(&units);
            if text.trim().is_empty() {
                return Some(());
            }
            let font = c.dc.font.clone().unwrap_or(Font {
                height: 12.0,
                escapement: 0.0,
                bold: false,
                italic: false,
                underline: false,
                face: "sans-serif".into(),
            });
            let size = (font.height * c.unit_scale()).max(1.0);
            let (x, y) = c.map(rx, ry);
            let align = c.dc.text_align;
            let anchor = match align & 6 {
                6 => "middle",
                2 => "end",
                _ => "start",
            };
            let baseline = match align & 24 {
                24 => 0.0,        // TA_BASELINE
                8 => -size * 0.2, // TA_BOTTOM
                _ => size * 0.8,  // TA_TOP
            };
            let rotate = if font.escapement.abs() > 0.01 {
                format!(" transform=\"rotate({:.2} {:.2} {:.2})\"", -font.escapement, x, y + baseline)
            } else {
                String::new()
            };
            let decoration = if font.underline { " text-decoration=\"underline\"" } else { "" };
            let _ = write!(
                c.body,
                "<text x=\"{:.2}\" y=\"{:.2}\" font-family=\"{}\" font-size=\"{:.2}\"{}{} text-anchor=\"{anchor}\" fill=\"{}\"{decoration}{rotate}{}>{}</text>",
                x,
                y + baseline,
                escape_text(&font_stack(&font.face)),
                size,
                if font.bold { " font-weight=\"bold\"" } else { "" },
                if font.italic { " font-style=\"italic\"" } else { "" },
                c.dc.text_color,
                c.clip_attr(),
                escape_text(&text)
            );
        }
        _ => {}
    }
    Some(())
}

// ---------------------------------------------------------------------------
// WMF (Windows Metafile)
// ---------------------------------------------------------------------------

/// Convert either metafile flavour: EMF (header signature) or 16-bit WMF, with or
/// without the Aldus "placeable" header. Visio itself stores legacy metafile
/// pictures as plain WMF under an `.emf` name, so the content, not the extension,
/// decides.
pub fn metafile_to_svg_at(bytes: &[u8], display_width_px: Option<f64>) -> Option<EmfSvg> {
    if u32_at(bytes, 0) == Some(1) && u32_at(bytes, 40) == Some(SIGNATURE) {
        return emf_to_svg_at(bytes, display_width_px);
    }
    wmf_to_svg_at(bytes, display_width_px)
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

/// WMF drawing parameters are stored in reverse order; this reads the n-th
/// 16-bit parameter counted from the end of the record, as a signed value.
fn param_from_end(rec: &[u8], n: usize) -> Option<f64> {
    let end = rec.len();
    let at = end.checked_sub(2 * (n + 1))?;
    if at < 6 {
        return None;
    }
    Some(i16::from_le_bytes([rec[at], rec[at + 1]]) as f64)
}

/// Convert a 16-bit WMF to SVG. See [`emf_to_svg_at`] for `display_width_px`.
pub fn wmf_to_svg_at(bytes: &[u8], display_width_px: Option<f64>) -> Option<EmfSvg> {
    // Optional placeable header (22 bytes) giving the logical box and units/inch.
    let placeable = u32_at(bytes, 0) == Some(0x9AC6_CDD7);
    let (start, bbox, inch) = if placeable {
        let l = i16_at(bytes, 6)? as f64;
        let t = i16_at(bytes, 8)? as f64;
        let r = i16_at(bytes, 10)? as f64;
        let b = i16_at(bytes, 12)? as f64;
        (22usize, Some((l, t, r, b)), u16_at(bytes, 14)? as f64)
    } else {
        (0usize, None, 0.0)
    };
    // METAHEADER: type 1 (memory) or 2 (disk), header size 9 words.
    let kind = u16_at(bytes, start)?;
    let header_words = u16_at(bytes, start + 2)? as usize;
    if !(kind == 1 || kind == 2) || header_words != 9 {
        return None;
    }
    let first_record = start + header_words * 2;

    // Pass 1: the first window origin/extent give the drawing's coordinate box.
    let mut win_org = (0.0, 0.0);
    let mut win_ext: Option<(f64, f64)> = None;
    let mut offset = first_record;
    let mut count = 0usize;
    while let (Some(words), Some(func)) = (u32_at(bytes, offset), u16_at(bytes, offset + 4)) {
        let size = words as usize * 2;
        if words < 3 || offset + size > bytes.len() || count >= MAX_RECORDS {
            break;
        }
        count += 1;
        let rec = &bytes[offset..offset + size];
        match func {
            0x020B if win_ext.is_none() => {
                // SETWINDOWORG stores y then x.
                if let (Some(x), Some(y)) = (param_from_end(rec, 0), param_from_end(rec, 1)) {
                    win_org = (x, y);
                }
            }
            0x020C if win_ext.is_none() => {
                if let (Some(x), Some(y)) = (param_from_end(rec, 0), param_from_end(rec, 1)) {
                    win_ext = Some((x, y));
                }
            }
            _ => {}
        }
        if func == 0 {
            break;
        }
        offset += size;
    }
    let (ex, ey) = match (win_ext, bbox) {
        (Some(e), _) => e,
        (None, Some((l, t, r, b))) => (r - l, b - t),
        _ => return None,
    };
    if ex == 0.0 || ey == 0.0 || ex.abs() > 1e7 || ey.abs() > 1e7 {
        return None;
    }
    let (vw, vh) = (ex.abs(), ey.abs());
    // Natural size: placeable files carry units/inch; plain ones are taken as pixels.
    let (width_px, height_px) = match (bbox, inch > 0.0) {
        (Some((l, t, r, b)), true) => (((r - l).abs() / inch) * 96.0, ((b - t).abs() / inch) * 96.0),
        _ => (vw, vh),
    };

    let mut conv = Converter {
        dc: Dc::new(),
        stack: Vec::new(),
        objects: Vec::new(),
        px_per_mm: (3.7795, 3.7795),
        min_stroke: vw / (display_width_px.filter(|w| *w > 0.0).unwrap_or(width_px) * HAIRLINE_OVERSAMPLE).max(1.0),
        body: String::new(),
        defs: String::new(),
        clips: 0,
        path: None,
        path_open: false,
    };
    // Logical units -> viewBox units through the window mapping. The viewport is
    // the positive viewBox with its origin at 0,0, so a negative window extent
    // gives a negative scale and flips that axis, as GDI does.
    conv.dc.map_mode = 8;
    conv.dc.win_org = win_org;
    conv.dc.win_ext = (ex, ey);
    conv.dc.vp_org = (0.0, 0.0);
    conv.dc.vp_ext = (vw, vh);

    let mut offset = first_record;
    let mut count = 0usize;
    while let (Some(words), Some(func)) = (u32_at(bytes, offset), u16_at(bytes, offset + 4)) {
        let size = words as usize * 2;
        if words < 3 || offset + size > bytes.len() || count >= MAX_RECORDS {
            break;
        }
        count += 1;
        let rec = &bytes[offset..offset + size];
        offset += size;
        if func == 0 {
            break;
        }
        let _ = wmf_record(&mut conv, func, rec);
    }

    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {:.2} {:.2}\" width=\"{:.2}\" height=\"{:.2}\" preserveAspectRatio=\"none\"><defs>{}</defs>{}</svg>",
        vw, vh, width_px, height_px, conv.defs, conv.body
    );
    Some(EmfSvg { svg, width_px, height_px })
}

fn wmf_points(rec: &[u8], at: usize, count: usize) -> Option<Vec<(f64, f64)>> {
    let mut out = Vec::with_capacity(count.min(1 << 16));
    for i in 0..count {
        out.push((i16_at(rec, at + i * 4)? as f64, i16_at(rec, at + i * 4 + 2)? as f64));
    }
    Some(out)
}

fn wmf_color(rec: &[u8], at: usize) -> Option<String> {
    Some(color(u32_at(rec, at)? & 0x00ff_ffff))
}

fn wmf_add_object(c: &mut Converter, object: Object) {
    let slot = c.objects.iter().position(Option::is_none).unwrap_or(c.objects.len());
    c.set_object(slot as u32, object);
}

fn wmf_record(c: &mut Converter, func: u16, rec: &[u8]) -> Option<()> {
    // Parameters start after the 6-byte record header.
    match func {
        0x0103 => c.dc.map_mode = u16_at(rec, 6)? as u32, // SETMAPMODE
        0x0106 => c.dc.fill_mode = u16_at(rec, 6)? as u32, // SETPOLYFILLMODE
        0x0209 => c.dc.text_color = wmf_color(rec, 6)?,   // SETTEXTCOLOR
        0x012E => c.dc.text_align = u16_at(rec, 6)? as u32, // SETTEXTALIGN
        0x020B => c.dc.win_org = (param_from_end(rec, 0)?, param_from_end(rec, 1)?),
        0x020C => c.dc.win_ext = (param_from_end(rec, 0)?, param_from_end(rec, 1)?),
        0x001E => c.stack.push(c.dc.clone()), // SAVEDC
        0x0127 => {
            // RESTOREDC: a negative index counts back from the top.
            restore_dc(&mut c.stack, &mut c.dc, i32::from(i16_at(rec, 6)?));
        }
        // Objects: a new object takes the lowest free slot.
        0x02FA => {
            let pen = Pen {
                style: u16_at(rec, 6)? as u32,
                width: i16_at(rec, 8)? as f64,
                color: wmf_color(rec, 12)?,
            };
            wmf_add_object(c, Object::Pen(pen));
        }
        0x02FC => {
            let brush = match u16_at(rec, 6)? {
                0 | 2 => Brush::Solid(wmf_color(rec, 8)?),
                1 => Brush::None,
                _ => Brush::Solid("#808080".into()),
            };
            wmf_add_object(c, Object::Brush(brush));
        }
        0x02FB => {
            let face: String = rec
                .get(6 + 18..)
                .unwrap_or(&[])
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| *byte as char)
                .collect();
            let font = Font {
                height: (i16_at(rec, 6)? as f64).abs(),
                escapement: i16_at(rec, 10)? as f64 / 10.0,
                bold: i16_at(rec, 14)? >= 600,
                italic: *rec.get(6 + 10)? != 0,
                underline: *rec.get(6 + 11)? != 0,
                face,
            };
            wmf_add_object(c, Object::Font(font));
        }
        // Palettes, regions and pattern brushes still occupy an object slot.
        0x00F7 | 0x06FF | 0x01F9 | 0x0142 => {
            wmf_add_object(c, Object::Brush(Brush::Solid("#808080".into())))
        }
        0x012D => c.select(u16_at(rec, 6)? as u32),
        0x01F0 => {
            let index = u16_at(rec, 6)? as usize;
            if let Some(slot) = c.objects.get_mut(index) {
                *slot = None;
            }
        }
        // Drawing.
        0x0214 => c.dc.pos = (param_from_end(rec, 0)?, param_from_end(rec, 1)?),
        0x0213 => {
            let to = (param_from_end(rec, 0)?, param_from_end(rec, 1)?);
            let from = c.dc.pos;
            c.dc.pos = to;
            let d = c.poly_d(&[from, to], false);
            c.emit(&d, false, true);
        }
        0x0325 | 0x0324 => {
            let count = u16_at(rec, 6)? as usize;
            let pts = wmf_points(rec, 8, count)?;
            let polygon = func == 0x0324;
            let d = c.poly_d(&pts, polygon);
            c.emit(&d, polygon, true);
        }
        0x0538 => {
            let polys = u16_at(rec, 6)? as usize;
            let mut counts = Vec::with_capacity(polys.min(1 << 16));
            for i in 0..polys {
                counts.push(u16_at(rec, 8 + i * 2)? as usize);
            }
            let mut at = 8 + polys * 2;
            let mut d = String::new();
            for count in counts {
                let pts = wmf_points(rec, at, count)?;
                at += count * 4;
                d.push_str(&c.poly_d(&pts, true));
            }
            c.emit(&d, true, true);
        }
        0x041B | 0x0418 | 0x0416 => {
            // RECTANGLE / ELLIPSE / INTERSECTCLIPRECT are stored bottom, right,
            // top, left, so counting from the end the order is left, top, right,
            // bottom.
            let (l, t) = (param_from_end(rec, 0)?, param_from_end(rec, 1)?);
            let (r, b) = (param_from_end(rec, 2)?, param_from_end(rec, 3)?);
            match func {
                0x041B => {
                    let d = c.rect_d(l, t, r, b);
                    c.emit(&d, true, true);
                }
                0x0418 => {
                    let d = c.ellipse_d(l, t, r, b);
                    c.emit(&d, true, true);
                }
                _ => {
                    let d = c.rect_d(l, t, r, b);
                    c.intersect_clip(&d, "nonzero");
                }
            }
        }
        0x061C => {
            // ROUNDRECT is stored corner height, corner width, bottom, right,
            // top, left; from the end: left, top, right, bottom, width, height.
            let (l, t) = (param_from_end(rec, 0)?, param_from_end(rec, 1)?);
            let (r, b) = (param_from_end(rec, 2)?, param_from_end(rec, 3)?);
            let (w, h) = (param_from_end(rec, 4)?, param_from_end(rec, 5)?);
            let d = c.round_rect_d(l, t, r, b, w, h);
            c.emit(&d, true, true);
        }
        // Bitmaps.
        0x0F43 | 0x0B41 | 0x0940 => {
            let _ = wmf_bitmap(c, func, rec);
        }
        // EXTTEXTOUT: y, x, count, options, [rect], string.
        0x0A32 => {
            let y = i16_at(rec, 6)? as f64;
            let x = i16_at(rec, 8)? as f64;
            let chars = u16_at(rec, 10)? as usize;
            let options = u16_at(rec, 12)?;
            let text_at = 14 + if options & 0x6 != 0 { 8 } else { 0 };
            wmf_text(c, x, y, rec.get(text_at..text_at + chars)?);
        }
        // TEXTOUT: count, string padded to even, y, x.
        0x0521 => {
            let chars = u16_at(rec, 6)? as usize;
            let padded = chars + chars % 2;
            let text = rec.get(8..8 + chars)?;
            let y = i16_at(rec, 8 + padded)? as f64;
            let x = i16_at(rec, 10 + padded)? as f64;
            wmf_text(c, x, y, text);
        }
        _ => {}
    }
    Some(())
}

fn wmf_text(c: &mut Converter, x: f64, y: f64, bytes: &[u8]) {
    let text: String = bytes.iter().map(|byte| *byte as char).collect();
    if text.trim().is_empty() {
        return;
    }
    let font = c.dc.font.clone().unwrap_or(Font {
        height: 12.0,
        escapement: 0.0,
        bold: false,
        italic: false,
        underline: false,
        face: "sans-serif".into(),
    });
    let size = (font.height * c.unit_scale()).max(1.0);
    let (px, py) = c.map(x, y);
    let anchor = match c.dc.text_align & 6 {
        6 => "middle",
        2 => "end",
        _ => "start",
    };
    let baseline = match c.dc.text_align & 24 {
        24 => 0.0,
        8 => -size * 0.2,
        _ => size * 0.8,
    };
    let _ = write!(
        c.body,
        "<text x=\"{:.2}\" y=\"{:.2}\" font-family=\"{}\" font-size=\"{:.2}\"{}{} text-anchor=\"{anchor}\" fill=\"{}\"{}>{}</text>",
        px,
        py + baseline,
        escape_text(&font_stack(&font.face)),
        size,
        if font.bold { " font-weight=\"bold\"" } else { "" },
        if font.italic { " font-style=\"italic\"" } else { "" },
        c.dc.text_color,
        c.clip_attr(),
        escape_text(&text)
    );
}

// ---------------------------------------------------------------------------
// Device-independent bitmaps (BITBLT / STRETCHDIBITS and the WMF equivalents)
// ---------------------------------------------------------------------------

/// Hairlines are kept this many times finer than one display pixel, so a picture shown at a higher
/// resolution than its nominal size (Visio exports icons at 300 dpi) keeps its thin pens thin.
const HAIRLINE_OVERSAMPLE: f64 = 3.0;
const SRCCOPY: u32 = 0x00CC_0020;
const NOTSRCCOPY: u32 = 0x0033_0008;
const SRCPAINT: u32 = 0x00EE_0086;
const SRCAND: u32 = 0x0088_00C6;
const BLACKNESS: u32 = 0x0000_0042;
const WHITENESS: u32 = 0x00FF_0062;
const PATCOPY: u32 = 0x00F0_0021;

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

/// A decoded bitmap: RGBA, top row first.
struct Dib {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Decode an uncompressed (BI_RGB) DIB with 1, 4, 8, 24 or 32 bits per pixel.
/// `info` starts at the BITMAPINFOHEADER and includes any colour table; `bits`
/// is the pixel data. Returns `None` for anything it cannot decode.
fn decode_dib(info: &[u8], bits: &[u8]) -> Option<Dib> {
    let header = u32_at(info, 0)? as usize;
    if header < 40 {
        return None;
    }
    let width = i32_at(info, 4)?;
    let raw_height = i32_at(info, 8)?;
    let bit_count = u16_at(info, 14)? as u32;
    let compression = u32_at(info, 16)?;
    let colors_used = u32_at(info, 32)? as usize;
    if width <= 0 || raw_height == 0 || compression != 0 {
        return None;
    }
    let (width, height, top_down) = (width as u32, raw_height.unsigned_abs(), raw_height < 0);
    if width > 16_384 || height > 16_384 || (width as u64) * (height as u64) > 64_000_000 {
        return None;
    }
    let palette_len = match bit_count {
        1 | 4 | 8 => {
            if colors_used > 0 { colors_used } else { 1usize << bit_count }
        }
        24 | 32 => 0,
        _ => return None,
    };
    let palette: Vec<[u8; 3]> = (0..palette_len)
        .map(|i| {
            let at = header + i * 4;
            Some([*info.get(at + 2)?, *info.get(at + 1)?, *info.get(at)?])
        })
        .collect::<Option<_>>()
        .unwrap_or_default();
    if bit_count <= 8 && palette.is_empty() {
        return None;
    }
    let stride = ((width as usize * bit_count as usize + 31) / 32) * 4;
    if bits.len() < stride * height as usize {
        return None;
    }
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let src_row = if top_down { y } else { height as usize - 1 - y };
        let row = &bits[src_row * stride..src_row * stride + stride];
        for x in 0..width as usize {
            let (r, g, b) = match bit_count {
                32 => (row[x * 4 + 2], row[x * 4 + 1], row[x * 4]),
                24 => (row[x * 3 + 2], row[x * 3 + 1], row[x * 3]),
                8 => {
                    let c = palette.get(row[x] as usize).copied().unwrap_or([0, 0, 0]);
                    (c[0], c[1], c[2])
                }
                4 => {
                    let nibble = if x % 2 == 0 { row[x / 2] >> 4 } else { row[x / 2] & 0xf };
                    let c = palette.get(nibble as usize).copied().unwrap_or([0, 0, 0]);
                    (c[0], c[1], c[2])
                }
                _ => {
                    let bit = (row[x / 8] >> (7 - x % 8)) & 1;
                    let c = palette.get(bit as usize).copied().unwrap_or([0, 0, 0]);
                    (c[0], c[1], c[2])
                }
            };
            let at = (y * width as usize + x) * 4;
            rgba[at..at + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    Some(Dib { width, height, rgba })
}

/// Crop a rectangle (clamped to the bitmap) out of a decoded DIB.
fn crop_dib(dib: &Dib, x: i64, y: i64, w: i64, h: i64) -> Option<Dib> {
    let x0 = x.clamp(0, dib.width as i64) as usize;
    let y0 = y.clamp(0, dib.height as i64) as usize;
    let x1 = (x + w).clamp(0, dib.width as i64) as usize;
    let y1 = (y + h).clamp(0, dib.height as i64) as usize;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for row in y0..y1 {
        let from = (row * dib.width as usize + x0) * 4;
        rgba.extend_from_slice(&dib.rgba[from..from + (x1 - x0) * 4]);
    }
    Some(Dib { width: (x1 - x0) as u32, height: (y1 - y0) as u32, rgba })
}

fn png_data_uri(dib: &Dib) -> Option<String> {
    let image = image::RgbaImage::from_raw(dib.width, dib.height, dib.rgba.clone())?;
    let mut bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", base64(&bytes)))
}

impl Converter {
    /// Draw a decoded bitmap into the destination rectangle (logical units). The
    /// raster operation decides how: plain copies and inverted copies are drawn,
    /// an OR mask paints with black treated as transparent, black-and-white AND masks
    /// are skipped (they only punch holes for the OR pass), coloured AND bitmaps are
    /// drawn with white as transparent.
    fn draw_bitmap(&mut self, dest: (f64, f64, f64, f64), mut dib: Dib, rop: u32) {
        match rop {
            SRCAND => {
                // A pure black-and-white AND bitmap only punches holes for an OR pass that
                // follows. A coloured one is the picture itself (the OR pass, if any, came
                // first): AND with white leaves the destination alone, so white is transparent.
                let coloured = dib.rgba.chunks_exact(4).any(|px| {
                    let (r, g, b) = (px[0], px[1], px[2]);
                    !(r == g && g == b && (r == 0 || r == 255))
                });
                if !coloured {
                    return;
                }
                for px in dib.rgba.chunks_exact_mut(4) {
                    if px[0] == 255 && px[1] == 255 && px[2] == 255 {
                        px[3] = 0;
                    }
                }
            }
            NOTSRCCOPY => {
                for px in dib.rgba.chunks_exact_mut(4) {
                    px[0] = 255 - px[0];
                    px[1] = 255 - px[1];
                    px[2] = 255 - px[2];
                }
            }
            SRCPAINT => {
                for px in dib.rgba.chunks_exact_mut(4) {
                    if px[0] == 0 && px[1] == 0 && px[2] == 0 {
                        px[3] = 0;
                    }
                }
            }
            // SRCCOPY and anything unrecognised: draw the bitmap as it is.
            SRCCOPY => {}
            _ => {}
        }
        let (x, y, w, h) = dest;
        let (ax, ay) = self.map(x, y);
        let (bx, by) = self.map(x + w, y + h);
        let (left, top) = (ax.min(bx), ay.min(by));
        let (width, height) = ((bx - ax).abs(), (by - ay).abs());
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let Some(uri) = png_data_uri(&dib) else {
            return;
        };
        // A negative extent on either axis mirrors the picture.
        let flip_x = (bx - ax) < 0.0;
        let flip_y = (by - ay) < 0.0;
        let transform = if flip_x || flip_y {
            format!(
                " transform=\"translate({:.2} {:.2}) scale({} {}) translate({:.2} {:.2})\"",
                left + if flip_x { width } else { 0.0 },
                top + if flip_y { height } else { 0.0 },
                if flip_x { -1 } else { 1 },
                if flip_y { -1 } else { 1 },
                -left,
                -top
            )
        } else {
            String::new()
        };
        let _ = write!(
            self.body,
            "<image x=\"{left:.2}\" y=\"{top:.2}\" width=\"{width:.2}\" height=\"{height:.2}\" preserveAspectRatio=\"none\" image-rendering=\"pixelated\"{transform}{} href=\"{uri}\"/>",
            self.clip_attr()
        );
    }

    /// A raster operation with no source bitmap: only the constant fills are drawn.
    fn fill_rect_rop(&mut self, dest: (f64, f64, f64, f64), rop: u32) {
        let colour = match rop {
            BLACKNESS => "#000000".to_string(),
            WHITENESS => "#ffffff".to_string(),
            PATCOPY => match &self.dc.brush {
                Brush::Solid(c) => c.clone(),
                Brush::None => return,
            },
            _ => return,
        };
        let (x, y, w, h) = dest;
        let d = self.rect_d(x, y, x + w, y + h);
        let _ = write!(
            self.body,
            "<path d=\"{d}\" fill=\"{colour}\" stroke=\"none\"{}/>",
            self.clip_attr()
        );
    }
}

/// EMR_STRETCHDIBITS (81), EMR_BITBLT (76), EMR_STRETCHBLT (77).
fn emf_bitmap(c: &mut Converter, kind: u32, rec: &[u8]) -> Option<()> {
    let (dest, src, rop, bmi_off, bmi_len, bits_off, bits_len) = match kind {
        81 => (
            (i32_at(rec, 24)? as f64, i32_at(rec, 28)? as f64, i32_at(rec, 72)? as f64, i32_at(rec, 76)? as f64),
            (i32_at(rec, 32)? as i64, i32_at(rec, 36)? as i64, i32_at(rec, 40)? as i64, i32_at(rec, 44)? as i64),
            u32_at(rec, 68)?,
            u32_at(rec, 48)? as usize,
            u32_at(rec, 52)? as usize,
            u32_at(rec, 56)? as usize,
            u32_at(rec, 60)? as usize,
        ),
        _ => {
            let (cx_dest, cy_dest) = (i32_at(rec, 32)?, i32_at(rec, 36)?);
            // BITBLT has no separate source size; STRETCHBLT appends it.
            let (cx_src, cy_src) = if kind == 77 {
                (i32_at(rec, 100)? as i64, i32_at(rec, 104)? as i64)
            } else {
                (cx_dest as i64, cy_dest as i64)
            };
            (
                (i32_at(rec, 24)? as f64, i32_at(rec, 28)? as f64, cx_dest as f64, cy_dest as f64),
                (i32_at(rec, 44)? as i64, i32_at(rec, 48)? as i64, cx_src, cy_src),
                u32_at(rec, 40)?,
                u32_at(rec, 84)? as usize,
                u32_at(rec, 88)? as usize,
                u32_at(rec, 92)? as usize,
                u32_at(rec, 96)? as usize,
            )
        }
    };
    if bmi_len == 0 || bits_len == 0 {
        c.fill_rect_rop(dest, rop);
        return Some(());
    }
    let info = rec.get(bmi_off..bmi_off.checked_add(bmi_len)?)?;
    let bits = rec.get(bits_off..bits_off.checked_add(bits_len)?)?;
    let dib = decode_dib(info, bits)?;
    let dib = if src.2 > 0 && src.3 > 0 { crop_dib(&dib, src.0, src.1, src.2, src.3)? } else { dib };
    c.draw_bitmap(dest, dib, rop);
    Some(())
}

/// WMF DIB records. `dib` starts at the BITMAPINFOHEADER and runs to the end of
/// the record; the pixel data follows the header and colour table.
fn wmf_bitmap(c: &mut Converter, func: u16, rec: &[u8]) -> Option<()> {
    // Parameters are stored in reverse order; read them from the front here.
    let (rop, dest, src, dib_at) = match func {
        // STRETCHDIB: rop(4) usage(2) srcH srcW srcY srcX destH destW destY destX
        0x0F43 => {
            let rop = u32_at(rec, 6)?;
            let (src_h, src_w, src_y, src_x) = (i16_at(rec, 12)?, i16_at(rec, 14)?, i16_at(rec, 16)?, i16_at(rec, 18)?);
            let (dest_h, dest_w, dest_y, dest_x) = (i16_at(rec, 20)?, i16_at(rec, 22)?, i16_at(rec, 24)?, i16_at(rec, 26)?);
            (
                rop,
                (dest_x as f64, dest_y as f64, dest_w as f64, dest_h as f64),
                (src_x as i64, src_y as i64, src_w as i64, src_h as i64),
                28usize,
            )
        }
        // DIBSTRETCHBLT: rop(4) srcH srcW srcY srcX destH destW destY destX
        0x0B41 => {
            let rop = u32_at(rec, 6)?;
            let (src_h, src_w, src_y, src_x) = (i16_at(rec, 10)?, i16_at(rec, 12)?, i16_at(rec, 14)?, i16_at(rec, 16)?);
            let (dest_h, dest_w, dest_y, dest_x) = (i16_at(rec, 18)?, i16_at(rec, 20)?, i16_at(rec, 22)?, i16_at(rec, 24)?);
            (
                rop,
                (dest_x as f64, dest_y as f64, dest_w as f64, dest_h as f64),
                (src_x as i64, src_y as i64, src_w as i64, src_h as i64),
                26usize,
            )
        }
        // DIBBITBLT: rop(4) srcY srcX height width destY destX [dib | reserved]
        0x0940 => {
            let rop = u32_at(rec, 6)?;
            let (src_y, src_x, h, w) = (i16_at(rec, 10)?, i16_at(rec, 12)?, i16_at(rec, 14)?, i16_at(rec, 16)?);
            let (dest_y, dest_x) = (i16_at(rec, 18)?, i16_at(rec, 20)?);
            (
                rop,
                (dest_x as f64, dest_y as f64, w as f64, h as f64),
                (src_x as i64, src_y as i64, w as i64, h as i64),
                22usize,
            )
        }
        _ => return None,
    };
    if rec.len() <= dib_at + 40 {
        // No bitmap in the record: only constant-colour raster operations.
        c.fill_rect_rop(dest, rop);
        return Some(());
    }
    let info_all = rec.get(dib_at..)?;
    let header = u32_at(info_all, 0)? as usize;
    let bit_count = u16_at(info_all, 14)? as u32;
    let colors_used = u32_at(info_all, 32)? as usize;
    let palette = match bit_count {
        1 | 4 | 8 => (if colors_used > 0 { colors_used } else { 1usize << bit_count }) * 4,
        _ => 0,
    };
    let bits_at = header.checked_add(palette)?;
    let info = info_all.get(..bits_at)?;
    let bits = info_all.get(bits_at..)?;
    let dib = decode_dib(info, bits)?;
    let dib = if src.2 > 0 && src.3 > 0 { crop_dib(&dib, src.0, src.1, src.2, src.3)? } else { dib };
    c.draw_bitmap(dest, dib, rop);
    Some(())
}

/// RESTOREDC: a negative `relative` pops that many saved states (the last one popped becomes current), anything else pops one.
/// The count comes from the file: it is capped at what is on the stack (a count of -2^31 used to loop 2^31 times, and negating
/// it overflowed), and the states are removed in one step.
fn restore_dc<T>(stack: &mut Vec<T>, dc: &mut T, relative: i32) {
    let wanted = if relative < 0 { relative.unsigned_abs() as usize } else { 1 };
    let keep = stack.len().saturating_sub(wanted);
    if let Some(restored) = stack.drain(keep..).next() {
        *dc = restored;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: u32, payload: &[u8]) -> Vec<u8> {
        let mut size = 8 + payload.len();
        let pad = (4 - size % 4) % 4;
        size += pad;
        let mut out = Vec::new();
        out.extend(kind.to_le_bytes());
        out.extend((size as u32).to_le_bytes());
        out.extend(payload);
        out.extend(std::iter::repeat(0u8).take(pad));
        out
    }

    fn header(w: i32, h: i32) -> Vec<u8> {
        let mut p = Vec::new();
        for v in [0, 0, w - 1, h - 1] {
            p.extend(v.to_le_bytes()); // bounds
        }
        for v in [0, 0, w * 10, h * 10] {
            p.extend(v.to_le_bytes()); // frame, 0.01 mm
        }
        p.extend(SIGNATURE.to_le_bytes());
        p.extend(0x0001_0000u32.to_le_bytes());
        p.extend(0u32.to_le_bytes()); // bytes
        p.extend(0u32.to_le_bytes()); // records
        p.extend(8u16.to_le_bytes()); // handles
        p.extend(0u16.to_le_bytes());
        p.extend([0u8; 12]); // description, pal
        for v in [w, h, w / 10, h / 10] {
            p.extend(v.to_le_bytes()); // device px, mm
        }
        rec(1, &p)
    }

    fn polygon16(points: &[(i16, i16)]) -> Vec<u8> {
        let mut p = vec![0u8; 16];
        p.extend((points.len() as u32).to_le_bytes());
        for (x, y) in points {
            p.extend(x.to_le_bytes());
            p.extend(y.to_le_bytes());
        }
        rec(86, &p)
    }

    #[test]
    fn rejects_non_emf() {
        assert!(emf_to_svg(b"not an emf").is_none());
        assert!(emf_to_svg(&[]).is_none());
    }

    #[test]
    fn draws_a_filled_polygon_with_brush_and_pen() {
        let mut data = header(100, 50);
        data.extend(rec(39, &[1, 0, 0, 0, 0, 0, 0, 0, 0xff, 0x00, 0x00, 0x00, 0, 0, 0, 0])); // brush 1 = red (BGR)
        data.extend(rec(37, &1u32.to_le_bytes()));
        data.extend(polygon16(&[(0, 0), (10, 0), (10, 10)]));
        data.extend(rec(14, &[0u8; 12]));
        let svg = emf_to_svg(&data).expect("svg");
        assert!(svg.svg.contains("viewBox=\"0.00 0.00 100.00 50.00\""), "{}", svg.svg);
        assert!(svg.svg.contains("fill=\"#ff0000\""), "{}", svg.svg);
        assert!(svg.svg.contains("M0.00 0.00L10.00 0.00L10.00 10.00Z"), "{}", svg.svg);
    }

    #[test]
    fn window_viewport_scales_coordinates() {
        let mut data = header(100, 100);
        data.extend(rec(17, &8u32.to_le_bytes())); // anisotropic
        data.extend(rec(9, &[10, 0, 0, 0, 10, 0, 0, 0])); // window ext 10x10
        data.extend(rec(11, &[100, 0, 0, 0, 100, 0, 0, 0])); // viewport ext 100x100
        data.extend(polygon16(&[(0, 0), (5, 0), (5, 5)]));
        let svg = emf_to_svg(&data).unwrap();
        assert!(svg.svg.contains("M0.00 0.00L50.00 0.00L50.00 50.00Z"), "{}", svg.svg);
    }

    #[test]
    fn restore_dc_with_a_huge_negative_count_is_bounded() {
        // SaveDC twice, then RestoreDC(-2^31): it used to overflow on negation and loop 2^31 times
        let mut data = header(10, 10);
        data.extend(rec(33, &[]));
        data.extend(rec(33, &[]));
        data.extend(rec(34, &i32::MIN.to_le_bytes()));
        data.extend(polygon16(&[(0, 0), (5, 0), (5, 5)]));
        let svg = emf_to_svg(&data).expect("svg");
        assert!(svg.svg.contains("M0.00 0.00L5.00 0.00L5.00 5.00Z"), "{}", svg.svg);
    }

    #[test]
    fn restore_dc_minus_one_brings_back_the_saved_mapping() {
        let mut data = header(100, 100);
        data.extend(rec(33, &[])); // save the unscaled state
        data.extend(rec(17, &8u32.to_le_bytes())); // anisotropic
        data.extend(rec(9, &[10, 0, 0, 0, 10, 0, 0, 0])); // window ext 10x10
        data.extend(rec(11, &[100, 0, 0, 0, 100, 0, 0, 0])); // viewport ext 100x100: 10x scale
        data.extend(rec(34, &(-1i32).to_le_bytes())); // back to the saved state
        data.extend(polygon16(&[(0, 0), (5, 0), (5, 5)]));
        let svg = emf_to_svg(&data).unwrap();
        assert!(svg.svg.contains("M0.00 0.00L5.00 0.00L5.00 5.00Z"), "the scale was not undone: {}", svg.svg);
    }

    #[test]
    fn restore_dc_pops_at_most_what_is_saved() {
        let (mut stack, mut dc) = (vec![1, 2, 3], 9);
        restore_dc(&mut stack, &mut dc, -2);
        assert_eq!((stack.as_slice(), dc), (&[1][..], 2));
        restore_dc(&mut stack, &mut dc, i32::MIN);
        assert_eq!((stack.len(), dc), (0, 1));
        restore_dc(&mut stack, &mut dc, -1);
        assert_eq!(dc, 1, "an empty stack leaves the state as it is");
        let (mut stack, mut dc) = (vec![7, 8], 0);
        restore_dc(&mut stack, &mut dc, 5);
        assert_eq!((stack.as_slice(), dc), (&[7][..], 8), "a positive count pops one");
    }

    #[test]
    fn truncated_record_does_not_panic() {
        let mut data = header(10, 10);
        let mut bad = polygon16(&[(0, 0), (5, 5), (1, 9)]);
        bad.truncate(bad.len() - 6);
        // Keep the declared size valid so the record is read, but with its
        // point count claiming more points than exist.
        let count_at = 8 + 16;
        bad[count_at..count_at + 4].copy_from_slice(&1000u32.to_le_bytes());
        data.extend(bad);
        let _ = emf_to_svg(&data);
        // Garbage with a valid signature must also be survivable.
        let mut garbage = header(10, 10);
        garbage.extend([0xff; 64]);
        let _ = emf_to_svg(&garbage);
    }

    #[test]
    fn save_and_restore_dc_restores_the_pen() {
        let mut data = header(20, 20);
        data.extend(rec(33, &[])); // SAVEDC
        data.extend(rec(38, &[2, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0x00])); // pen 2 width 3 blue-ish
        data.extend(rec(37, &2u32.to_le_bytes()));
        data.extend(rec(34, &(-1i32).to_le_bytes())); // RESTOREDC
        data.extend(polygon16(&[(0, 0), (5, 0), (5, 5)]));
        let svg = emf_to_svg(&data).unwrap();
        assert!(svg.svg.contains("stroke=\"#000000\""), "{}", svg.svg);
    }

    #[test]
    fn a_drawing_that_starts_at_pixel_one_keeps_the_margin_to_the_frame_corner() {
        let mut data = header(16, 10);
        data[8..12].copy_from_slice(&1i32.to_le_bytes()); // bounds left
        data[12..16].copy_from_slice(&1i32.to_le_bytes()); // bounds top
        data.extend(polygon16(&[(1, 1), (6, 1), (6, 6)]));
        data.extend(rec(14, &[0u8; 12]));
        let svg = emf_to_svg(&data).unwrap().svg;
        assert!(svg.contains("viewBox=\"0.00 0.00 "), "the view box starts at the frame corner: {svg}");
    }
}
