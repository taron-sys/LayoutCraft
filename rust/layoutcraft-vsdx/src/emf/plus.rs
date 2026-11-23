//! EMF+ (GDI+) records carried in EMF comment records.
//!
//! Format: the record layouts of the public Open Specification [MS-EMFPLUS] (Enhanced Metafile Format Plus Extensions).
//!
//! GDI+ based tools (Office, browsers, Visio itself) write "dual" metafiles: the EMF+
//! records draw the real picture with anti-aliased, alpha-blended brushes, and the plain
//! GDI records are a crude fallback made of raster-operation tricks (pattern XOR/AND
//! passes) that cannot show the same thing. Windows plays the EMF+ records when they are
//! present, so this module does too.
//!
//! Supported: solid (and approximated hatch/gradient) brushes, pens, paths, rectangles,
//! ellipses, polygons, world and page transforms, save/restore, rectangular and path
//! clips, bitmap images. The converter only switches to EMF+ when every drawing record
//! in the file is one of these; otherwise the GDI path is used unchanged.
use std::collections::HashMap;
use std::fmt::Write as _;

use super::{color_hex_alpha, f32_at, i16_at, u32_at, Converter, Xf};

#[derive(Debug, Clone)]
pub(super) enum Obj {
    Brush { color: String, alpha: f64, grad: Option<Grad> },
    Pen { width: f64, color: String, alpha: f64 },
    Path { points: Vec<(f64, f64)>, types: Vec<u8> },
    Image { uri: String, width: f64, height: f64 },
    Region(Region),
    Other,
}

/// A GDI+ gradient brush: the shape of the gradient in brush space, its colour stops and the
/// brush transform (brush space to world space).
#[derive(Debug, Clone)]
pub(super) struct Grad {
    kind: GradKind,
    stops: Vec<(f64, String, f64)>,
    xf: Xf,
}

#[derive(Debug, Clone)]
enum GradKind {
    /// Horizontal in brush space: from x1 to x2 along y.
    Linear { x1: f64, x2: f64, y: f64 },
    /// Path gradient approximated by an ellipse: centre and the farthest boundary extent along x and y.
    Radial { cx: f64, cy: f64, rx: f64, ry: f64 },
}

#[derive(Debug, Clone)]
pub(super) enum Region {
    Rect(f64, f64, f64, f64),
    Infinite,
    Empty,
    Unsupported,
}

pub(super) struct Plus {
    world: Xf,
    page_scale: f64,
    dpi: (f64, f64),
    stack: Vec<(u32, Xf, f64, Option<usize>)>,
    objects: HashMap<u8, Obj>,
    clip: Option<usize>,
    /// True after a GetDC record until the next EMF+ record: the GDI records in
    /// between are real drawing (text, bitmaps) and are played by the GDI path.
    pub gdi_live: bool,
}

const SUPPORTED: &[u16] = &[
    0x4001, 0x4002, 0x4003, 0x4004, 0x4008, 0x4009, 0x400A, 0x400B, 0x400C, 0x400E, 0x400F, 0x4014, 0x4015, 0x401B,
    0x401D, 0x401E, 0x401F, 0x4020, 0x4021, 0x4022, 0x4023, 0x4024, 0x4025, 0x4026, 0x402A, 0x402B, 0x402C,
    0x402D, 0x402E, 0x402F, 0x4030, 0x4031, 0x4032, 0x4033, 0x4034, 0x4035,
];

/// Does the metafile carry EMF+ records, and are all of them ones we can play? Returns the
/// reference device resolution from the EMF+ header.
pub(super) fn prescan(bytes: &[u8]) -> Option<(f64, f64)> {
    let mut offset = 0usize;
    let mut dpi = None;
    let mut drawing = false;
    while let (Some(kind), Some(size)) = (u32_at(bytes, offset), u32_at(bytes, offset + 4)) {
        let size = size as usize;
        if size < 8 || offset + size > bytes.len() {
            break;
        }
        if kind == 70 && bytes.get(offset + 12..offset + 16) == Some(b"EMF+") {
            let mut at = offset + 16;
            let end = offset + size;
            while at + 12 <= end {
                let record = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
                let rsize = u32_at(bytes, at + 4)? as usize;
                if rsize < 12 {
                    break;
                }
                if !SUPPORTED.contains(&record) {
                    return None;
                }
                // A pen with a dash pattern or custom caps (arrowheads) draws something we do
                // not reproduce: use the GDI records for such pictures.
                if record == 0x4008 {
                    let flags = u16::from_le_bytes([bytes[at + 2], bytes[at + 3]]);
                    let object = bytes.get(at + 12..at + rsize)?;
                    match (flags >> 8) & 0x7f {
                        2 if !pen_is_plain(object) => return None,
                        // Hatch and texture brushes are only approximated: play the GDI records for
                        // such pictures instead (solid, linear and path gradients are drawn here).
                        1 if !matches!(u32_at(object, 4)?, 0 | 3 | 4) => return None,
                        _ => {}
                    }
                }
                if record == 0x4001 {
                    dpi = Some((u32_at(bytes, at + 12 + 8)? as f64, u32_at(bytes, at + 12 + 12)? as f64));
                }
                if matches!(record, 0x400A | 0x400B | 0x400C | 0x400E | 0x400F | 0x4014 | 0x4015 | 0x401B) {
                    drawing = true;
                }
                at += rsize;
            }
        }
        if kind == 14 {
            break;
        }
        offset += size;
    }
    if drawing { dpi.filter(|(x, y)| *x > 0.0 && *y > 0.0) } else { None }
}

impl Plus {
    pub(super) fn new(dpi: (f64, f64)) -> Plus {
        Plus {
            world: Xf::IDENTITY,
            page_scale: 1.0,
            dpi,
            stack: Vec::new(),
            objects: HashMap::new(),
            clip: None,
            gdi_live: false,
        }
    }

    /// Page units to device pixels (EMF+ coordinates are in the page's unit).
    fn page_xf(&self) -> Xf {
        let s = self.page_scale;
        Xf { a: s, b: 0.0, c: 0.0, d: s, e: 0.0, f: 0.0 }
    }

    /// World then page: object space to device pixels.
    fn device_xf(&self) -> Xf {
        self.world.then(&self.page_xf())
    }
}

fn argb(raw: u32) -> (String, f64) {
    color_hex_alpha(raw)
}

fn read_points(data: &[u8], at: usize, count: usize, compressed: bool) -> Option<Vec<(f64, f64)>> {
    let mut out = Vec::with_capacity(count.min(1 << 16));
    for i in 0..count.min(1 << 20) {
        out.push(if compressed {
            (i16_at(data, at + i * 4)? as f64, i16_at(data, at + i * 4 + 2)? as f64)
        } else {
            (f32_at(data, at + i * 8)? as f64, f32_at(data, at + i * 8 + 4)? as f64)
        });
    }
    Some(out)
}

fn read_rects(data: &[u8], at: usize, count: usize, compressed: bool) -> Option<Vec<(f64, f64, f64, f64)>> {
    let mut out = Vec::new();
    for i in 0..count.min(1 << 16) {
        out.push(if compressed {
            let o = at + i * 8;
            (i16_at(data, o)? as f64, i16_at(data, o + 2)? as f64, i16_at(data, o + 4)? as f64, i16_at(data, o + 6)? as f64)
        } else {
            let o = at + i * 16;
            (f32_at(data, o)? as f64, f32_at(data, o + 4)? as f64, f32_at(data, o + 8)? as f64, f32_at(data, o + 12)? as f64)
        });
    }
    Some(out)
}

fn read_object(object_type: u8, data: &[u8]) -> Obj {
    match object_type {
        1 => read_brush(data, 0).map_or(Obj::Other, |(color, alpha)| Obj::Brush { color, alpha, grad: read_gradient(data, 0) }),
        2 => read_pen(data).unwrap_or(Obj::Other),
        3 => read_path(data).unwrap_or(Obj::Other),
        4 => Obj::Region(read_region(data)),
        5 => read_image(data).unwrap_or(Obj::Other),
        _ => Obj::Other,
    }
}

/// A brush object at `at`: version, type, data. Non-solid brushes are approximated by
/// their (first) colour.
fn read_brush(data: &[u8], at: usize) -> Option<(String, f64)> {
    let kind = u32_at(data, at + 4)?;
    match kind {
        0 => Some(argb(u32_at(data, at + 8)?)),
        // HatchBrush: style, fore, back.
        1 => Some(argb(u32_at(data, at + 12)?)),
        // LinearGradient: flags, wrap, rect (16), start, end.
        4 => {
            let start = u32_at(data, at + 8 + 8 + 16)?;
            Some(argb(start))
        }
        _ => Some(("#808080".to_string(), 1.0)),
    }
}

fn matrix_at(data: &[u8], at: usize) -> Option<Xf> {
    Some(Xf {
        a: f32_at(data, at)? as f64,
        b: f32_at(data, at + 4)? as f64,
        c: f32_at(data, at + 8)? as f64,
        d: f32_at(data, at + 12)? as f64,
        e: f32_at(data, at + 16)? as f64,
        f: f32_at(data, at + 20)? as f64,
    })
}

/// A linear (type 4) or path (type 3) gradient brush object at `at`, or `None` for other brushes.
fn read_gradient(data: &[u8], at: usize) -> Option<Grad> {
    let kind = u32_at(data, at + 4)?;
    let flags = u32_at(data, at + 8)?;
    match kind {
        4 => {
            let (x, y, w, h) = (
                f32_at(data, at + 16)? as f64,
                f32_at(data, at + 20)? as f64,
                f32_at(data, at + 24)? as f64,
                f32_at(data, at + 28)? as f64,
            );
            let (c0, a0) = argb(u32_at(data, at + 32)?);
            let (c1, a1) = argb(u32_at(data, at + 36)?);
            let mut p = at + 48;
            let xf = if flags & 0x2 != 0 {
                let m = matrix_at(data, p)?;
                p += 24;
                m
            } else {
                Xf::IDENTITY
            };
            let mix = |t: f64| -> (String, f64) {
                let ch = |s: &str, i: usize| u8::from_str_radix(&s[1 + 2 * i..3 + 2 * i], 16).unwrap_or(0) as f64;
                let c = |i: usize| (ch(&c0, i) + (ch(&c1, i) - ch(&c0, i)) * t).round() as u8;
                (format!("#{:02x}{:02x}{:02x}", c(0), c(1), c(2)), a0 + (a1 - a0) * t)
            };
            let mut stops: Vec<(f64, String, f64)> = Vec::new();
            if flags & 0x4 != 0 {
                let n = (u32_at(data, p)? as usize).min(256);
                for i in 0..n {
                    let pos = f32_at(data, p + 4 + 4 * i)? as f64;
                    let (c, a) = argb(u32_at(data, p + 4 + 4 * n + 4 * i)?);
                    stops.push((pos, c, a));
                }
            } else if flags & 0x8 != 0 {
                let n = (u32_at(data, p)? as usize).min(256);
                for i in 0..n {
                    let pos = f32_at(data, p + 4 + 4 * i)? as f64;
                    let f = f32_at(data, p + 4 + 4 * n + 4 * i)? as f64;
                    let (c, a) = mix(f.clamp(0.0, 1.0));
                    stops.push((pos, c, a));
                }
            }
            if stops.len() < 2 {
                stops = vec![(0.0, c0, a0), (1.0, c1, a1)];
            }
            Some(Grad { kind: GradKind::Linear { x1: x, x2: x + w, y: y + h / 2.0 }, stops, xf })
        }
        3 => {
            let (cc, ca) = argb(u32_at(data, at + 16)?);
            let (cx, cy) = (f32_at(data, at + 20)? as f64, f32_at(data, at + 24)? as f64);
            let n = (u32_at(data, at + 28)? as usize).min(1024);
            let (sc, sa) = argb(u32_at(data, at + 32)?);
            let mut p = at + 32 + 4 * n;
            let (mut rx, mut ry) = (0.0f64, 0.0f64);
            let mut consider = |x: f64, y: f64| {
                rx = rx.max((x - cx).abs());
                ry = ry.max((y - cy).abs());
            };
            if flags & 0x1 != 0 {
                let size = u32_at(data, p)? as usize;
                let path = data.get(p + 4..p + 4 + size)?;
                if let Some(Obj::Path { points, .. }) = read_path(path) {
                    for (x, y) in points {
                        consider(x, y);
                    }
                }
                p += 4 + size;
            } else {
                let count = (u32_at(data, p)? as usize).min(1 << 16);
                for i in 0..count {
                    consider(f32_at(data, p + 4 + 8 * i)? as f64, f32_at(data, p + 8 + 8 * i)? as f64);
                }
                p += 4 + 8 * count;
            }
            let xf = if flags & 0x2 != 0 {
                let m = matrix_at(data, p).unwrap_or(Xf::IDENTITY);
                p += 24;
                m
            } else {
                Xf::IDENTITY
            };
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            // Positions run from the boundary (0) to the centre (1); an SVG radial gradient
            // starts at the centre.
            let mut stops: Vec<(f64, String, f64)> = Vec::new();
            if flags & 0x4 != 0 {
                let n = (u32_at(data, p).unwrap_or(0) as usize).min(256);
                for i in 0..n {
                    let (Some(pos), Some(raw)) = (f32_at(data, p + 4 + 4 * i), u32_at(data, p + 4 + 4 * n + 4 * i)) else { break };
                    let (c, a) = argb(raw);
                    stops.push((1.0 - pos as f64, c, a));
                }
            } else if flags & 0x8 != 0 {
                let n = (u32_at(data, p).unwrap_or(0) as usize).min(256);
                let ch = |s: &str, i: usize| u8::from_str_radix(&s[1 + 2 * i..3 + 2 * i], 16).unwrap_or(0) as f64;
                for i in 0..n {
                    let (Some(pos), Some(f)) = (f32_at(data, p + 4 + 4 * i), f32_at(data, p + 4 + 4 * n + 4 * i)) else { break };
                    let t = (f as f64).clamp(0.0, 1.0);
                    // t = 0 is the boundary colour, 1 the centre colour.
                    let c = |i: usize| (ch(&sc, i) + (ch(&cc, i) - ch(&sc, i)) * t).round() as u8;
                    stops.push((1.0 - pos as f64, format!("#{:02x}{:02x}{:02x}", c(0), c(1), c(2)), sa + (ca - sa) * t));
                }
            }
            if stops.len() >= 2 {
                stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            } else {
                stops = vec![(0.0, cc, ca), (1.0, sc, sa)];
            }
            Some(Grad { kind: GradKind::Radial { cx, cy, rx, ry }, stops, xf })
        }
        _ => None,
    }
}

/// Solid, undashed pen with at most the standard caps and joins.
fn pen_is_plain(data: &[u8]) -> bool {
    let Some(flags) = u32_at(data, 8) else { return false };
    if flags & (0x100 | 0x400 | 0x800 | 0x1000) != 0 {
        return false;
    }
    if flags & 0x20 != 0 {
        // LineStyle follows the transform, caps, join and miter fields that come first.
        let mut at = 20usize;
        for (bit, size) in [(0x1u32, 24usize), (0x2, 4), (0x4, 4), (0x8, 4), (0x10, 4)] {
            if flags & bit != 0 {
                at += size;
            }
        }
        return u32_at(data, at) == Some(0);
    }
    true
}

fn read_pen(data: &[u8]) -> Option<Obj> {
    // Version, type(0), then PenData: flags, unit, width.
    let flags = u32_at(data, 8)?;
    let width = f32_at(data, 16)? as f64;
    let mut at = 20usize;
    for (bit, size) in [(0x1u32, 24usize), (0x2, 4), (0x4, 4), (0x8, 4), (0x10, 4), (0x20, 4), (0x40, 4), (0x80, 4)] {
        if flags & bit != 0 {
            at += size;
        }
    }
    if flags & 0x100 != 0 {
        at += 4 + u32_at(data, at)? as usize * 4;
    }
    if flags & 0x200 != 0 {
        at += 4;
    }
    if flags & 0x400 != 0 {
        at += 4 + u32_at(data, at)? as usize * 4;
    }
    if flags & 0x800 != 0 {
        at += 4 + u32_at(data, at)? as usize;
    }
    if flags & 0x1000 != 0 {
        at += 4 + u32_at(data, at)? as usize;
    }
    let (color, alpha) = read_brush(data, at)?;
    Some(Obj::Pen { width, color, alpha })
}

fn read_path(data: &[u8]) -> Option<Obj> {
    let count = u32_at(data, 4)? as usize;
    let flags = u32_at(data, 8)?;
    if flags & 0x800 != 0 || count > 1 << 20 {
        return None; // relative coordinates: not supported
    }
    let compressed = flags & 0x4000 != 0;
    let points = read_points(data, 12, count, compressed)?;
    let types_at = 12 + count * if compressed { 4 } else { 8 };
    let types = data.get(types_at..types_at + count)?.to_vec();
    Some(Obj::Path { points, types })
}

fn read_region(data: &[u8]) -> Region {
    // Version, node count, then the root node (type, data).
    let node = match u32_at(data, 8) {
        Some(n) => n,
        None => return Region::Unsupported,
    };
    match node {
        0x1000_0000 => match (f32_at(data, 12), f32_at(data, 16), f32_at(data, 20), f32_at(data, 24)) {
            (Some(x), Some(y), Some(w), Some(h)) => Region::Rect(x as f64, y as f64, w as f64, h as f64),
            _ => Region::Unsupported,
        },
        0x1000_0002 => Region::Empty,
        0x1000_0003 => Region::Infinite,
        _ => Region::Unsupported,
    }
}

fn read_image(data: &[u8]) -> Option<Obj> {
    if u32_at(data, 4)? != 1 {
        return None; // metafile images
    }
    let width = u32_at(data, 8)?;
    let height = u32_at(data, 12)?;
    let stride = u32_at(data, 16)? as usize;
    let format = u32_at(data, 20)?;
    let kind = u32_at(data, 24)?;
    let pixels = data.get(28..)?;
    if kind == 1 {
        let mime = if pixels.starts_with(&[0x89, b'P', b'N', b'G']) {
            "image/png"
        } else if pixels.starts_with(&[0xff, 0xd8]) {
            "image/jpeg"
        } else if pixels.starts_with(b"GIF8") {
            "image/gif"
        } else if pixels.starts_with(b"BM") {
            return None;
        } else {
            return None;
        };
        // Compressed bitmaps often store 0 for width and height; the data knows its size.
        let (width, height) = if width == 0 || height == 0 {
            image::load_from_memory(pixels)
                .ok()
                .map(|decoded| {
                    use image::GenericImageView;
                    decoded.dimensions()
                })
                .unwrap_or((width, height))
        } else {
            (width, height)
        };
        return Some(Obj::Image {
            uri: format!("data:{mime};base64,{}", super::base64(pixels)),
            width: width as f64,
            height: height as f64,
        });
    }
    // Raw 32 bits per pixel (ARGB / PARGB): bytes are B, G, R, A, top-down.
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 || !matches!(format, 0x0026_200A | 0x000E_200B | 0x0002_2009) {
        return None;
    }
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let row = pixels.get(y * stride..y * stride + width as usize * 4)?;
        for x in 0..width as usize {
            let (b, g, r, a) = (row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]);
            let o = (y * width as usize + x) * 4;
            rgba[o..o + 4].copy_from_slice(&[r, g, b, a]);
        }
    }
    let uri = super::png_uri_from_rgba(width, height, &rgba)?;
    Some(Obj::Image { uri, width: width as f64, height: height as f64 })
}

/// Play one EMF comment record's EMF+ records.
pub(super) fn handle_comment(conv: &mut Converter, st: &mut Plus, rec: &[u8]) {
    let mut at = 16usize;
    let mut saw_plus = false;
    while at + 12 <= rec.len() {
        let kind = u16::from_le_bytes([rec[at], rec[at + 1]]);
        let flags = u16::from_le_bytes([rec[at + 2], rec[at + 3]]);
        let size = match u32_at(rec, at + 4) {
            Some(s) if s >= 12 => s as usize,
            _ => break,
        };
        let data_size = u32_at(rec, at + 8).unwrap_or(0) as usize;
        let end = (at + size).min(rec.len());
        let data = rec.get(at + 12..(at + 12 + data_size).min(end)).unwrap_or(&[]);
        saw_plus = true;
        st.gdi_live = false;
        let _ = play(conv, st, kind, flags, data);
        if kind == 0x4004 {
            // GetDC: GDI records that follow are drawn in the current EMF+ space.
            conv.dc = super::Dc::new();
            conv.dc.world = st.device_xf();
            conv.stack.clear();
            st.gdi_live = true;
        }
        at += size;
    }
    let _ = saw_plus;
}

fn clip_attr(st: &Plus) -> String {
    st.clip.map(|id| format!(" clip-path=\"url(#p{id})\"")).unwrap_or_default()
}

fn path_d(xf: &Xf, points: &[(f64, f64)], types: &[u8]) -> String {
    let mut d = String::new();
    let mut i = 0;
    let p = |i: usize| xf.apply(points[i].0, points[i].1);
    while i < points.len() {
        match types.get(i).copied().unwrap_or(1) & 0x0f {
            0 => {
                let (x, y) = p(i);
                let _ = write!(d, "M{x:.2} {y:.2}");
                i += 1;
            }
            3 if i + 2 < points.len() => {
                let (a, b, c) = (p(i), p(i + 1), p(i + 2));
                let _ = write!(d, "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}", a.0, a.1, b.0, b.1, c.0, c.1);
                if types.get(i + 2).copied().unwrap_or(0) & 0x80 != 0 {
                    d.push('Z');
                }
                i += 3;
                continue;
            }
            _ => {
                let (x, y) = p(i);
                let _ = write!(d, "L{x:.2} {y:.2}");
                i += 1;
            }
        }
        if types.get(i - 1).copied().unwrap_or(0) & 0x80 != 0 {
            d.push('Z');
        }
    }
    d
}

fn rect_d(xf: &Xf, x: f64, y: f64, w: f64, h: f64) -> String {
    let c = [xf.apply(x, y), xf.apply(x + w, y), xf.apply(x + w, y + h), xf.apply(x, y + h)];
    format!(
        "M{:.2} {:.2}L{:.2} {:.2}L{:.2} {:.2}L{:.2} {:.2}Z",
        c[0].0, c[0].1, c[1].0, c[1].1, c[2].0, c[2].1, c[3].0, c[3].1
    )
}

fn ellipse_d(xf: &Xf, x: f64, y: f64, w: f64, h: f64) -> String {
    const K: f64 = 0.552_284_749_830_793_4;
    let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
    let p = |px: f64, py: f64| xf.apply(px, py);
    let (n, e, s, wv) = (p(cx, cy - ry), p(cx + rx, cy), p(cx, cy + ry), p(cx - rx, cy));
    let c = |x1: f64, y1: f64, x2: f64, y2: f64, t: (f64, f64)| {
        let (a, b) = (p(x1, y1), p(x2, y2));
        format!("C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}", a.0, a.1, b.0, b.1, t.0, t.1)
    };
    format!(
        "M{:.2} {:.2}{}{}{}{}Z",
        n.0,
        n.1,
        c(cx + rx * K, cy - ry, cx + rx, cy - ry * K, e),
        c(cx + rx, cy + ry * K, cx + rx * K, cy + ry, s),
        c(cx - rx * K, cy + ry, cx - rx, cy + ry * K, wv),
        c(cx - rx, cy - ry * K, cx - rx * K, cy - ry, n),
    )
}

fn fill_with(conv: &mut Converter, st: &Plus, d: &str, color: &str, alpha: f64) {
    let opacity = if alpha < 0.999 { format!(" fill-opacity=\"{alpha:.3}\"") } else { String::new() };
    let _ = write!(conv.body, "<path d=\"{d}\" fill=\"{color}\"{opacity} stroke=\"none\"{}/>", clip_attr(st));
}

fn stroke_with(conv: &mut Converter, st: &Plus, d: &str, pen: &Obj) {
    let Obj::Pen { width, color, alpha } = pen else { return };
    let xf = st.device_xf();
    let scale = (xf.a * xf.d - xf.b * xf.c).abs().sqrt().max(1e-9);
    let w = (width * scale).max(conv.min_stroke * 0.5);
    let opacity = if *alpha < 0.999 { format!(" stroke-opacity=\"{alpha:.3}\"") } else { String::new() };
    let _ = write!(
        conv.body,
        "<path d=\"{d}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{w:.3}\"{opacity} stroke-linejoin=\"round\" stroke-linecap=\"round\"{}/>",
        clip_attr(st)
    );
}

/// The brush a fill record names: an inline ARGB colour (flag 0x8000) or an object id. A gradient
/// brush is written to the defs and returned as a `url(#..)` paint.
fn brush_of(conv: &mut Converter, st: &Plus, flags: u16, id: u32) -> Option<(String, f64)> {
    if flags & 0x8000 != 0 {
        return Some(argb(id));
    }
    match st.objects.get(&((id & 0xff) as u8))? {
        Obj::Brush { grad: Some(grad), .. } => {
            conv.clips += 1;
            let gid = format!("eg{}", conv.clips + 2000);
            let mut m = grad.xf.then(&st.device_xf());
            if let GradKind::Radial { cx, cy, rx, ry } = grad.kind {
                // The unit circle scaled to the boundary extents and moved to the centre.
                m = Xf { a: rx, b: 0.0, c: 0.0, d: ry, e: cx, f: cy }.then(&m);
            }
            let transform = format!("matrix({:.6} {:.6} {:.6} {:.6} {:.4} {:.4})", m.a, m.b, m.c, m.d, m.e, m.f);
            let mut stops = String::new();
            for (offset, color, alpha) in &grad.stops {
                let _ = write!(stops, "<stop offset=\"{offset:.4}\" stop-color=\"{color}\" stop-opacity=\"{alpha:.3}\"/>");
            }
            match grad.kind {
                GradKind::Linear { x1, x2, y } => {
                    let _ = write!(
                        conv.defs,
                        "<linearGradient id=\"{gid}\" gradientUnits=\"userSpaceOnUse\" x1=\"{x1:.4}\" y1=\"{y:.4}\" x2=\"{x2:.4}\" y2=\"{y:.4}\" gradientTransform=\"{transform}\">{stops}</linearGradient>"
                    );
                }
                GradKind::Radial { .. } => {
                    let _ = write!(
                        conv.defs,
                        "<radialGradient id=\"{gid}\" gradientUnits=\"userSpaceOnUse\" cx=\"0\" cy=\"0\" r=\"1\" gradientTransform=\"{transform}\">{stops}</radialGradient>"
                    );
                }
            }
            Some((format!("url(#{gid})"), 1.0))
        }
        Obj::Brush { color, alpha, .. } => Some((color.clone(), *alpha)),
        _ => None,
    }
}

fn set_clip_path(conv: &mut Converter, st: &mut Plus, d: &str, combine: u16) {
    // Combine modes: 0 replace, 1 intersect; other modes are treated as replace.
    let id = conv.clips + 1000;
    conv.clips += 1;
    let _ = write!(conv.defs, "<clipPath id=\"p{id}\"><path d=\"{d}\"/></clipPath>");
    if combine == 1 {
        if let Some(prev) = st.clip {
            // Intersection of two clips: nest through a group clip.
            let nid = conv.clips + 1000;
            conv.clips += 1;
            let _ = write!(
                conv.defs,
                "<clipPath id=\"p{nid}\" clip-path=\"url(#p{prev})\"><path d=\"{d}\"/></clipPath>"
            );
            st.clip = Some(nid);
            return;
        }
    }
    st.clip = Some(id);
}

fn play(conv: &mut Converter, st: &mut Plus, kind: u16, flags: u16, data: &[u8]) -> Option<()> {
    match kind {
        // --- objects ---
        0x4008 => {
            let object_type = ((flags >> 8) & 0x7f) as u8;
            let id = (flags & 0xff) as u8;
            // Continued objects (large images) are not reassembled.
            if flags & 0x8000 != 0 {
                st.objects.insert(id, Obj::Other);
                return Some(());
            }
            let obj = read_object(object_type, data);
            st.objects.insert(id, obj);
        }
        // --- state ---
        0x4025 => {
            st.stack.push((u32_at(data, 0)?, st.world, st.page_scale, st.clip));
        }
        0x4026 => {
            let index = u32_at(data, 0)?;
            while let Some((i, world, scale, clip)) = st.stack.pop() {
                if i == index {
                    st.world = world;
                    st.page_scale = scale;
                    st.clip = clip;
                    break;
                }
            }
        }
        0x402A => {
            st.world = Xf {
                a: f32_at(data, 0)? as f64,
                b: f32_at(data, 4)? as f64,
                c: f32_at(data, 8)? as f64,
                d: f32_at(data, 12)? as f64,
                e: f32_at(data, 16)? as f64,
                f: f32_at(data, 20)? as f64,
            };
        }
        0x402B => st.world = Xf::IDENTITY,
        0x402C | 0x402D | 0x402E | 0x402F => {
            let m = match kind {
                0x402C => Xf {
                    a: f32_at(data, 0)? as f64,
                    b: f32_at(data, 4)? as f64,
                    c: f32_at(data, 8)? as f64,
                    d: f32_at(data, 12)? as f64,
                    e: f32_at(data, 16)? as f64,
                    f: f32_at(data, 20)? as f64,
                },
                0x402D => Xf { e: f32_at(data, 0)? as f64, f: f32_at(data, 4)? as f64, ..Xf::IDENTITY },
                0x402E => Xf { a: f32_at(data, 0)? as f64, d: f32_at(data, 4)? as f64, ..Xf::IDENTITY },
                _ => {
                    let (s, c) = (f32_at(data, 0)? as f64).to_radians().sin_cos();
                    Xf { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
                }
            };
            // Order bit set: the new transform is applied after the current one.
            st.world = if flags & 0x2000 != 0 { st.world.then(&m) } else { m.then(&st.world) };
        }
        0x4030 => {
            let unit = flags & 0xff;
            let scale = f32_at(data, 0)? as f64;
            let per_unit = match unit {
                3 => st.dpi.0 / 72.0,
                4 => st.dpi.0,
                5 => st.dpi.0 / 300.0,
                6 => st.dpi.0 / 25.4,
                _ => 1.0,
            };
            st.page_scale = if scale > 0.0 { scale * per_unit } else { per_unit };
        }
        // --- clip ---
        0x4031 => st.clip = None,
        0x4032 => {
            let combine = (flags >> 8) & 0xf;
            let (x, y, w, h) = (f32_at(data, 0)? as f64, f32_at(data, 4)? as f64, f32_at(data, 8)? as f64, f32_at(data, 12)? as f64);
            let d = rect_d(&st.device_xf(), x, y, w, h);
            set_clip_path(conv, st, &d, combine);
        }
        0x4033 => {
            let combine = (flags >> 8) & 0xf;
            if let Some(Obj::Path { points, types }) = st.objects.get(&((flags & 0xff) as u8)).cloned() {
                let d = path_d(&st.device_xf(), &points, &types);
                set_clip_path(conv, st, &d, combine);
            }
        }
        0x4034 => {
            let combine = (flags >> 8) & 0xf;
            match st.objects.get(&((flags & 0xff) as u8)).cloned() {
                Some(Obj::Region(Region::Rect(x, y, w, h))) => {
                    let d = rect_d(&st.device_xf(), x, y, w, h);
                    set_clip_path(conv, st, &d, combine);
                }
                Some(Obj::Region(Region::Infinite)) => st.clip = None,
                _ => {} // empty and complex regions: leave the clip as it is
            }
        }
        // --- drawing ---
        0x4009 => {}
        0x400A => {
            let brush = brush_of(conv, st, flags, u32_at(data, 0)?)?;
            let count = u32_at(data, 4)? as usize;
            let rects = read_rects(data, 8, count, flags & 0x4000 != 0)?;
            let xf = st.device_xf();
            for (x, y, w, h) in rects {
                let d = rect_d(&xf, x, y, w, h);
                fill_with(conv, st, &d, &brush.0, brush.1);
            }
        }
        0x400B => {
            let pen = st.objects.get(&((flags & 0xff) as u8))?.clone();
            let count = u32_at(data, 0)? as usize;
            let rects = read_rects(data, 4, count, flags & 0x4000 != 0)?;
            let xf = st.device_xf();
            for (x, y, w, h) in rects {
                let d = rect_d(&xf, x, y, w, h);
                stroke_with(conv, st, &d, &pen);
            }
        }
        0x400C => {
            let brush = brush_of(conv, st, flags, u32_at(data, 0)?)?;
            let count = u32_at(data, 4)? as usize;
            let points = read_points(data, 8, count, flags & 0x4000 != 0)?;
            let xf = st.device_xf();
            let mut d = String::new();
            for (i, (x, y)) in points.iter().enumerate() {
                let (px, py) = xf.apply(*x, *y);
                let _ = write!(d, "{}{px:.2} {py:.2}", if i == 0 { "M" } else { "L" });
            }
            d.push('Z');
            fill_with(conv, st, &d, &brush.0, brush.1);
        }
        0x400E => {
            let brush = brush_of(conv, st, flags, u32_at(data, 0)?)?;
            let rect = read_rects(data, 4, 1, flags & 0x4000 != 0)?.first().copied()?;
            let d = ellipse_d(&st.device_xf(), rect.0, rect.1, rect.2, rect.3);
            fill_with(conv, st, &d, &brush.0, brush.1);
        }
        0x400F => {
            let pen = st.objects.get(&((flags & 0xff) as u8))?.clone();
            let rect = read_rects(data, 0, 1, flags & 0x4000 != 0)?.first().copied()?;
            let d = ellipse_d(&st.device_xf(), rect.0, rect.1, rect.2, rect.3);
            stroke_with(conv, st, &d, &pen);
        }
        0x4014 => {
            let brush = brush_of(conv, st, flags & 0x7fff | (flags & 0x8000), u32_at(data, 0)?)?;
            if let Some(Obj::Path { points, types }) = st.objects.get(&((flags & 0xff) as u8)).cloned() {
                let d = path_d(&st.device_xf(), &points, &types);
                fill_with(conv, st, &d, &brush.0, brush.1);
            }
        }
        0x4015 => {
            let pen = st.objects.get(&(u32_at(data, 0)? as u8))?.clone();
            if let Some(Obj::Path { points, types }) = st.objects.get(&((flags & 0xff) as u8)).cloned() {
                let d = path_d(&st.device_xf(), &points, &types);
                stroke_with(conv, st, &d, &pen);
            }
        }
        0x401B => {
            let Some(Obj::Image { uri, width, height }) = st.objects.get(&((flags & 0xff) as u8)).cloned() else {
                return Some(());
            };
            let compressed = flags & 0x4000 != 0;
            // Attributes id, source unit, source rect, count, then three destination points.
            let (sx, sy, sw, sh) = (f32_at(data, 8)? as f64, f32_at(data, 12)? as f64, f32_at(data, 16)? as f64, f32_at(data, 20)? as f64);
            let count = u32_at(data, 24)? as usize;
            if count < 3 {
                return Some(());
            }
            let pts = read_points(data, 28, 3, compressed)?;
            let xf = st.device_xf();
            let (p0, p1, p2) = (xf.apply(pts[0].0, pts[0].1), xf.apply(pts[1].0, pts[1].1), xf.apply(pts[2].0, pts[2].1));
            // The source rectangle is in pixels unless the unit says otherwise (treated as pixels).
            let (sw, sh) = (if sw > 0.0 { sw } else { width }, if sh > 0.0 { sh } else { height });
            let a = (p1.0 - p0.0) / sw;
            let b = (p1.1 - p0.1) / sw;
            let c = (p2.0 - p0.0) / sh;
            let d = (p2.1 - p0.1) / sh;
            let e = p0.0 - a * sx - c * sy;
            let f = p0.1 - b * sx - d * sy;
            let id = conv.clips + 1000;
            conv.clips += 1;
            let _ = write!(
                conv.defs,
                "<clipPath id=\"p{id}\"><rect x=\"{sx:.2}\" y=\"{sy:.2}\" width=\"{sw:.2}\" height=\"{sh:.2}\"/></clipPath>"
            );
            let _ = write!(
                conv.body,
                "<g{}><g transform=\"matrix({a:.6} {b:.6} {c:.6} {d:.6} {e:.3} {f:.3})\" clip-path=\"url(#p{id})\"><image x=\"0\" y=\"0\" width=\"{width:.2}\" height=\"{height:.2}\" preserveAspectRatio=\"none\" href=\"{uri}\"/></g></g>",
                clip_attr(st)
            );
        }
        _ => {}
    }
    Some(())
}
