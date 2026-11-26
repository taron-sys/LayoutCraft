//! Rotation-aware group editing: resize a group, move or resize one member.
//!
//! All sizes and positions are Visio's page units (inches, y up). A shape's
//! frame is `PinX/PinY/Width/Height/LocPinX/LocPinY/Angle`; a point `p` of the
//! shape's own box (origin bottom-left) sits in its parent at
//! `Pin + R(angle) * (p - LocPin)`. Children of a group are positioned in the
//! group's own (rotated) box, so every operation here works in the *local*
//! frame of the group being edited. That is what makes a rotated group
//! editable: the editor hands over local-axis sizes and world deltas and this
//! module turns them into literal cell values.
//!
//! The functions return `ShapeCellPatch`es for `writer::patch_page*`. Writing a
//! `V` drops the cell's formula (see `patch_cell_in_range`), so the values are
//! authoritative afterwards.
//!
//! Not supported (returned as errors, never written wrongly): flipped groups or
//! members. A member turned by any angle in a non-uniformly resized group keeps its
//! angle and takes the factors Visio gives it (`own_scale`, measured on Visio).
use std::collections::BTreeMap;

use crate::router::{route_between_shapes, route_right_angle, Rect, Site};
use layoutcraft_model::LineJump;
use crate::writer::{GeometryRow, ShapeCellPatch};
use crate::{Connect, Error, MasterCatalog, Package, ResolvedShape, Result};

pub mod nurbs;
pub mod shapeops;
pub mod vertices;

const EPS: f64 = 1e-9;

/// A shape frame in its parent's coordinates (inches).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub pin: (f64, f64),
    pub size: (f64, f64),
    pub loc: (f64, f64),
    pub angle: f64,
    /// `FlipX` / `FlipY`: the shape (and, for a group, its members) is mirrored about its `LocPin` before it is turned.
    /// Measured on Visio (probe-flip-group.ps1): a member at local (x, y) of a group flipped in x with an off-centre
    /// `LocPinX` lands at `pin + R(angle) * diag(-1, 1) * (p - loc)`: the mirror axis is the pin, not the box centre.
    pub flip: (bool, bool),
}

impl Frame {
    /// A point of the shape's own box, expressed in the parent.
    pub fn to_parent(&self, p: (f64, f64)) -> (f64, f64) {
        let (s, c) = self.angle.sin_cos();
        let mut d = (p.0 - self.loc.0, p.1 - self.loc.1);
        if self.flip.0 { d.0 = -d.0; }
        if self.flip.1 { d.1 = -d.1; }
        (self.pin.0 + c * d.0 - s * d.1, self.pin.1 + s * d.0 + c * d.1)
    }

    /// A parent point expressed in the shape's own box.
    pub fn from_parent(&self, q: (f64, f64)) -> (f64, f64) {
        let (s, c) = self.angle.sin_cos();
        let d = (q.0 - self.pin.0, q.1 - self.pin.1);
        let mut e = (c * d.0 + s * d.1, -s * d.0 + c * d.1);
        if self.flip.0 { e.0 = -e.0; }
        if self.flip.1 { e.1 = -e.1; }
        (e.0 + self.loc.0, e.1 + self.loc.1)
    }

    /// The linear part of `to_parent` as a matrix `[[a, b], [c, d]]` (the turn after the mirror).
    pub fn linear(&self) -> [[f64; 2]; 2] {
        let (s, c) = self.angle.sin_cos();
        let (fx, fy) = (if self.flip.0 { -1.0 } else { 1.0 }, if self.flip.1 { -1.0 } else { 1.0 });
        [[c * fx, -s * fy], [s * fx, c * fy]]
    }

    /// The four box corners in the parent (bottom-left, bottom-right, top-right, top-left).
    pub fn corners(&self) -> [(f64, f64); 4] {
        let (w, h) = self.size;
        [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|p| self.to_parent(p))
    }
}

/// Resize `frame` to `new_size` keeping the point at `anchor` (a fraction of the
/// box, bottom-left origin) fixed in the parent. `LocPin` keeps its fraction.
pub fn resize_frame(frame: &Frame, new_size: (f64, f64), anchor: (f64, f64)) -> Frame {
    let sx = if frame.size.0.abs() > EPS { new_size.0 / frame.size.0 } else { 1.0 };
    let sy = if frame.size.1.abs() > EPS { new_size.1 / frame.size.1 } else { 1.0 };
    let loc = (frame.loc.0 * sx, frame.loc.1 * sy);
    let fixed = frame.to_parent((anchor.0 * frame.size.0, anchor.1 * frame.size.1));
    let mut out = Frame { pin: frame.pin, size: new_size, loc, angle: frame.angle, flip: frame.flip };
    let moved = out.to_parent((anchor.0 * new_size.0, anchor.1 * new_size.1));
    out.pin = (frame.pin.0 + fixed.0 - moved.0, frame.pin.1 + fixed.1 - moved.1);
    out
}

fn fmt(value: f64) -> String {
    let rounded = (value * 1e12).round() / 1e12;
    format!("{rounded}")
}

/// The cached end points of a one-dimensional shape or group (BeginX / BeginY / EndX / EndY) after it was resized from `old` to `new`:
/// the points scale with the shape's own box like its children do (a flat group such as an IBM rack console switch is a line whose
/// length is its width, so its end points move apart / together with the width). Shapes without end points are left alone.
fn scale_end_points(member: &ResolvedShape, old: &Frame, new: &Frame, scale: (f64, f64), patch: &mut ShapeCellPatch) {
    for (xn, yn) in [("BeginX", "BeginY"), ("EndX", "EndY")] {
        let (Some(x), Some(y)) = (num(member, xn), num(member, yn)) else { continue };
        let local = old.from_parent((x, y));
        let moved = new.to_parent((local.0 * scale.0, local.1 * scale.1));
        *patch = std::mem::replace(patch, ShapeCellPatch::new(member.id.clone())).set_cached(xn, fmt(moved.0)).set_cached(yn, fmt(moved.1));
    }
}

fn frame_patch(id: &str, frame: &Frame) -> ShapeCellPatch {
    ShapeCellPatch::new(id)
        .set("PinX", fmt(frame.pin.0))
        .set("PinY", fmt(frame.pin.1))
        .set("Width", fmt(frame.size.0))
        .set("Height", fmt(frame.size.1))
        .set("LocPinX", fmt(frame.loc.0))
        .set("LocPinY", fmt(frame.loc.1))
}

fn num(node: &ResolvedShape, name: &str) -> Option<f64> {
    node.cells
        .get(name)
        .and_then(|cell| cell.value.as_deref())
        .and_then(|value| value.parse::<f64>().ok())
}

/// A direction in shape `chain[last]`'s own axes as a page direction: every level's mirror and turn applied from the inside out.
fn world_vector(chain: &[&ResolvedShape], v: (f64, f64)) -> (f64, f64) {
    let mut v = v;
    for node in chain.iter().rev() {
        if let Ok(frame) = frame_of(node) {
            let m = frame.linear();
            v = (m[0][0] * v.0 + m[0][1] * v.1, m[1][0] * v.0 + m[1][1] * v.1);
        }
    }
    v
}

/// The linear maps of the ancestors of `chain`'s last shape, outermost first, multiplied: parent-local vectors to page vectors.
fn parent_linear(chain: &[&ResolvedShape]) -> [[f64; 2]; 2] {
    let mut total = [[1.0, 0.0], [0.0, 1.0]];
    for node in &chain[..chain.len() - 1] {
        if let Ok(frame) = frame_of(node) {
            let m = frame.linear();
            total = [
                [total[0][0] * m[0][0] + total[0][1] * m[1][0], total[0][0] * m[0][1] + total[0][1] * m[1][1]],
                [total[1][0] * m[0][0] + total[1][1] * m[1][0], total[1][0] * m[0][1] + total[1][1] * m[1][1]],
            ];
        }
    }
    total
}

/// The inverse of a 2 x 2 matrix of a turn and mirrors (determinant of absolute value 1).
fn linear_inverse(m: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    let det = if det.abs() < 1e-12 { 1.0 } else { det };
    [[m[1][1] / det, -m[0][1] / det], [-m[1][0] / det, m[0][0] / det]]
}

/// A linear map of a turn and at most one mirror as `(angle, mirrored)`: `R(angle) * diag(-1 if mirrored, 1)`.
fn decompose(m: [[f64; 2]; 2]) -> (f64, bool) {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if det >= 0.0 {
        (m[1][0].atan2(m[0][0]), false)
    } else {
        ((-m[1][0]).atan2(-m[0][0]), true)
    }
}

fn frame_of(node: &ResolvedShape) -> Result<Frame> {
    frame_ignoring_flip(node)
}

/// The frame cells of a shape, with its flips.
fn frame_ignoring_flip(node: &ResolvedShape) -> Result<Frame> {
    let w = num(node, "Width").ok_or_else(|| Error(format!("shape {} has no Width", node.id)))?;
    let h = num(node, "Height").ok_or_else(|| Error(format!("shape {} has no Height", node.id)))?;
    Ok(Frame {
        pin: (num(node, "PinX").unwrap_or(0.0), num(node, "PinY").unwrap_or(0.0)),
        size: (w, h),
        loc: (num(node, "LocPinX").unwrap_or(w / 2.0), num(node, "LocPinY").unwrap_or(h / 2.0)),
        angle: num(node, "Angle").unwrap_or(0.0),
        flip: (num(node, "FlipX").unwrap_or(0.0) != 0.0, num(node, "FlipY").unwrap_or(0.0) != 0.0),
    })
}

/// A format change for shapes: colours as `#rrggbb` (or `none`), the line weight in model pixels
/// (96 per inch), the line pattern 0..=23 (0 none, 1 solid, 2..=23 the dashes of the Visio gallery)
/// and the font size in points.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleEdit {
    pub fill: Option<String>,
    pub line: Option<String>,
    pub weight_px: Option<f64>,
    pub pattern: Option<u32>,
    pub font_pt: Option<f64>,
    /// Bold / italic / underline on (`Some(true)`) or off (`Some(false)`); `None` leaves it.
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    /// The font family name (the document lists it in its `FaceNames`; see `writer::ensure_face_names`).
    pub font_family: Option<String>,
    /// The text colour as `#rrggbb`.
    pub text_color: Option<String>,
    /// Fill / line transparency, 0 (opaque) to 1 (invisible): `FillForegndTrans` / `LineColorTrans`.
    pub fill_trans: Option<f64>,
    pub line_trans: Option<f64>,
    /// A drop shadow on (`ShdwPattern` 1 with a Visio-like outer shadow) or off (`ShdwPattern` 0).
    pub shadow: Option<bool>,
    /// Line ends: arrow kind 0..=45 (`BeginArrow` / `EndArrow`, see docs/27) and size 0..=6.
    pub begin_arrow: Option<u32>,
    pub end_arrow: Option<u32>,
    pub begin_arrow_size: Option<u32>,
    pub end_arrow_size: Option<u32>,
    /// The fill pattern 0..=40 (`FillPattern`: 1 solid, 2-24 patterns, 25-40 gradients between the fill colour and
    /// `fill_back`) and its second colour (`FillBkgnd`).
    pub fill_pattern: Option<u32>,
    pub fill_back: Option<String>,
    /// A gradient with custom stops (`FillGradient` rows): `(position 0..1, colour #rrggbb, transparency 0..1)`, 2 to 8
    /// stops, with the angle in degrees and the gradient type (`FillGradientDir`: 0 linear, 1-2 radial, 3.. other
    /// shapes). Turns `FillGradientEnabled` on.
    pub gradient_stops: Option<Vec<(f64, String, f64)>>,
    pub gradient_angle_deg: Option<f64>,
    pub gradient_dir: Option<u32>,
    /// `Some(false)` switches the gradient off (`FillGradientEnabled` 0; the fill is the plain fill colour again).
    pub gradient_enabled: Option<bool>,
    /// Glow (`GlowSize`, `GlowColor`, `GlowColorTrans`): the radius in pixels (0 switches it off), colour, transparency.
    pub glow_px: Option<f64>,
    pub glow_color: Option<String>,
    pub glow_trans: Option<f64>,
    /// Soft edges (`SoftEdgesSize`): the radius in pixels, 0 = off.
    pub soft_edges_px: Option<f64>,
    /// Reflection on / off (the preset Visio's own quick styles use: size 50 %, no gap, 2 pt blur, 30 % transparent).
    pub reflection: Option<bool>,
    /// Bevel (`BevelTopType`, 0 = none .. 12) with the width / height in pixels (default 6 pt).
    pub bevel: Option<u32>,
    pub bevel_px: Option<f64>,
    /// The shadow in detail (turns the shadow on): colour `#rrggbb`, offset in pixels (x right, y DOWN), blur in
    /// pixels, transparency 0..1.
    pub shadow_color: Option<String>,
    pub shadow_dx: Option<f64>,
    pub shadow_dy: Option<f64>,
    pub shadow_blur: Option<f64>,
    pub shadow_trans: Option<f64>,
    /// `ShapeShdwType` (1 simple, 2 oblique, 3 inner), the oblique shadow's scale factor (1 = 100 %) and angle in degrees.
    pub shadow_type: Option<u32>,
    pub shadow_scale: Option<f64>,
    pub shadow_angle: Option<f64>,
    /// `LineCap`: 0 round, 1 flat, 2 square; and the corner radius `Rounding` in pixels.
    pub line_cap: Option<u32>,
    pub rounding_px: Option<f64>,
    /// Protection: `Some(true)` sets the position / size / delete locks (`LockMoveX`, `LockMoveY`, `LockWidth`,
    /// `LockHeight`, `LockRotate`, `LockDelete` = 1), `Some(false)` clears them (0).
    pub lock: Option<bool>,
}

fn hex_colour(value: &str) -> Result<String> {
    let v = value.trim();
    let ok = v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok {
        Ok(v.to_ascii_lowercase())
    } else {
        Err(Error(format!("{value:?} is not a colour like #1971c2")))
    }
}

/// The geometry of a connector that is about to be added (see `GroupEditor::plan_connector`).
#[derive(Debug, Clone)]
pub struct ConnectorPlan {
    pub begin: (f64, f64),
    pub end: (f64, f64),
    /// The elbow points in page inches, first = begin, last = end.
    pub route: Vec<(f64, f64)>,
}

/// Add a right-angle connector from one shape end to another and glue both ends, the way Visio
/// writes a dynamic connector: an instance of the document's "Dynamic connector" master when it
/// has one (a plain 1-D shape otherwise) whose ends are `PAR(PNT(Sheet.n!Connections.Xk, ...))`
/// formulas, plus the `Connect` rows. Returns the new package bytes and the new shape's id.
pub fn add_connector(
    package: &Package,
    page_index: usize,
    from: (&str, Option<usize>),
    to: (&str, Option<usize>),
) -> Result<(Vec<u8>, String)> {
    let end = |(shape, site): (&str, Option<usize>)| EndTarget::Glue(shape.to_string(), site);
    add_connector_between(package, page_index, &end(from), &end(to))
}

/// The shortest connector that may be drawn with a loose end, in inches.
const MIN_LOOSE_LENGTH: f64 = 0.05;

/// [`add_connector`] where either end may also be loose at a page position, written as a dragged-off end is
/// ([`move_connector_end`]): literal coordinates, no trigger and no `Connect` row. The stored route turns at right angles between
/// two shapes; with a loose end it is the straight line between the ends, which the scene routes at right angles when it draws it.
pub fn add_connector_between(package: &Package, page_index: usize, from: &EndTarget, to: &EndTarget) -> Result<(Vec<u8>, String)> {
    let editor = GroupEditor::open(package, page_index)?;
    let glue = |end: &EndTarget| match end {
        EndTarget::Glue(shape, site) => Some((shape.clone(), *site)),
        EndTarget::Free(..) => None,
    };
    let (from_glue, to_glue) = (glue(from), glue(to));
    let plan = match (&from_glue, &to_glue) {
        (Some(a), Some(b)) => editor.plan_connector((&a.0, a.1), (&b.0, b.1))?,
        _ => {
            for (shape, site) in [&from_glue, &to_glue].into_iter().flatten() {
                if editor.chain(shape).is_none() {
                    return Err(Error(format!("no shape {shape}")));
                }
                if let Some(n) = site {
                    if editor.site(shape, *n).is_none() {
                        return Err(Error(format!("shape {shape} has no connection point {n}")));
                    }
                }
            }
            // A glued end takes the connection point nearest to the loose one.
            let (begin, end) = match (from, to) {
                (EndTarget::Free(ax, ay), EndTarget::Free(bx, by)) => ((*ax, *ay), (*bx, *by)),
                (EndTarget::Free(x, y), glued) => ((*x, *y), editor.end_point(glued, (*x, *y))?),
                (glued, EndTarget::Free(x, y)) => (editor.end_point(glued, (*x, *y))?, (*x, *y)),
                _ => return Err(Error("a connector needs two ends".to_string())),
            };
            let length = (end.0 - begin.0).hypot(end.1 - begin.1);
            if !length.is_finite() || length < MIN_LOOSE_LENGTH {
                return Err(Error("the two ends of the connector are too close together".to_string()));
            }
            ConnectorPlan { begin, end, route: vec![begin, end] }
        }
    };
    let id = crate::writer::next_shape_id(package, page_index)?.to_string();
    let catalog = MasterCatalog::load(package)?;
    let master = catalog
        .masters
        .values()
        .find(|m| m.name.eq_ignore_ascii_case("Dynamic connector"))
        .map(|m| m.id.clone());
    let (w, h) = (plan.end.0 - plan.begin.0, plan.end.1 - plan.begin.1);
    let cell = |name: &str, value: f64, formula: Option<String>| match formula {
        Some(f) => format!("<Cell N='{name}' V='{}' F='{}'/>", fmt(value), escape_xml_attr(&f)),
        None => format!("<Cell N='{name}' V='{}'/>", fmt(value)),
    };
    let themed = if master.is_none() { crate::draw::themed_style(package) } else { None };
    let mut xml = match &master {
        Some(m) => format!("<Shape ID='{id}' NameU='Dynamic connector.{id}' Name='Dynamic connector.{id}' Type='Shape' Master='{m}'>"),
        // a drawing without a "Dynamic connector" master (a new drawing): a plain connector shape in the document's theme-based Normal style (Q7: it follows the theme)
        None => match themed {
            Some(s) => format!("<Shape ID='{id}' NameU='Connector.{id}' Name='Connector.{id}' Type='Shape' LineStyle='{s}' FillStyle='{s}' TextStyle='{s}'>"),
            None => format!("<Shape ID='{id}' NameU='Connector.{id}' Name='Connector.{id}' Type='Shape'>"),
        },
    };
    let inh = |v: f64| match &master {
        Some(_) => format!("<Cell N='@' V='{}' F='Inh'/>", fmt(v)),
        None => format!("<Cell N='@' V='{}'/>", fmt(v)),
    };
    xml.push_str(&inh(plan.begin.0 + w / 2.0).replace('@', "PinX"));
    xml.push_str(&inh(plan.begin.1 + h / 2.0).replace('@', "PinY"));
    xml.push_str(&cell("Width", w, Some("GUARD(EndX-BeginX)".to_string())));
    xml.push_str(&cell("Height", h, Some("GUARD(EndY-BeginY)".to_string())));
    xml.push_str(&inh(w / 2.0).replace('@', "LocPinX"));
    xml.push_str(&inh(h / 2.0).replace('@', "LocPinY"));
    let formula = |end: &Option<(String, Option<usize>)>, begin: bool| end.as_ref().map(|(shape, site)| glue_formula(&(shape.as_str(), *site), begin));
    xml.push_str(&cell("BeginX", plan.begin.0, formula(&from_glue, true)));
    xml.push_str(&cell("BeginY", plan.begin.1, formula(&from_glue, true)));
    xml.push_str(&cell("EndX", plan.end.0, formula(&to_glue, false)));
    xml.push_str(&cell("EndY", plan.end.1, formula(&to_glue, false)));
    xml.push_str("<Cell N='LayerMember' V='0'/>");
    if themed.is_some() {
        xml.push_str(crate::draw::QUICK_STYLE);
    }
    let trigger = |shape: &str| format!("_XFTRIGGER(Sheet.{shape}!EventXFMod)");
    for (name, end) in [("BegTrigger", &from_glue), ("EndTrigger", &to_glue)] {
        if let Some((shape, _)) = end {
            xml.push_str(&format!("<Cell N='{name}' V='2' F='{}'/>", escape_xml_attr(&trigger(shape))));
        }
    }
    xml.push_str("<Cell N='ShapeRouteStyle' V='1'/>");
    if master.is_none() {
        xml.push_str("<Cell N='WalkPreference' V='0'/>");
        xml.push_str("<Cell N='ObjType' V='2'/><Cell N='ConFixedCode' V='6'/><Cell N='EndArrow' V='4'/>");
    }
    xml.push_str("<Section N='Geometry' IX='0'>");
    for (i, p) in plan.route.iter().enumerate() {
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        xml.push_str(&format!(
            "<Row T='{kind}' IX='{}'><Cell N='X' V='{}'/><Cell N='Y' V='{}'/></Row>",
            i + 1,
            fmt(p.0 - plan.begin.0),
            fmt(p.1 - plan.begin.1)
        ));
    }
    xml.push_str("</Section></Shape>");
    let row = |from_cell: &str, from_part: u32, end: &Option<(String, Option<usize>)>| end.as_ref().map_or_else(String::new, |(shape, site)| connect_row(&id, from_cell, from_part, &(shape.as_str(), *site)));
    let rows = format!("{}{}", row("BeginX", 9, &from_glue), row("EndX", 12, &to_glue));
    let bytes = crate::writer::append_shape_with_connects(package, page_index, &xml, &rows)?;
    // The page must relate to the connector master too (see `writer::page_master_relationship`).
    let bytes = match &master {
        Some(m) => {
            let added = Package::from_reader(std::io::Cursor::new(bytes.clone()))?;
            match crate::writer::page_master_relationship(&added, page_index, m)? {
                Some((rels_part, rels)) => crate::writer::with_part(&added, &rels_part, rels.into_bytes())?,
                None => bytes,
            }
        }
        None => bytes,
    };
    Ok((bytes, id))
}

/// The glue formula of a connector end: the connection point's position, or `_WALKGLUE` when the
/// connector is glued to the whole shape (Visio then picks the side; the master defines
/// `WalkPreference`). `begin_end` is true for the begin end.
fn glue_formula(end_of: &(&str, Option<usize>), begin_end: bool) -> String {
    match end_of.1 {
        Some(n) => format!("PAR(PNT(Sheet.{0}!Connections.X{1},Sheet.{0}!Connections.Y{1}))", end_of.0, n),
        None if begin_end => "_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)".to_string(),
        None => "_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_string(),
    }
}

/// The `Connect` row for one connector end. Connection point k is glued with ToPart 99 + k (the
/// first one is 100); the whole shape with 3.
fn connect_row(connector: &str, from_cell: &str, from_part: u32, end: &(&str, Option<usize>)) -> String {
    match end.1 {
        Some(n) => format!(
            "<Connect FromSheet='{connector}' FromCell='{from_cell}' FromPart='{from_part}' ToSheet='{}' ToCell='Connections.X{n}' ToPart='{}'/>",
            end.0,
            99 + n
        ),
        None => format!(
            "<Connect FromSheet='{connector}' FromCell='{from_cell}' FromPart='{from_part}' ToSheet='{}' ToCell='PinX' ToPart='3'/>",
            end.0
        ),
    }
}

/// Copy shapes for the clipboard: returns the shapes' XML and the `Connect` rows (naming the old
/// ids) of the copied connectors' ends that stay inside the copy.
///
/// * a shape of the page itself is copied as it is;
/// * a member of a group is copied as a page-level shape: its pin and angle become its position
///   and rotation on the page (a flipped group cannot be unfolded and is refused);
/// * a connector keeps the glue of an end whose shape is part of the copy (the formulas follow the
///   renumbering and the Connect row is returned); an end glued to a shape that is not copied is
///   made loose, like a connector pulled off its shape. A connector inside a group is copied as a loose page-level connector (a turned or flipped group refuses);
///   connectors among the members of a copied group keep their glue to the copied members.
pub fn copy_shapes(package: &Package, page_index: usize, ids: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let editor = GroupEditor::open(package, page_index)?;
    let part = package
        .pages()?
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} does not exist")))?
        .part
        .clone();
    let connects = package.connects(&part)?;
    let mut inside: Vec<String> = Vec::new();
    for id in ids {
        inside.extend(editor.subtree_ids(id)?);
    }
    let is_connector = |node: &ResolvedShape| ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|name| num(node, name).is_some());
    let mut xmls = Vec::new();
    let mut rows = Vec::new();
    for id in ids {
        let chain = editor.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
        let node = chain[chain.len() - 1];
        let xml = crate::writer::shape_xml(package, page_index, id)?;
        if chain.len() > 1 {
            // A member of a group: the page-level pin and angle.
            if is_connector(node) {
                // A connector inside a group becomes a loose page-level connector: its ends and pin in page
                // inches. Its own geometry is relative to its frame, so only a plain (unturned) path keeps its shape.
                let parent_id = chain[chain.len() - 2].id.clone();
                let mut angle = 0.0;
                for n in &chain {
                    let f = frame_of(n)?;
                    if f.flip.0 || f.flip.1 {
                        return Err(Error(format!("connector {id} sits in a flipped group and cannot be copied")));
                    }
                    angle += f.angle;
                }
                if angle.rem_euclid(std::f64::consts::TAU).min(std::f64::consts::TAU - angle.rem_euclid(std::f64::consts::TAU)) > EPS {
                    return Err(Error(format!("connector {id} sits in a turned group and cannot be copied")));
                }
                let up = |x: f64, y: f64| editor.world_point(&parent_id, (x, y)).ok_or_else(|| Error(format!("connector {id} sits in a flipped group and cannot be copied")));
                let mut patch = ShapeCellPatch::new(id.clone());
                for (xn, yn) in [("BeginX", "BeginY"), ("EndX", "EndY"), ("PinX", "PinY")] {
                    let p = up(num(node, xn).unwrap_or(0.0), num(node, yn).unwrap_or(0.0))?;
                    patch = patch.set(xn, fmt(p.0)).set(yn, fmt(p.1));
                    // An end glued to a shape that is copied too stays glued to the copy (the shapes become page-level, so
                    // the glue formula reads the page positions).
                    let (prefix, from_cell, from_part, begin_end) = match xn {
                        "BeginX" => ("Begin", "BeginX", 9u32, true),
                        "EndX" => ("End", "EndX", 12u32, false),
                        _ => continue,
                    };
                    if let Some(c) = connects.iter().find(|c| c.from_sheet == *id && c.from_cell == from_cell && inside.contains(&c.to_sheet)) {
                        let n = c.to_cell.strip_prefix("Connections.X").and_then(|n| n.parse::<usize>().ok());
                        let formula = glue_formula(&(c.to_sheet.as_str(), n), begin_end);
                        patch = patch.set_with_formula(xn, fmt(p.0), formula.clone()).set_with_formula(yn, fmt(p.1), formula);
                        let trigger = if begin_end { "BegTrigger" } else { "EndTrigger" };
                        patch = patch.set_with_formula(trigger, "2", format!("_XFTRIGGER(Sheet.{}!EventXFMod)", c.to_sheet));
                        rows.push(connect_row(id, from_cell, from_part, &(c.to_sheet.as_str(), n)));
                        let _ = prefix;
                    }
                }
                xmls.push(crate::writer::patch_shape_xml(&xml, &patch)?);
                continue;
            }
            let frame = frame_of(node)?;
            let pin = editor.world_point(id, frame.loc).ok_or_else(|| Error(format!("shape {id} cannot be placed on the page")))?;
            // As a page shape its angle and mirror are those of the whole chain's linear map (a mirrored group turns and
            // mirrors its members): `R(angle) * diag(-1 if FlipX, 1)`.
            let (ex, ey) = (world_vector(&chain, (1.0, 0.0)), world_vector(&chain, (0.0, 1.0)));
            let (angle, mirrored) = decompose([[ex.0, ey.0], [ex.1, ey.1]]);
            let patch = ShapeCellPatch::new(id.clone())
                .set("PinX", fmt(pin.0))
                .set("PinY", fmt(pin.1))
                .set("Angle", fmt(angle.rem_euclid(std::f64::consts::TAU)))
                .set("FlipX", if mirrored { "1" } else { "0" })
                .set("FlipY", "0");
            xmls.push(crate::writer::patch_shape_xml(&xml, &patch)?);
        } else if is_connector(node) {
            let mut patch = ShapeCellPatch::new(id.clone());
            let mut loosened = false;
            for (prefix, from_cell, from_part) in [("Begin", "BeginX", 9u32), ("End", "EndX", 12u32)] {
                let glue = connects.iter().find(|c| c.from_sheet == *id && c.from_cell == from_cell);
                match glue {
                    Some(c) if inside.contains(&c.to_sheet) => {
                        let n = c.to_cell.strip_prefix("Connections.X").and_then(|n| n.parse::<usize>().ok());
                        rows.push(connect_row(id, from_cell, from_part, &(c.to_sheet.as_str(), n)));
                    }
                    Some(_) => {
                        for axis in ["X", "Y"] {
                            let name = format!("{prefix}{axis}");
                            patch = patch.set(name.clone(), fmt(num(node, &name).unwrap_or(0.0)));
                        }
                        loosened = true;
                    }
                    None => {}
                }
            }
            xmls.push(if loosened { crate::writer::patch_shape_xml(&xml, &patch)? } else { xml });
        } else {
            // A group (or other shape with members): connectors among its members keep the glue to shapes that are
            // part of the copy; an end glued to a shape outside the copy is made loose, in the member's own coordinates.
            let mut xml = xml;
            for descendant in editor.subtree_ids(id)?.into_iter().skip(1) {
                let Some(chain) = editor.chain(&descendant) else { continue };
                let child = chain[chain.len() - 1];
                if !is_connector(child) {
                    continue;
                }
                let mut patch = ShapeCellPatch::new(descendant.clone());
                let mut loosened = false;
                for (prefix, from_cell, from_part) in [("Begin", "BeginX", 9u32), ("End", "EndX", 12u32)] {
                    match connects.iter().find(|c| c.from_sheet == descendant && c.from_cell == from_cell) {
                        Some(c) if inside.contains(&c.to_sheet) => {
                            let n = c.to_cell.strip_prefix("Connections.X").and_then(|n| n.parse::<usize>().ok());
                            rows.push(connect_row(&descendant, from_cell, from_part, &(c.to_sheet.as_str(), n)));
                        }
                        Some(_) => {
                            for axis in ["X", "Y"] {
                                let name = format!("{prefix}{axis}");
                                patch = patch.set(name.clone(), fmt(num(child, &name).unwrap_or(0.0)));
                            }
                            loosened = true;
                        }
                        None => {}
                    }
                }
                if loosened {
                    xml = crate::writer::patch_shape_xml(&xml, &patch)?;
                }
            }
            xmls.push(xml);
        }
    }
    Ok((xmls, rows))
}

/// Where one end of an existing connector goes (see `move_connector_end`).
#[derive(Debug, Clone, PartialEq)]
pub enum EndTarget {
    /// Glued to shape `.0`: connection point `.1` (1-based) or, with `None`, the whole shape.
    Glue(String, Option<usize>),
    /// Loose at a page position (inches).
    Free(f64, f64),
}

/// Move one end of the page-level connector `id` (the begin end when `begin`): glue it to a shape
/// or set it free, the way dragging a connector end does in Visio. Glued ends get the glue
/// formula, the trigger formula and a `Connect` row for the new shape; a loose end gets literal
/// coordinates and loses its `Connect` row (probe: Visio drops the row when the cell is set to a
/// literal). The route is recomputed (right-angle when both ends are glued, else straight).
/// Returns the new package bytes.
pub fn move_connector_end(package: &Package, page_index: usize, id: &str, begin: bool, target: EndTarget) -> Result<Vec<u8>> {
    let editor = GroupEditor::open(package, page_index)?;
    let part = package
        .pages()?
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} does not exist")))?
        .part
        .clone();
    let connects = package.connects(&part)?;
    // A TALL one-dimensional group (a PDU: Width = the length of the Begin-End line, Height follows, Angle = the line's direction):
    // dragging an end stretches AND turns it about the middle of the line (Visio, step 227).
    if let Some(node) = editor.roots.iter().find(|n| n.id == id && turnable_one_d(n)) {
        let EndTarget::Free(x, y) = &target else {
            return Err(Error("a one-dimensional group cannot be glued; drag its end to a free place".to_string()));
        };
        return move_tall_one_d_end(package, page_index, &editor, node, begin, (*x, *y));
    }
    // A connector of the page, or one inside a group that only translates (its cells are in the group's coordinates).
    let (node, off) = editor
        .connector_nodes()
        .into_iter()
        .find(|(n, _)| n.id == id)
        .ok_or_else(|| Error(format!("shape {id} is not a connector on the page or inside an unturned group")))?;
    if num(node, "Angle").unwrap_or(0.0).abs() > EPS {
        return Err(Error("a rotated connector cannot be edited".to_string()));
    }
    // A one-dimensional GROUP (the equipment masters of the Dell / HPE stencils: a 19 inch line with the art in its
    // children and no geometry of its own) has no route to rewrite: moving an end sets its LENGTH, the children and the
    // height follow (uniform scale), the other end stays where it is.
    if node.shape_type.as_deref() == Some("Group") && (!node.children.is_empty() || !node.master_children.is_empty()) {
        let EndTarget::Free(x, _) = &target else {
            return Err(Error("a one-dimensional group cannot be glued; drag its end to a free place".to_string()));
        };
        return resize_one_d_group(package, page_index, &editor, node, off, begin, *x);
    }
    let (moved, other) = if begin { ("Begin", "End") } else { ("End", "Begin") };
    let glue_of = |prefix: &str| {
        connects
            .iter()
            .find(|c| c.from_sheet == id && c.from_cell == format!("{prefix}X"))
            .map(|c| (c.to_sheet.clone(), c.to_cell.strip_prefix("Connections.X").and_then(|n| n.parse::<usize>().ok())))
    };
    let other_glue = glue_of(other);
    let other_pos = (num(node, &format!("{other}X")).unwrap_or(0.0) + off.0, num(node, &format!("{other}Y")).unwrap_or(0.0) + off.1);
    if let EndTarget::Glue(sheet, n) = &target {
        if sheet == id {
            return Err(Error("a connector cannot be glued to itself".to_string()));
        }
        if editor.chain(sheet).is_none() {
            return Err(Error(format!("no shape {sheet}")));
        }
        if let Some(n) = n {
            if editor.site(sheet, *n).is_none() {
                return Err(Error(format!("shape {sheet} has no connection point {n}")));
            }
        }
    }
    let target_ref = match &target {
        EndTarget::Glue(sheet, n) => Some((sheet.clone(), *n)),
        EndTarget::Free(..) => None,
    };
    let (first, second) = if begin { (target_ref.clone(), other_glue.clone()) } else { (other_glue.clone(), target_ref.clone()) };
    let (begin_pos, end_pos, route) = match (&first, &second) {
        (Some(a), Some(b)) => {
            let plan = editor.plan_connector((&a.0, a.1), (&b.0, b.1))?;
            (plan.begin, plan.end, plan.route)
        }
        _ => {
            let target_pos = editor.end_point(&target, other_pos)?;
            let (b, e) = if begin { (target_pos, other_pos) } else { (other_pos, target_pos) };
            (b, e, vec![b, e])
        }
    };
    let (w, h) = (end_pos.0 - begin_pos.0, end_pos.1 - begin_pos.1);
    let mut patch = ShapeCellPatch::new(id);
    for (prefix, pos, is_begin) in [("Begin", begin_pos, true), ("End", end_pos, false)] {
        let is_moved = prefix == moved;
        let glued = if is_begin { &first } else { &second };
        for (axis, value) in [("X", pos.0 - off.0), ("Y", pos.1 - off.1)] {
            let name = format!("{prefix}{axis}");
            patch = match (is_moved, glued) {
                (true, Some((sheet, n))) => patch.set_with_formula(name, fmt(value), glue_formula(&(sheet.as_str(), *n), is_begin)),
                (true, None) => patch.set(name, fmt(value)),
                _ => patch.set_cached(name, fmt(value)),
            };
        }
        if let (true, Some((sheet, _))) = (is_moved, glued) {
            let trigger = if is_begin { "BegTrigger" } else { "EndTrigger" };
            patch = patch.set_with_formula(trigger, "2", format!("_XFTRIGGER(Sheet.{sheet}!EventXFMod)"));
        }
    }
    let mut rows = Vec::new();
    for (i, p) in route.iter().enumerate() {
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        rows.push(
            GeometryRow::new(kind, (i + 1).to_string())
                .set("X", fmt(p.0 - begin_pos.0))
                .set("Y", fmt(p.1 - begin_pos.1)),
        );
    }
    patch = patch
        .set_cached("PinX", fmt(begin_pos.0 + w / 2.0 - off.0))
        .set_cached("PinY", fmt(begin_pos.1 + h / 2.0 - off.1))
        .set_cached("Width", fmt(w))
        .set_cached("Height", fmt(h))
        .set_cached("LocPinX", fmt(w / 2.0))
        .set_cached("LocPinY", fmt(h / 2.0))
        .geometry_rows("0", rows);
    let bytes = crate::writer::patch_page_cells(package, page_index, &[patch])?;
    let patched = Package::from_reader(std::io::Cursor::new(bytes))?;
    let (cell, part_no) = if begin { ("BeginX", 9) } else { ("EndX", 12) };
    let new_row = target_ref
        .as_ref()
        .map(|(sheet, n)| connect_row(id, cell, part_no, &(sheet.as_str(), *n)))
        .unwrap_or_default();
    crate::writer::replace_connect_rows(&patched, page_index, id, &[cell], &new_row)
}

/// Moving an end of a one-dimensional group sets its length along the x axis (see `move_connector_end`).
/// A one-dimensional GROUP (Begin / End cells, art in the children) whose box is tall (the flat ones are listed as connectors).
fn tall_one_d(node: &ResolvedShape) -> bool {
    node.shape_type.as_deref() == Some("Group")
        && (!node.children.is_empty() || !node.master_children.is_empty())
        && ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|n| num(node, n).is_some())
        && num(node, "Height").is_some_and(|h| h.abs() > 0.05)
}

/// A one-dimensional group that is turned by dragging an end: the tall ones (`tall_one_d`) and the flat ones (rack servers, Height ~ 0) whose
/// cached Begin-End line is as long as the box is wide, so that the box IS the line (probe-flat-1d-turn.ps1, IBM Console Switch Rear: the same
/// rule as for the tall ones, Pin = the middle of the line, Width = its length, LocPin = Width * 0.5, Angle = ATAN2). A flat group whose ends do not
/// span the box (the Dell servers: ends 0.19 in apart on a 1.9 in box) keeps the plain x-only end path.
fn turnable_one_d(node: &ResolvedShape) -> bool {
    if tall_one_d(node) {
        return true;
    }
    let get = |n: &str| num(node, n);
    let (Some(bx), Some(by), Some(ex), Some(ey), Some(w)) = (get("BeginX"), get("BeginY"), get("EndX"), get("EndY"), get("Width")) else { return false };
    node.shape_type.as_deref() == Some("Group")
        && (!node.children.is_empty() || !node.master_children.is_empty())
        && w > 0.1
        && ((ex - bx).hypot(ey - by) - w).abs() < 0.01 * w.max(1.0)
}

/// Move one end of a tall one-dimensional group to `to` (page coordinates). Visio's rule (probe-tall-1d-turn.ps1, PDU83107): PinX / PinY =
/// the middle of Begin-End, Width = the length of the line, Height = Width times the master's ratio (GUARD(Width * k)), LocPin = (Width, 0)
/// (the same proportions as before), Angle = ATAN2(EndY - BeginY, EndX - BeginX); the art in the children scales with the box.
fn move_tall_one_d_end(package: &Package, page_index: usize, editor: &GroupEditor, node: &ResolvedShape, begin: bool, to: (f64, f64)) -> Result<Vec<u8>> {
    const MIN_LENGTH: f64 = 0.05;
    let get = |n: &str| num(node, n).ok_or_else(|| Error(format!("the group has no {n}")));
    let (b, e) = ((get("BeginX")?, get("BeginY")?), (get("EndX")?, get("EndY")?));
    let (nb, ne) = if begin { (to, e) } else { (b, to) };
    let (dx, dy) = (ne.0 - nb.0, ne.1 - nb.1);
    let len = dx.hypot(dy);
    if len < MIN_LENGTH {
        return Err(Error("the two ends of the group cannot be that close together".to_string()));
    }
    let old = frame_of(node)?;
    let ratio = old.size.1 / old.size.0.max(EPS);
    let mut patches = editor.resize_member(&node.id, (len, len * ratio), (1.0, 0.0), false)?;
    let mid = ((nb.0 + ne.0) / 2.0, (nb.1 + ne.1) / 2.0);
    if let Some(patch) = patches.iter_mut().find(|p| p.visio_id == node.id) {
        *patch = std::mem::replace(patch, ShapeCellPatch::new(node.id.clone()))
            .set("PinX", fmt(mid.0))
            .set("PinY", fmt(mid.1))
            .set("LocPinX", fmt(len * old.loc.0 / old.size.0.max(EPS)))
            .set("LocPinY", fmt(len * ratio * old.loc.1 / old.size.1.max(EPS)))
            .set("Angle", fmt(dy.atan2(dx)))
            .set_cached("BeginX", fmt(nb.0))
            .set_cached("BeginY", fmt(nb.1))
            .set_cached("EndX", fmt(ne.0))
            .set_cached("EndY", fmt(ne.1));
    }
    crate::writer::patch_page_cells(package, page_index, &patches)
}

fn resize_one_d_group(package: &Package, page_index: usize, editor: &GroupEditor, node: &ResolvedShape, off: (f64, f64), begin: bool, x: f64) -> Result<Vec<u8>> {
    const MIN_LENGTH: f64 = 0.05;
    let begin_x = num(node, "BeginX").ok_or_else(|| Error("the group has no BeginX".to_string()))? + off.0;
    let end_x = num(node, "EndX").ok_or_else(|| Error("the group has no EndX".to_string()))? + off.0;
    if end_x < begin_x {
        return Err(Error("a one-dimensional group that runs right to left cannot be resized here".to_string()));
    }
    // the moved end stays on its side of the other one
    let (new_begin, new_end) = if begin { (x.min(end_x - MIN_LENGTH), end_x) } else { (begin_x, x.max(begin_x + MIN_LENGTH)) };
    let new_len = new_end - new_begin;
    let old = frame_of(node)?;
    let anchor = (if begin { 1.0 } else { 0.0 }, 0.5);
    let new_size = (new_len, old.size.1 * new_len / old.size.0.max(EPS));
    let mut patches = editor.resize_member(&node.id, new_size, anchor, false)?;
    if let Some(patch) = patches.iter_mut().find(|p| p.visio_id == node.id) {
        *patch = std::mem::replace(patch, ShapeCellPatch::new(node.id.clone()))
            .set_cached("BeginX", fmt(new_begin - off.0))
            .set_cached("EndX", fmt(new_end - off.0));
    }
    crate::writer::patch_page_cells(package, page_index, &patches)
}

fn escape_xml_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('\'', "&apos;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Edits one page of a package. Holds the resolved shape tree.
pub struct GroupEditor {
    roots: Vec<ResolvedShape>,
    /// Shapes that are taken to be moved by a page delta while a preview is computed (`preview_connector_routes`); `None` otherwise.
    shift: std::cell::RefCell<Option<(Vec<String>, (f64, f64))>>,
}

/// The ends and the route of a connector between two shapes, each given by the sites it may leave from (one site for a connector
/// glued to a connection point, several for one glued to the whole shape) and its page box. Shapes in a turned group have their
/// sites off the sides of the axis-aligned page box, which the side router cannot use: then the two sites that lie closest
/// together are taken and routed between.
fn route_ends(from_sites: &[Site], a: Rect, to_sites: &[Site], b: Rect) -> Option<(Site, Site, Vec<(f64, f64)>, bool)> {
    let origin = ((a.x0 + a.x1) / 2.0, (a.y0 + a.y1) / 2.0);
    if from_sites.len() == 1 && to_sites.len() == 1 {
        return Some((from_sites[0], to_sites[0], route_right_angle(from_sites[0], to_sites[0], &[a, b], origin), false));
    }
    if let Some((f, t, route)) = route_between_shapes(from_sites, a, to_sites, b) {
        return Some((f, t, route, false));
    }
    let mut best: Option<(f64, Site, Site)> = None;
    for f in from_sites {
        for t in to_sites {
            let d = (f.point.0 - t.point.0).hypot(f.point.1 - t.point.1);
            if best.map_or(true, |(bd, _, _)| d < bd) {
                best = Some((d, *f, *t));
            }
        }
    }
    let (_, f, t) = best?;
    Some((f, t, route_right_angle(f, t, &[a, b], origin), true))
}

impl GroupEditor {
    pub fn open(package: &Package, page_index: usize) -> Result<Self> {
        let catalog = MasterCatalog::load(package)?;
        Self::open_with_catalog(package, page_index, &catalog)
    }

    /// Like `open` with a master catalog that was loaded from the same masters already (loading it is ~30 ms on a drawing with many masters).
    pub fn open_with_catalog(package: &Package, page_index: usize, catalog: &MasterCatalog) -> Result<Self> {
        let pages = package.pages()?;
        let page = pages
            .get(page_index)
            .ok_or_else(|| Error(format!("page {page_index} does not exist")))?;
        let tree = package.page_shapes(&page.part)?;
        Ok(Self { roots: catalog.resolve(&tree)?, shift: Default::default() })
    }

    /// Every shape of the page with its frame, for a UI: id, parent id, whether it is a group,
    /// the frame in its parent's coordinates (inches, y up) and whether it can be edited
    /// (flipped shapes cannot). `angle` is in radians.
    pub fn describe(&self) -> serde_json::Value {
        fn walk(nodes: &[ResolvedShape], parent: Option<&str>, out: &mut Vec<serde_json::Value>) {
            for node in nodes {
                let (frame, editable) = match frame_of(node) {
                    Ok(frame) => (Some(frame), true),
                    // A flipped group on the page itself can still be moved, turned and flipped back.
                    Err(_) if parent.is_none() => match frame_ignoring_flip(node) {
                        Ok(frame) => (Some(frame), true),
                        Err(_) => (None, false),
                    },
                    Err(_) => (None, false),
                };
                let locked = ["LockMoveX", "LockMoveY", "LockDelete"].iter().all(|n| node.cells.get(*n).and_then(|c| c.value.as_deref()).is_some_and(|v| v.trim() == "1"));
                out.push(serde_json::json!({
                    "id": node.id,
                    "parentId": parent,
                    "isGroup": node.shape_type.as_deref() == Some("Group"),
                    "editable": editable,
                    "pin": frame.map(|f| [f.pin.0, f.pin.1]),
                    "size": frame.map(|f| [f.size.0, f.size.1]),
                    "loc": frame.map(|f| [f.loc.0, f.loc.1]),
                    "angle": frame.map(|f| f.angle),
                    "flipX": frame.is_some_and(|f| f.flip.0),
                    "flipY": frame.is_some_and(|f| f.flip.1),
                    "locked": locked,
                    // a TALL one-dimensional group on the page (CyberPower / APC PDUs): its Begin / End points, for two end handles (step 227)
                    // the vertices of a plain freeform shape (page inches) for the Edit Points mode (step 251): {points, closed}
                    "vertices": if parent.is_none() && editable { vertices::describe_vertices(node) } else { serde_json::Value::Null },
                    "ends": if parent.is_none() && editable && turnable_one_d(node) {
                        serde_json::json!([[num(node, "BeginX"), num(node, "BeginY")], [num(node, "EndX"), num(node, "EndY")]])
                    } else {
                        serde_json::Value::Null
                    },
                }));
                walk(&node.children, Some(&node.id), out);
            }
        }
        let mut out = Vec::new();
        walk(&self.roots, None, &mut out);
        serde_json::Value::Array(out)
    }

    /// The page position of the origin of shape `id`'s own box when no turn and no flip sits between it and the page
    /// (the shape and its groups only translate); `None` otherwise. A connector inside such a group keeps its cells in
    /// the group's coordinates, which are the page's shifted by this offset.
    fn plain_origin(&self, id: &str) -> Option<(f64, f64)> {
        let chain = self.chain(id)?;
        let mut angle = 0.0;
        for node in &chain {
            let frame = frame_of(node).ok()?;
            if frame.flip.0 || frame.flip.1 {
                return None;
            }
            angle += frame.angle;
        }
        if angle.rem_euclid(std::f64::consts::TAU).min(std::f64::consts::TAU - angle.rem_euclid(std::f64::consts::TAU)) > EPS {
            return None;
        }
        self.world_point(id, (0.0, 0.0))
    }

    /// Every connector the editor can follow and edit: the page's own (offset 0) and those inside groups that only
    /// translate (offset = page position of the group's origin; their cells are in the group's coordinates).
    fn connector_nodes(&self) -> Vec<(&ResolvedShape, (f64, f64))> {
        fn walk<'a>(editor: &GroupEditor, nodes: &'a [ResolvedShape], parent: Option<&str>, out: &mut Vec<(&'a ResolvedShape, (f64, f64))>) {
            for node in nodes {
                // A one-dimensional GROUP whose frame has a real height (the CyberPower / APC rack PDUs: 2.2 x 72 in) is a symbol with a box,
                // not a connector. One with a flat frame (the Dell / HPE servers, height ~ 0) keeps its end handles: only those make sense for it.
                let symbol = (!node.children.is_empty() || !node.master_children.is_empty()) && num(node, "Height").is_some_and(|h| h.abs() > 0.05);
                if !symbol && ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|name| num(node, name).is_some()) {
                    match parent {
                        None => out.push((node, (0.0, 0.0))),
                        Some(parent) => {
                            if let Some(offset) = editor.plain_origin(parent) {
                                out.push((node, offset));
                            }
                        }
                    }
                }
                walk(editor, &node.children, Some(&node.id), out);
            }
        }
        let mut out = Vec::new();
        walk(self, &self.roots, None, &mut out);
        out
    }

    /// The connectors (straight or elbow, not rotated; also those inside groups that only translate) as JSON for a UI:
    /// `[{id, route: [[x, y]...], begin: {x, y, glue}, end: {x, y, glue}}]` in page inches, with
    /// `glue` = `{sheet, n}` (`n` = connection point or null for the whole shape) or null.
    pub fn connectors(&self, connects: &[Connect]) -> serde_json::Value {
        let mut out = Vec::new();
        for (node, off) in self.connector_nodes() {
            let cells: Option<Vec<f64>> = ["BeginX", "BeginY", "EndX", "EndY"].iter().map(|name| num(node, name)).collect();
            let Some(mut cells) = cells else { continue };
            cells[0] += off.0;
            cells[1] += off.1;
            cells[2] += off.0;
            cells[3] += off.1;
            if num(node, "Angle").unwrap_or(0.0).abs() > EPS {
                continue;
            }
            let glue = |prefix: &str| {
                connects
                    .iter()
                    .find(|c| c.from_sheet == node.id && c.from_cell == format!("{prefix}X"))
                    .map(|c| {
                        serde_json::json!({
                            "sheet": c.to_sheet,
                            "n": c.to_cell.strip_prefix("Connections.X").and_then(|n| n.parse::<usize>().ok()),
                        })
                    })
                    .unwrap_or(serde_json::Value::Null)
            };
            let route = stored_base_route(node).map(|r| r.0.into_iter().map(|p| (p.0 + off.0, p.1 + off.1)).collect()).unwrap_or_else(|| vec![(cells[0], cells[1]), (cells[2], cells[3])]);
            out.push(serde_json::json!({
                "id": node.id,
                "parentId": self.chain(&node.id).and_then(|c| (c.len() > 1).then(|| c[c.len() - 2].id.clone())),
                "route": route.iter().map(|p| [p.0, p.1]).collect::<Vec<_>>(),
                "begin": { "x": cells[0], "y": cells[1], "glue": glue("Begin") },
                "end": { "x": cells[2], "y": cells[3], "glue": glue("End") },
            }));
        }
        serde_json::Value::Array(out)
    }

    /// The ShapeSheet of shape `id` as JSON for a read-only pane: `{id, name, type, master, cells: [{name, value, formula, unit}], sections:
    /// [{name, index, fromMaster, cells: [...], rows: [{name, index, type, cells: [...]}]}]}`. The cells and sections are the shape's own
    /// merged with those its master supplies (what Visio's ShapeSheet window shows); `fromMaster` marks a section the page shape does not
    /// carry itself. Values are the stored `V` text (inches, radians, as in the file), formulas the stored `F`.
    pub fn shape_sheet(&self, id: &str) -> Option<serde_json::Value> {
        let chain = self.chain(id)?;
        let node = chain[chain.len() - 1];
        let cell_json = |name: &str, c: &crate::shapes::Cell| serde_json::json!({"name": name, "value": c.value, "formula": c.formula, "unit": c.unit});
        let cells: Vec<serde_json::Value> = node.cells.iter().map(|(k, c)| cell_json(k, c)).collect();
        let sections: Vec<serde_json::Value> = node
            .sections
            .iter()
            .map(|sec| {
                let own = node.own_sections.iter().any(|o| o.name == sec.name && o.index == sec.index);
                serde_json::json!({
                    "name": sec.name,
                    "index": sec.index,
                    "fromMaster": !own,
                    "cells": sec.cells.iter().map(|(k, c)| cell_json(k, c)).collect::<Vec<_>>(),
                    "rows": sec.rows.iter().map(|r| serde_json::json!({
                        "name": r.name, "index": r.index, "type": r.row_type,
                        "cells": r.cells.iter().map(|(k, c)| cell_json(k, c)).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        Some(serde_json::json!({"id": node.id, "name": node.name, "type": node.shape_type, "master": node.master_name, "cells": cells, "sections": sections}))
    }

    /// Whether the master of shape `id` supplies the Shape Data row `name` (a row the instance can only hide with `Del='1'`).
    pub fn master_supplies_data_row(&self, id: &str, name: &str) -> bool {
        let Some(chain) = self.chain(id) else { return false };
        let node = chain[chain.len() - 1];
        node.master_sections.iter().filter(|s| s.name == "Property").any(|s| s.rows.iter().any(|r| r.name.as_deref() == Some(name)))
    }

    /// A point of shape `id`'s own box (inches, origin bottom-left) on the page.
    fn world_point(&self, id: &str, point: (f64, f64)) -> Option<(f64, f64)> {
        let chain = self.chain(id)?;
        let mut p = point;
        for node in chain.iter().rev() {
            p = frame_of(node).ok()?.to_parent(p);
        }
        // a preview: the shape (or a group it is in) is taken to be moved by the delta
        if let Some((moved, delta)) = &*self.shift.borrow() {
            if chain.iter().any(|node| moved.contains(&node.id)) {
                p = (p.0 + delta.0, p.1 + delta.1);
            }
        }
        Some(p)
    }

    /// The routes (page inches) the connectors glued to the shapes in `moved` would have if those shapes (with everything in them) were moved
    /// by `delta` (page inches, y up): the live preview of a drag. Nothing is changed. A connector inside a moved group, or moved itself, is
    /// left out (it moves with its group); so is one that no moved shape touches. Same router as `reroute_connectors`.
    pub fn preview_connector_routes(&self, connects: &[Connect], moved: &[String], delta: (f64, f64)) -> Vec<(String, Vec<(f64, f64)>)> {
        *self.shift.borrow_mut() = Some((moved.to_vec(), delta));
        let touched = |id: &str| self.chain(id).is_some_and(|chain| chain.iter().any(|node| moved.contains(&node.id)));
        let mut out = Vec::new();
        for (node, off) in self.connector_nodes() {
            if num(node, "Angle").unwrap_or(0.0).abs() > EPS || touched(&node.id) {
                continue;
            }
            let glue = |prefix: &str| {
                connects
                    .iter()
                    .find(|c| c.from_sheet == node.id && c.from_cell == format!("{prefix}X"))
                    .map(|c| (c.to_sheet.as_str(), c.to_cell.strip_prefix("Connections.X").and_then(|n| n.parse::<usize>().ok())))
            };
            let (Some((from_id, from_n)), Some((to_id, to_n))) = (glue("Begin"), glue("End")) else { continue };
            if !touched(from_id) && !touched(to_id) {
                continue;
            }
            let ends = |id: &str, n: Option<usize>| match n {
                Some(n) => self.site(id, n).map(|(site, rect)| (vec![site], rect)),
                None => self.sites_of(id),
            };
            let (Some((from_sites, a)), Some((to_sites, b))) = (ends(from_id, from_n), ends(to_id, to_n)) else { continue };
            let Some((_, _, route, _)) = route_ends(&from_sites, a, &to_sites, b) else { continue };
            let _ = off;
            out.push((node.id.clone(), route));
        }
        *self.shift.borrow_mut() = None;
        out
    }

    /// Page position, outward direction and page box of connection point `n` (1-based, as in
    /// `Connections.X<n>`) of shape `id`.
    fn site(&self, id: &str, n: usize) -> Option<(Site, Rect)> {
        let chain = self.chain(id)?;
        let node = chain[chain.len() - 1];
        let section = node.sections.iter().find(|s| s.name == "Connection")?;
        let row = section.rows.iter().find(|r| r.index.as_deref().and_then(|i| i.parse::<usize>().ok()) == Some(n - 1))
            .or_else(|| section.rows.get(n - 1))?;
        self.site_of_row(id, &chain, row)
    }

    /// The own frame of page-level shape `id` when it can be replaced by another master (`replace::replace_shape`): not a connector, not a member of a group.
    pub(crate) fn replaceable_frame(&self, id: &str) -> Option<Frame> {
        let node = self.roots.iter().find(|n| n.id == id)?;
        if self.connector_nodes().iter().any(|(n, _)| n.id == id) {
            return None;
        }
        frame_of(node).ok()
    }

    /// The rows of shape `id`'s Connection section as the shape resolves them (own rows over inherited ones): the shape's Width and Height and
    /// `(IX, cells)` per row, for `connpoints::add_connection_point`.
    pub(crate) fn connection_rows(&self, id: &str) -> Option<(f64, f64, Vec<(usize, std::collections::BTreeMap<String, crate::shapes::Cell>)>)> {
        let chain = self.chain(id)?;
        let node = chain[chain.len() - 1];
        let (width, height) = (num(node, "Width")?, num(node, "Height")?);
        let rows = node
            .sections
            .iter()
            .find(|s| s.name == "Connection")
            .map(|s| s.rows.iter().enumerate().filter(|(_, r)| !r.deleted).map(|(position, r)| (r.index.as_deref().and_then(|i| i.parse::<usize>().ok()).unwrap_or(position), r.cells.clone())).collect())
            .unwrap_or_default();
        Some((width, height, rows))
    }

    /// The connection points of shape `id` as `(n, x, y)` with `n` as in `Connections.X<n>`
    /// (1-based) and the position in page inches. Empty when the shape defines none.
    pub fn connection_points(&self, id: &str) -> Vec<(usize, f64, f64)> {
        let Some(chain) = self.chain(id) else { return Vec::new() };
        let node = chain[chain.len() - 1];
        let Some(section) = node.sections.iter().find(|s| s.name == "Connection") else { return Vec::new() };
        section
            .rows
            .iter()
            .enumerate()
            .filter_map(|(position, row)| {
                let n = row.index.as_deref().and_then(|i| i.parse::<usize>().ok()).map_or(position + 1, |i| i + 1);
                let (site, _) = self.site_of_row(id, &chain, row)?;
                Some((n, site.point.0, site.point.1))
            })
            .collect()
    }

    /// Where a new right-angle connector between two shape ends runs. An end is a shape id and
    /// `Some(n)` for connection point `n` (1-based) or `None` for the whole shape (Visio then
    /// picks the best side). Uses the same router as `reroute_connectors`, so the stored route is
    /// the one a later reroute would compute.
    pub fn plan_connector(
        &self,
        from: (&str, Option<usize>),
        to: (&str, Option<usize>),
    ) -> Result<ConnectorPlan> {
        if from.0 == to.0 {
            return Err(Error("a connector needs two different shapes".to_string()));
        }
        let ends = |id: &str, n: Option<usize>| -> Result<(Vec<Site>, Rect)> {
            match n {
                Some(n) => self
                    .site(id, n)
                    .map(|(site, rect)| (vec![site], rect))
                    .ok_or_else(|| Error(format!("shape {id} has no connection point {n}"))),
                None => self.sites_of(id).ok_or_else(|| Error(format!("no shape {id}"))),
            }
        };
        let (from_sites, a) = ends(from.0, from.1)?;
        let (to_sites, b) = ends(to.0, to.1)?;
        let (begin, end, route, _) = route_ends(&from_sites, a, &to_sites, b).ok_or_else(|| Error("no route between these shapes".to_string()))?;
        Ok(ConnectorPlan { begin: begin.point, end: end.point, route })
    }

    /// Where a connector end `target` lies when the other end is loose at `toward`: the position, the connection point, or the
    /// point of the shape's connection points nearest to `toward`.
    fn end_point(&self, target: &EndTarget, toward: (f64, f64)) -> Result<(f64, f64)> {
        Ok(match target {
            EndTarget::Free(x, y) => (*x, *y),
            EndTarget::Glue(sheet, Some(n)) => self.site(sheet, *n).map(|s| s.0.point).unwrap_or(toward),
            EndTarget::Glue(sheet, None) => {
                let (sites, _) = self.sites_of(sheet).ok_or_else(|| Error(format!("no shape {sheet}")))?;
                sites
                    .iter()
                    .map(|s| s.point)
                    .min_by(|p, q| {
                        let d = |r: &(f64, f64)| (r.0 - toward.0).powi(2) + (r.1 - toward.1).powi(2);
                        d(p).total_cmp(&d(q))
                    })
                    .unwrap_or(toward)
            }
        })
    }

    /// The box shape `id` covers on the page, `(left, bottom, right, top)` in inches (a turned shape: the box around it).
    pub fn shape_page_box(&self, id: &str) -> Option<(f64, f64, f64, f64)> {
        let chain = self.chain(id)?;
        let frame = frame_of(chain[chain.len() - 1]).ok()?;
        self.page_box(id, &frame).map(|r| (r.x0, r.y0, r.x1, r.y1))
    }

    /// Every connection point of shape `id` (the four side midpoints when it defines none) and
    /// the shape's page box: the choices Visio has when a connector is glued to the whole shape.
    fn sites_of(&self, id: &str) -> Option<(Vec<Site>, Rect)> {
        let chain = self.chain(id)?;
        let node = chain[chain.len() - 1];
        let frame = frame_of(node).ok()?;
        let mut sites = Vec::new();
        let mut rect = None;
        if let Some(section) = node.sections.iter().find(|s| s.name == "Connection") {
            for row in &section.rows {
                if let Some((site, r)) = self.site_of_row(id, &chain, row) {
                    sites.push(site);
                    rect = Some(r);
                }
            }
        }
        if sites.is_empty() {
            let (w, h) = frame.size;
            for (local, dir) in [((w / 2.0, 0.0), (0.0, -1.0)), ((w, h / 2.0), (1.0, 0.0)), ((w / 2.0, h), (0.0, 1.0)), ((0.0, h / 2.0), (-1.0, 0.0))] {
                let (wx, wy) = world_vector(&chain, dir);
                let out = if wx.abs() >= wy.abs() { (wx.signum() as i8, 0) } else { (0, wy.signum() as i8) };
                sites.push(Site { point: self.world_point(id, local)?, out });
            }
        }
        let rect = match rect {
            Some(r) => r,
            None => self.page_box(id, &frame)?,
        };
        Some((sites, rect))
    }

    fn page_box(&self, id: &str, frame: &Frame) -> Option<Rect> {
        let corners = [(0.0, 0.0), (frame.size.0, 0.0), (frame.size.0, frame.size.1), (0.0, frame.size.1)]
            .map(|corner| self.world_point(id, corner));
        let corners: Vec<(f64, f64)> = corners.into_iter().collect::<Option<_>>()?;
        Some(Rect {
            x0: corners.iter().map(|c| c.0).fold(f64::MAX, f64::min),
            y0: corners.iter().map(|c| c.1).fold(f64::MAX, f64::min),
            x1: corners.iter().map(|c| c.0).fold(f64::MIN, f64::max),
            y1: corners.iter().map(|c| c.1).fold(f64::MIN, f64::max),
        })
    }

    fn site_of_row(&self, id: &str, chain: &[&ResolvedShape], row: &crate::shapes::SectionRow) -> Option<(Site, Rect)> {
        let node = chain[chain.len() - 1];
        let frame = frame_of(node).ok()?;
        let cell = |name: &str| row.cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
        // A flipped shape mirrors its connection points inside its box (Visio's FlipX/FlipY): the point at x is
        // where w - x was, and the direction across the flipped axis reverses (probed in Visio: a diamond's right
        // point sits on its left after FlipX, and a glued connector loops around the shape).
        let flip_x = num(node, "FlipX").unwrap_or(0.0) != 0.0;
        let flip_y = num(node, "FlipY").unwrap_or(0.0) != 0.0;
        let (x, y) = (cell("X")?, cell("Y")?);
        let _ = (flip_x, flip_y);
        // Outward direction in the shape's own axes: the row's DirX/DirY point into the shape
        // (the Rectangle master's bottom point has DirY = 1, and Visio's routes leave downward),
        // so negate them; without them use the nearest side of the box.
        let (dir_x, dir_y) = (cell("DirX").unwrap_or(0.0), cell("DirY").unwrap_or(0.0));
        let local = if dir_x.abs() > 0.5 || dir_y.abs() > 0.5 {
            if dir_x.abs() >= dir_y.abs() { (-dir_x.signum(), 0.0) } else { (0.0, -dir_y.signum()) }
        } else {
            let sides = [
                (x, (-1.0, 0.0)),
                (frame.size.0 - x, (1.0, 0.0)),
                (y, (0.0, -1.0)),
                (frame.size.1 - y, (0.0, 1.0)),
            ];
            sides.iter().fold((f64::MAX, (1.0, 0.0)), |best, side| if side.0 < best.0 { *side } else { best }).1
        };
        let (wx, wy) = world_vector(chain, local);
        let out = if wx.abs() >= wy.abs() { (wx.signum() as i8, 0) } else { (0, wy.signum() as i8) };
        let point = self.world_point(id, (x, y))?;
        Some((Site { point, out }, self.page_box(id, &frame)?))
    }

    /// New routes for the page's right-angle connectors that are glued at both ends to
    /// connection points, where a shape has moved so the stored ends no longer match. Connectors
    /// inside groups, rotated connectors and ends glued to whole shapes are left alone.
    pub fn reroute_connectors(&self, connects: &[Connect], jump: &LineJump) -> Vec<ShapeCellPatch> {
        let mut patches = Vec::new();
        // New hop-free routes (page inches) of the connectors rerouted below, with their frame origin.
        let mut rerouted: BTreeMap<String, (Vec<(f64, f64)>, (f64, f64))> = BTreeMap::new();
        for (node, off) in self.connector_nodes() {
            if num(node, "Angle").unwrap_or(0.0).abs() > EPS {
                continue;
            }
            // Where an end is glued: a connection point (`Connections.X<n>`) or the whole shape.
            let glue = |prefix: &str| {
                connects
                    .iter()
                    .find(|c| c.from_sheet == node.id && c.from_cell == format!("{prefix}X"))
                    .map(|c| (c.to_sheet.as_str(), c.to_cell.strip_prefix("Connections.X").and_then(|n| n.parse::<usize>().ok())))
            };
            let (Some((from_id, from_n)), Some((to_id, to_n))) = (glue("Begin"), glue("End")) else { continue };
            let ends = |id: &str, n: Option<usize>| match n {
                Some(n) => self.site(id, n).map(|(site, rect)| (vec![site], rect)),
                None => self.sites_of(id),
            };
            let (Some((from_sites, a)), Some((to_sites, b))) = (ends(from_id, from_n), ends(to_id, to_n)) else { continue };
            let Some((begin, end, route, nearest)) = route_ends(&from_sites, a, &to_sites, b) else { continue };
            // The side router had no answer (members of a turned group) and the nearest sites were taken: that is a guess, so it is only
            // made when an end has really left its shape (a member moved). A connector whose ends still sit on connection points of
            // its shapes - what Visio's own ungroup of a turned group leaves - is kept as it is.
            if nearest {
                let on_site = |end: (f64, f64), sites: &[Site]| sites.iter().any(|s| (s.point.0 - end.0).abs() < 1e-3 && (s.point.1 - end.1).abs() < 1e-3);
                let ends: Option<Vec<f64>> = ["BeginX", "BeginY", "EndX", "EndY"].iter().map(|name| num(node, name)).collect();
                if let Some(e) = ends {
                    if on_site((e[0] + off.0, e[1] + off.1), &from_sites) && on_site((e[2] + off.0, e[3] + off.1), &to_sites) {
                        continue;
                    }
                }
            }
            // Leave connectors alone whose stored route is already this one.
            if let Some((stored, _, _)) = stored_base_route(node) {
                if stored.len() == route.len()
                    && stored.iter().zip(&route).all(|(p, q)| (p.0 + off.0 - q.0).abs() < 1e-4 && (p.1 + off.1 - q.1).abs() < 1e-4)
                {
                    continue;
                }
            }
            if off == (0.0, 0.0) {
                rerouted.insert(node.id.clone(), (route.clone(), begin.point));
            }
            let (w, h) = (end.point.0 - begin.point.0, end.point.1 - begin.point.1);
            let mut rows = Vec::new();
            for (i, p) in route.iter().enumerate() {
                let kind = if i == 0 { "MoveTo" } else { "LineTo" };
                rows.push(
                    GeometryRow::new(kind, (i + 1).to_string())
                        .set("X", fmt(p.0 - begin.point.0))
                        .set("Y", fmt(p.1 - begin.point.1)),
                );
            }
            patches.push(
                ShapeCellPatch::new(node.id.clone())
                    .set_cached("BeginX", fmt(begin.point.0 - off.0))
                    .set_cached("BeginY", fmt(begin.point.1 - off.1))
                    .set_cached("EndX", fmt(end.point.0 - off.0))
                    .set_cached("EndY", fmt(end.point.1 - off.1))
                    .set_cached("PinX", fmt(begin.point.0 + w / 2.0 - off.0))
                    .set_cached("PinY", fmt(begin.point.1 + h / 2.0 - off.1))
                    .set_cached("Width", fmt(w))
                    .set_cached("Height", fmt(h))
                    .set_cached("LocPinX", fmt(w / 2.0))
                    .set_cached("LocPinY", fmt(h / 2.0))
                    .geometry_rows("0", rows),
            );
        }
        self.refresh_hops(&mut patches, &rerouted, jump);
        patches
    }

    /// Redraw the hops of every connector whose route changed, whose old route had hops, or that
    /// crosses a rerouted one, so that hops follow the connectors that moved. Writes only
    /// connectors whose geometry actually changes.
    fn refresh_hops(
        &self,
        patches: &mut Vec<ShapeCellPatch>,
        rerouted: &BTreeMap<String, (Vec<(f64, f64)>, (f64, f64))>,
        jump: &LineJump,
    ) {
        if rerouted.is_empty() {
            return;
        }
        // Hop-free routes of all page-level straight connectors.
        struct Entry<'a> {
            id: &'a str,
            node: &'a ResolvedShape,
            route: Vec<(f64, f64)>,
            origin: (f64, f64),
            stored_hops: bool,
        }
        let mut entries: Vec<Entry> = Vec::new();
        for node in &self.roots {
            if ["BeginX", "BeginY", "EndX", "EndY"].iter().any(|name| num(node, name).is_none())
                || num(node, "Angle").unwrap_or(0.0).abs() > EPS
            {
                continue;
            }
            if let Some((route, origin)) = rerouted.get(&node.id) {
                entries.push(Entry { id: &node.id, node, route: route.clone(), origin: *origin, stored_hops: false });
            } else if let Some((route, origin, hops)) = stored_base_route(node) {
                entries.push(Entry { id: &node.id, node, route, origin, stored_hops: hops });
            }
        }
        let hops = compute_hops(&entries.iter().map(|e| e.route.clone()).collect::<Vec<_>>(), jump);
        for (index, entry) in entries.iter().enumerate() {
            let crosses_rerouted = hops[index].iter().any(|h| rerouted.contains_key(entries[h.other].id))
                || entries.iter().enumerate().any(|(j, other)| {
                    rerouted.contains_key(other.id) && hops[j].iter().any(|h| h.other == index)
                });
            let is_rerouted = rerouted.contains_key(entry.id);
            if !(is_rerouted || entry.stored_hops || crosses_rerouted) {
                continue;
            }
            let rows = hop_rows(&entry.route, &hops[index], entry.origin, jump);
            let existing = patches.iter().position(|p| p.visio_id == entry.id);
            if is_rerouted {
                if let Some(at) = existing {
                    patches[at].geometry_rows = Some(("0".to_string(), rows));
                }
            } else if !same_rows(entry.node, &rows) {
                patches.push(ShapeCellPatch::new(entry.id.to_string()).geometry_rows("0", rows));
            }
        }
    }

    /// Ancestor chain `[root, ..., node]` for a Visio shape ID.
    fn chain(&self, id: &str) -> Option<Vec<&ResolvedShape>> {
        fn walk<'a>(nodes: &'a [ResolvedShape], id: &str, path: &mut Vec<&'a ResolvedShape>) -> bool {
            for node in nodes {
                path.push(node);
                if node.id == id || walk(&node.children, id, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        walk(&self.roots, id, &mut path).then_some(path)
    }

    /// Resize group `id` to `new_size` (its own width/height, i.e. along the group's
    /// rotated axes), keeping the box point at `anchor` fixed in the page. Members
    /// scale with the box, in the group's own axes.
    pub fn resize_group(&self, id: &str, new_size: (f64, f64), anchor: (f64, f64)) -> Result<Vec<ShapeCellPatch>> {
        let chain = self.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
        let group = chain[chain.len() - 1];
        if group.shape_type.as_deref() != Some("Group") {
            return Err(Error(format!("shape {id} is not a group")));
        }
        let old = frame_of(group)?;
        let new = resize_frame(&old, new_size, anchor);
        let scale = (
            if old.size.0.abs() > EPS { new.size.0 / old.size.0 } else { 1.0 },
            if old.size.1.abs() > EPS { new.size.1 / old.size.1 } else { 1.0 },
        );
        let mut patches = vec![frame_patch(id, &new)];
        scale_end_points(group, &old, &new, scale, &mut patches[0]);
        scale_children(group, scale, &mut patches)?;
        Ok(patches)
    }

    /// Cell patches that apply `edit` to each shape of `ids` and to everything inside it, the way
    /// Visio's Format commands treat a group: the cells are written on the page-level shapes
    /// (dropping any inherited formula, so the value is a local override). Choosing a colour for a
    /// shape whose fill or line is switched off (pattern 0) switches it on again; text size goes to
    /// every `Character` row of the shapes that carry text.
    pub fn style_patches(&self, ids: &[String], edit: &StyleEdit) -> Result<Vec<ShapeCellPatch>> {
        let fill = edit.fill.as_deref().map(|f| if f == "none" { Ok(None) } else { hex_colour(f).map(Some) }).transpose()?;
        let line = edit.line.as_deref().map(|l| if l == "none" { Ok(None) } else { hex_colour(l).map(Some) }).transpose()?;
        if let Some(w) = edit.weight_px {
            if !w.is_finite() || w < 0.0 {
                return Err(Error("the line weight must be a number of at least 0".to_string()));
            }
        }
        if let Some(p) = edit.pattern {
            if p > 23 {
                return Err(Error(format!("line pattern {p} is not between 0 and 23")));
            }
        }
        if let Some(f) = edit.font_pt {
            if !f.is_finite() || !(1.0..=1000.0).contains(&f) {
                return Err(Error("the font size must be between 1 and 1000 points".to_string()));
            }
        }
        for (name, value) in [("fill transparency", edit.fill_trans), ("line transparency", edit.line_trans)] {
            if let Some(t) = value {
                if !t.is_finite() || !(0.0..=1.0).contains(&t) {
                    return Err(Error(format!("the {name} must be between 0 and 1")));
                }
            }
        }
        for (name, value, max) in [
            ("begin arrow", edit.begin_arrow, 45),
            ("end arrow", edit.end_arrow, 45),
            ("begin arrow size", edit.begin_arrow_size, 6),
            ("end arrow size", edit.end_arrow_size, 6),
        ] {
            if value.is_some_and(|v| v > max) {
                return Err(Error(format!("the {name} must be between 0 and {max}")));
            }
        }
        let font_family = match edit.font_family.as_deref().map(str::trim) {
            None => None,
            Some(name) if !name.is_empty() && name.chars().count() <= 64 && !name.chars().any(|c| c.is_control() || "<>&'\"\\".contains(c)) => Some(name.to_string()),
            Some(name) => return Err(Error(format!("{name:?} is not a usable font name"))),
        };
        let text_color = edit.text_color.as_deref().map(hex_colour).transpose()?;
        let fill_back = edit.fill_back.as_deref().map(hex_colour).transpose()?;
        let shadow_colour = edit.shadow_color.as_deref().map(hex_colour).transpose()?;
        if edit.fill_pattern.is_some_and(|p| p > 40) {
            return Err(Error("the fill pattern must be between 0 and 40".to_string()));
        }
        let gradient: Option<Vec<(f64, String, f64)>> = match &edit.gradient_stops {
            None => None,
            Some(stops) => {
                if !(2..=8).contains(&stops.len()) {
                    return Err(Error("a gradient needs between 2 and 8 stops".to_string()));
                }
                let mut out = Vec::new();
                for (pos, colour, trans) in stops {
                    if !pos.is_finite() || !(0.0..=1.0).contains(pos) || !trans.is_finite() || !(0.0..=1.0).contains(trans) {
                        return Err(Error("a gradient stop position and transparency must be between 0 and 1".to_string()));
                    }
                    out.push((*pos, hex_colour(colour)?, *trans));
                }
                out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                Some(out)
            }
        };
        for (name, value) in [("glow size", edit.glow_px), ("soft edges size", edit.soft_edges_px), ("bevel size", edit.bevel_px)] {
            if value.is_some_and(|v| !v.is_finite() || !(0.0..=200.0).contains(&v)) {
                return Err(Error(format!("the {name} must be between 0 and 200 pixels")));
            }
        }
        if edit.glow_trans.is_some_and(|t| !t.is_finite() || !(0.0..=1.0).contains(&t)) {
            return Err(Error("the glow transparency must be between 0 and 1".to_string()));
        }
        if edit.bevel.is_some_and(|b| b > 12) {
            return Err(Error("the bevel type must be between 0 and 12".to_string()));
        }
        let glow_colour = edit.glow_color.as_deref().map(hex_colour).transpose()?;
        if edit.gradient_angle_deg.is_some_and(|a| !a.is_finite()) || edit.gradient_dir.is_some_and(|d| d > 15) {
            return Err(Error("the gradient angle must be a number and the gradient type between 0 and 15".to_string()));
        }
        if edit.line_cap.is_some_and(|cap| cap > 2) {
            return Err(Error("the line cap must be 0 (round), 1 (flat) or 2 (square)".to_string()));
        }
        for (name, value) in [("shadow offset x", edit.shadow_dx), ("shadow offset y", edit.shadow_dy), ("shadow blur", edit.shadow_blur), ("rounding", edit.rounding_px), ("shadow transparency", edit.shadow_trans)] {
            if value.is_some_and(|v| !v.is_finite()) {
                return Err(Error(format!("the {name} must be a number")));
            }
        }
        if edit.shadow_blur.is_some_and(|v| v < 0.0) || edit.rounding_px.is_some_and(|v| v < 0.0) {
            return Err(Error("the shadow blur and the rounding cannot be negative".to_string()));
        }
        if edit.shadow_trans.is_some_and(|t| !(0.0..=1.0).contains(&t)) {
            return Err(Error("the shadow transparency must be between 0 and 1".to_string()));
        }
        let (mut style_set, mut style_clear) = (0u32, 0u32);
        for (wanted, bit) in [(edit.bold, 1u32), (edit.italic, 2), (edit.underline, 4)] {
            match wanted {
                Some(true) => style_set |= bit,
                Some(false) => style_clear |= bit,
                None => {}
            }
        }
        fn walk<'a>(node: &'a ResolvedShape, out: &mut Vec<&'a ResolvedShape>) {
            out.push(node);
            for child in &node.children {
                walk(child, out);
            }
        }
        let wants_line = matches!(&line, Some(Some(_)))
            || edit.weight_px.is_some()
            || edit.pattern.is_some_and(|p| p >= 1);
        let wants_fill = matches!(&fill, Some(Some(_))) || edit.fill_pattern.is_some_and(|p| p >= 1);
        let mut patches = Vec::new();
        for id in ids {
            let chain = self.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
            let mut nodes: Vec<&ResolvedShape> = Vec::new();
            walk(chain[chain.len() - 1], &mut nodes);
            // A geometry section with NoLine / NoFill = 1 hides the line / fill whatever the cells
            // say (shapes exported by Lucidchart are fill-only; the flag often lives in the master, so the
            // merged sections are read and an override is written on the page shape). When nothing of the shape draws a
            // line (fill) and the edit asks for one, those flags are switched off, otherwise the
            // chosen colour would never show.
            let sections = |flag: &str| -> Vec<(usize, String, bool)> {
                nodes
                    .iter()
                    .enumerate()
                    .flat_map(|(i, n)| {
                        n.sections
                            .iter()
                            .filter(|s| s.name == "Geometry")
                            .filter_map(|s| {
                                s.cells.get(flag).and_then(|c| c.value.as_deref()).map(|v| (i, s.index.clone().unwrap_or_default(), v.trim() == "1"))
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect()
            };
            let line_flags = sections("NoLine");
            let fill_flags = sections("NoFill");
            let clear_no_line = wants_line && !line_flags.is_empty() && line_flags.iter().all(|f| f.2);
            let clear_no_fill = wants_fill && !fill_flags.is_empty() && fill_flags.iter().all(|f| f.2);
            for (i, node) in nodes.iter().enumerate() {
                let mut patch = ShapeCellPatch::new(node.id.clone());
                let mut any = false;
                match &fill {
                    Some(Some(colour)) => {
                        // A transparent fill (FillForegndTrans > 0) would hide the chosen colour.
                        patch = patch.set("FillForegnd", colour.clone()).set("FillForegndTrans", "0");
                        if num(node, "FillPattern") == Some(0.0) {
                            patch = patch.set("FillPattern", "1");
                        }
                        any = true;
                    }
                    Some(None) => {
                        patch = patch.set("FillPattern", "0");
                        any = true;
                    }
                    None => {}
                }
                match &line {
                    Some(Some(colour)) => {
                        // LineColorTrans 1 is a fully transparent line (the Lucidchart masters have
                        // it), so a chosen colour makes the line opaque again.
                        patch = patch.set("LineColor", colour.clone()).set("LineColorTrans", "0");
                        if num(node, "LinePattern") == Some(0.0) && edit.pattern.is_none() {
                            patch = patch.set("LinePattern", "1");
                        }
                        any = true;
                    }
                    Some(None) => {
                        patch = patch.set("LinePattern", "0");
                        any = true;
                    }
                    None => {}
                }
                let transparent_line = num(node, "LineColorTrans").is_some_and(|t| t > 0.0);
                if let Some(w) = edit.weight_px {
                    patch = patch.set("LineWeight", fmt(w / 96.0));
                    if transparent_line && !matches!(&line, Some(Some(_))) {
                        patch = patch.set("LineColorTrans", "0");
                    }
                    any = true;
                }
                if let Some(p) = edit.pattern {
                    patch = patch.set("LinePattern", p.to_string());
                    if p >= 1 && transparent_line && !matches!(&line, Some(Some(_))) {
                        patch = patch.set("LineColorTrans", "0");
                    }
                    any = true;
                }
                if let Some(pt) = edit.font_pt {
                    if node.has_text || node.text.is_some() {
                        patch = patch.char_size(pt / 72.0);
                        any = true;
                    }
                }
                if let Some(t) = edit.fill_trans {
                    patch = patch.set("FillForegndTrans", fmt(t));
                    any = true;
                }
                if let Some(t) = edit.line_trans {
                    patch = patch.set("LineColorTrans", fmt(t));
                    any = true;
                }
                if let Some(p) = edit.fill_pattern {
                    patch = patch.set("FillPattern", p.to_string());
                    any = true;
                }
                if let Some(px) = edit.glow_px {
                    patch = patch.set("GlowSize", fmt(px / 96.0));
                    if px > 0.0 {
                        patch = patch.set("GlowColor", glow_colour.clone().unwrap_or_else(|| "#ffc000".to_string())).set("GlowColorTrans", fmt(edit.glow_trans.unwrap_or(0.4)));
                    }
                    any = true;
                } else if glow_colour.is_some() || edit.glow_trans.is_some() {
                    if let Some(c) = &glow_colour {
                        patch = patch.set("GlowColor", c.clone());
                    }
                    if let Some(t) = edit.glow_trans {
                        patch = patch.set("GlowColorTrans", fmt(t));
                    }
                    any = true;
                }
                if let Some(px) = edit.soft_edges_px {
                    patch = patch.set("SoftEdgesSize", fmt(px / 96.0));
                    any = true;
                }
                match edit.reflection {
                    Some(true) => {
                        patch = patch.set("ReflectionSize", "0.5").set("ReflectionDist", "0").set("ReflectionBlur", "0.0278").set("ReflectionTrans", "0.3");
                        any = true;
                    }
                    Some(false) => {
                        patch = patch.set("ReflectionSize", "0");
                        any = true;
                    }
                    None => {}
                }
                if let Some(kind) = edit.bevel {
                    patch = patch.set("BevelTopType", kind.to_string());
                    if kind > 0 {
                        let size = fmt(edit.bevel_px.unwrap_or(8.0) / 96.0);
                        patch = patch.set("BevelTopWidth", size.clone()).set("BevelTopHeight", size);
                    }
                    any = true;
                }
                if edit.gradient_enabled == Some(false) && gradient.is_none() {
                    patch = patch.set("FillGradientEnabled", "0");
                    if num(node, "FillPattern").is_some_and(|p| p >= 25.0) {
                        patch = patch.set("FillPattern", "1");
                    }
                    any = true;
                }
                if let Some(stops) = &gradient {
                    patch = patch.set("FillGradientEnabled", "1");
                    if let Some(dir) = edit.gradient_dir {
                        patch = patch.set("FillGradientDir", dir.to_string());
                    }
                    if num(node, "FillPattern").map_or(true, |p| p < 25.0) && edit.fill_pattern.is_none() {
                        patch = patch.set("FillPattern", "30");
                    }
                    if let Some(angle) = edit.gradient_angle_deg {
                        patch = patch.set("FillGradientAngle", fmt(angle.to_radians()));
                    }
                    let existing = node.sections.iter().find(|s| s.name == "FillGradient").map_or(0, |s| s.rows.len());
                    for index in 0..stops.len().max(existing) {
                        // rows the shape had beyond the new stops repeat the last stop (a row cannot be dropped from here)
                        let (pos, colour, trans) = stops.get(index).unwrap_or_else(|| stops.last().unwrap());
                        let pos = if index < stops.len() { *pos } else { 1.0 };
                        patch = patch.row_override(crate::writer::RowOverride {
                            section: "FillGradient".to_string(),
                            section_index: String::new(),
                            row_index: index.to_string(),
                            row_type: None,
                            cells: vec![
                                ("GradientStopColor".to_string(), colour.clone(), String::new()),
                                ("GradientStopColorTrans".to_string(), fmt(*trans), String::new()),
                                ("GradientStopPosition".to_string(), fmt(pos), String::new()),
                            ],
                        });
                    }
                    any = true;
                }
                if let Some(back) = &fill_back {
                    patch = patch.set("FillBkgnd", back.clone()).set("FillBkgndTrans", "0");
                    any = true;
                }
                if let Some(cap) = edit.line_cap {
                    patch = patch.set("LineCap", cap.to_string());
                    any = true;
                }
                if let Some(r) = edit.rounding_px {
                    patch = patch.set("Rounding", fmt(r / 96.0));
                    any = true;
                }
                if let Some(lock) = edit.lock {
                    let v = if lock { "1" } else { "0" };
                    for cell in ["LockMoveX", "LockMoveY", "LockWidth", "LockHeight", "LockRotate", "LockDelete"] {
                        patch = patch.set(cell, v);
                    }
                    any = true;
                }
                match edit.shadow {
                    // The cells Visio draws a visible outer shadow from (probe-shadow-cells.ps1 + shadow.py, Visio 16.0.14332: with these cells the exported page differs from a plain rectangle only to the right of and below it, in grey 153 = black at 60 % transparency; with ShdwPattern 0 the image is identical; see also make-shadow-types.ps1): pattern 1, type 1,
                    // an offset down and to the right, a little blur and a 60 % transparent black.
                    Some(true) => {
                        patch = patch
                            .set("ShdwPattern", "1")
                            .set("ShapeShdwType", "1")
                            .set("ShapeShdwOffsetX", "0.0417")
                            .set("ShapeShdwOffsetY", "-0.0417")
                            .set("ShapeShdwBlur", "0.0417")
                            .set("ShdwForegnd", "#000000")
                            .set("ShdwForegndTrans", "0.6");
                        any = true;
                    }
                    Some(false) => {
                        patch = patch.set("ShdwPattern", "0");
                        any = true;
                    }
                    None => {}
                }
                let shadow_detail = shadow_colour.is_some() || edit.shadow_dx.is_some() || edit.shadow_dy.is_some() || edit.shadow_blur.is_some() || edit.shadow_trans.is_some() || edit.shadow_type.is_some() || edit.shadow_scale.is_some() || edit.shadow_angle.is_some();
                if shadow_detail && edit.shadow != Some(false) {
                    patch = patch.set("ShdwPattern", "1").set("ShapeShdwType", edit.shadow_type.filter(|t| (1..=3).contains(t)).unwrap_or(1).to_string());
                    if let Some(scale) = edit.shadow_scale.filter(|s| *s > 0.0) {
                        patch = patch.set("ShapeShdwScaleFactor", fmt(scale));
                    }
                    if let Some(angle) = edit.shadow_angle {
                        patch = patch.set("ShapeShdwObliqueAngle", fmt(angle.to_radians()));
                    }
                    if let Some(colour) = &shadow_colour {
                        patch = patch.set("ShdwForegnd", colour.clone());
                    }
                    if let Some(dx) = edit.shadow_dx {
                        patch = patch.set("ShapeShdwOffsetX", fmt(dx / 96.0));
                    }
                    if let Some(dy) = edit.shadow_dy {
                        patch = patch.set("ShapeShdwOffsetY", fmt(-dy / 96.0));
                    }
                    if let Some(blur) = edit.shadow_blur {
                        patch = patch.set("ShapeShdwBlur", fmt(blur / 96.0));
                    }
                    if let Some(t) = edit.shadow_trans {
                        patch = patch.set("ShdwForegndTrans", fmt(t));
                    }
                    any = true;
                }
                for (cell, value) in [
                    ("BeginArrow", edit.begin_arrow),
                    ("EndArrow", edit.end_arrow),
                    ("BeginArrowSize", edit.begin_arrow_size),
                    ("EndArrowSize", edit.end_arrow_size),
                ] {
                    if let Some(v) = value {
                        patch = patch.set(cell, v.to_string());
                        any = true;
                    }
                }
                if node.has_text || node.text.is_some() {
                    if let Some(face) = &font_family {
                        patch = patch.char_cell("Font", face.clone());
                        any = true;
                    }
                    if let Some(colour) = &text_color {
                        patch = patch.char_cell("Color", colour.clone());
                        any = true;
                    }
                    if style_set | style_clear != 0 {
                        patch = patch.char_style(style_set, style_clear);
                        any = true;
                    }
                }
                if clear_no_line {
                    for (_, index, _) in line_flags.iter().filter(|f| f.0 == i) {
                        patch = patch.geometry_flag(index.clone(), "NoLine", "0");
                        any = true;
                    }
                }
                if clear_no_fill {
                    for (_, index, _) in fill_flags.iter().filter(|f| f.0 == i) {
                        patch = patch.geometry_flag(index.clone(), "NoFill", "0");
                        any = true;
                    }
                }
                if any {
                    patches.push(patch);
                }
            }
        }
        Ok(patches)
    }

    /// What the format panel shows for shape `id` besides the resolved colours: the line pattern,
    /// the line weight in inches, the fill pattern and the first character size in points; each is
    /// `null` when the shape (or its master) does not state it.
    pub fn style_info(&self, id: &str) -> Result<serde_json::Value> {
        let chain = self.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
        let node = chain[chain.len() - 1];
        let font_pt = node
            .sections
            .iter()
            .find(|s| s.name == "Character")
            .and_then(|s| s.rows.first())
            .and_then(|row| row.cells.get("Size"))
            .and_then(|c| c.value.as_deref())
            .and_then(|v| v.parse::<f64>().ok())
            .map(|inches| inches * 72.0);
        let first_row = node.sections.iter().find(|s| s.name == "Character").and_then(|s| s.rows.first());
        let row_cell = |name: &str| first_row.and_then(|row| row.cells.get(name)).and_then(|c| c.value.as_deref());
        let style_bits = row_cell("Style").and_then(|v| v.trim().parse::<f64>().ok()).map(|v| v.round() as u32);
        // the custom gradient stops (rows with literal colours only) for the gradient editor
        let gradient_stops: Vec<serde_json::Value> = node
            .sections
            .iter()
            .find(|s| s.name == "FillGradient")
            .map(|section| {
                section
                    .rows
                    .iter()
                    .filter(|row| !row.deleted)
                    .enumerate()
                    .filter_map(|(i, row)| {
                        let cell = |name: &str| row.cells.get(name).and_then(|c| c.value.as_deref());
                        let colour = cell("GradientStopColor").filter(|v| v.starts_with('#') && v.len() == 7)?;
                        let fallback = if section.rows.len() > 1 { i as f64 / (section.rows.len() - 1) as f64 } else { 0.0 };
                        let position = cell("GradientStopPosition").and_then(|v| v.parse::<f64>().ok()).unwrap_or(fallback);
                        let trans = cell("GradientStopColorTrans").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                        Some(serde_json::json!([position, colour, trans]))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(serde_json::json!({
            "gradient": {
                "enabled": num(node, "FillGradientEnabled").map(|v| v != 0.0),
                "dir": num(node, "FillGradientDir"),
                "angleDeg": num(node, "FillGradientAngle").map(|r| r.to_degrees()),
                "stops": gradient_stops,
            },
            "bold": style_bits.map(|b| b & 1 != 0),
            "italic": style_bits.map(|b| b & 2 != 0),
            "underline": style_bits.map(|b| b & 4 != 0),
            "fontFamily": row_cell("Font").filter(|v| !v.is_empty() && *v != "Themed"),
            "fillTrans": num(node, "FillForegndTrans"),
            "lineTrans": num(node, "LineColorTrans"),
            "shadow": num(node, "ShdwPattern").map(|p| p > 0.0),
            "beginArrow": num(node, "BeginArrow"),
            "endArrow": num(node, "EndArrow"),
            "beginArrowSize": num(node, "BeginArrowSize"),
            "endArrowSize": num(node, "EndArrowSize"),
            "linePattern": num(node, "LinePattern"),
            "lineWeight": num(node, "LineWeight"),
            "fillPattern": num(node, "FillPattern"),
            "glow": num(node, "GlowSize").map(|v| v * 96.0),
            "softEdges": num(node, "SoftEdgesSize").map(|v| v * 96.0),
            "reflection": num(node, "ReflectionSize").map(|v| v > 0.001),
            "bevel": num(node, "BevelTopType"),
            "lineCap": num(node, "LineCap"),
            "rounding": num(node, "Rounding"),
            "fontPt": font_pt,
            "hasText": node.has_text || node.text.is_some(),
        }))
    }

    /// Whether shape `id` can be copied: it must sit directly on the page (a member of a group
    /// has coordinates relative to the group) and must not be a connector (its ends are glued to
    /// other shapes).
    pub fn check_copyable(&self, id: &str) -> Result<()> {
        let node = self
            .roots
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| Error(format!("shape {id} is not a shape of the page itself (only those can be copied)")))?;
        if ["BeginX", "EndX"].iter().all(|name| num(node, name).is_some()) {
            return Err(Error(format!("shape {id} is a connector; connectors cannot be copied yet")));
        }
        Ok(())
    }

    /// The ids of shape `id` and everything inside it.
    pub fn subtree_ids(&self, id: &str) -> Result<Vec<String>> {
        fn walk(node: &ResolvedShape, out: &mut Vec<String>) {
            out.push(node.id.clone());
            for child in &node.children {
                walk(child, out);
            }
        }
        let chain = self.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
        let mut ids = Vec::new();
        walk(chain[chain.len() - 1], &mut ids);
        Ok(ids)
    }

    /// The shape that owns the text shown for `id` and that text: `id` itself when it has text of
    /// its own, else the first member (depth first) with some, else `id` with an empty text.
    pub fn text_owner(&self, package: &Package, page_index: usize, id: &str) -> Result<(String, String)> {
        for one in self.subtree_ids(id)? {
            if let Some(text) = crate::writer::own_text(package, page_index, &one)? {
                if !text.is_empty() {
                    return Ok((one, text));
                }
            }
        }
        Ok((id.to_string(), String::new()))
    }

    /// What deleting `removed` shapes does to the connectors that stay: like Visio, a connector end
    /// glued to a removed shape stays where it is and its glue formula is replaced by the
    /// current value (the matching `Connects` rows are dropped by the writer).
    pub fn detach_patches(&self, removed: &[String], connects: &[Connect]) -> Vec<ShapeCellPatch> {
        let mut patches = Vec::new();
        for node in &self.roots {
            if removed.contains(&node.id) {
                continue;
            }
            let mut patch = ShapeCellPatch::new(node.id.clone());
            let mut any = false;
            for prefix in ["Begin", "End"] {
                let glued_to_removed = connects.iter().any(|c| {
                    c.from_sheet == node.id && c.from_cell == format!("{prefix}X") && removed.contains(&c.to_sheet)
                });
                if !glued_to_removed {
                    continue;
                }
                for axis in ["X", "Y"] {
                    let name = format!("{prefix}{axis}");
                    if let Some(value) = num(node, &name) {
                        patch = patch.set(name, fmt(value));
                        any = true;
                    }
                }
            }
            if any {
                patches.push(patch);
            }
        }
        patches
    }

    /// The page-level, non-connector shapes `ids` with their frames (the shapes several-shape turn
    /// and flip commands work on). Members of groups and connectors are refused.
    fn page_level_frames(&self, ids: &[String]) -> Result<Vec<(&ResolvedShape, Frame)>> {
        if ids.is_empty() {
            return Err(Error("nothing selected".to_string()));
        }
        let mut out = Vec::new();
        for id in ids {
            let node = self
                .roots
                .iter()
                .find(|n| n.id == *id)
                .ok_or_else(|| Error(format!("shape {id} is not a shape of the page itself (members of groups cannot be turned or flipped together)")))?;
            if ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|name| num(node, name).is_some()) {
                return Err(Error(format!("shape {id} is a connector; its ends follow the shapes it is glued to")));
            }
            out.push((node, frame_ignoring_flip(node)?));
        }
        Ok(out)
    }

    /// Centre of the page box that holds every given frame.
    fn bounds_centre(frames: &[(&ResolvedShape, Frame)]) -> (f64, f64) {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (_, frame) in frames {
            for (x, y) in frame.corners() {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
    }

    /// Turn page-level shapes by `delta` radians (counter-clockwise): one shape turns about its pin
    /// (only its `Angle` changes, like the rotation handle); several turn as one body about the
    /// centre of their joint box (each pin goes round that centre and each `Angle` grows by `delta`).
    pub fn rotate_many(&self, ids: &[String], delta: f64) -> Result<Vec<ShapeCellPatch>> {
        if !delta.is_finite() {
            return Err(Error("the angle is not a number".to_string()));
        }
        let frames = self.page_level_frames(ids)?;
        let turn = std::f64::consts::TAU;
        let centre = Self::bounds_centre(&frames);
        let (s, c) = delta.sin_cos();
        let mut patches = Vec::new();
        for (node, frame) in &frames {
            let angle = (frame.angle + delta).rem_euclid(turn);
            let angle = if turn - angle < 1e-12 { 0.0 } else { angle };
            let mut patch = ShapeCellPatch::new(node.id.clone()).set("Angle", fmt(angle));
            if frames.len() > 1 {
                let (dx, dy) = (frame.pin.0 - centre.0, frame.pin.1 - centre.1);
                patch = patch.set("PinX", fmt(centre.0 + c * dx - s * dy)).set("PinY", fmt(centre.1 + s * dx + c * dy));
            }
            patches.push(patch);
        }
        Ok(patches)
    }

    /// Page box (`x0, y0, x1, y1`, inches, y up) of a page-level shape: the box of its (turned) frame.
    fn page_extent(frame: &Frame) -> (f64, f64, f64, f64) {
        let corners = frame.corners();
        (
            corners.iter().map(|c| c.0).fold(f64::MAX, f64::min),
            corners.iter().map(|c| c.1).fold(f64::MAX, f64::min),
            corners.iter().map(|c| c.0).fold(f64::MIN, f64::max),
            corners.iter().map(|c| c.1).fold(f64::MIN, f64::max),
        )
    }

    /// Align page-level shapes to the FIRST of `ids` (what Visio does: its `Selection.Align` keeps the first shape selected
    /// where it is; probe-align.ps1): `mode` 0 left edges, 1 centres, 2 right edges, 3 tops, 4 middles, 5 bottoms. Turned
    /// shapes count by the box they cover on the page; only the pins change.
    pub fn align_many(&self, ids: &[String], mode: u32) -> Result<Vec<ShapeCellPatch>> {
        if ids.len() < 2 {
            return Err(Error("select at least two shapes to align them".to_string()));
        }
        if mode > 5 {
            return Err(Error(format!("unknown alignment {mode}")));
        }
        let frames = self.page_level_frames(ids)?;
        let reference = Self::page_extent(&frames[0].1);
        let mut patches = Vec::new();
        for (node, frame) in frames.iter().skip(1) {
            let b = Self::page_extent(frame);
            let (dx, dy) = match mode {
                0 => (reference.0 - b.0, 0.0),
                1 => ((reference.0 + reference.2) / 2.0 - (b.0 + b.2) / 2.0, 0.0),
                2 => (reference.2 - b.2, 0.0),
                3 => (0.0, reference.3 - b.3),
                4 => (0.0, (reference.1 + reference.3) / 2.0 - (b.1 + b.3) / 2.0),
                _ => (0.0, reference.1 - b.1),
            };
            if dx.abs() > EPS || dy.abs() > EPS {
                patches.push(ShapeCellPatch::new(node.id.clone()).set("PinX", fmt(frame.pin.0 + dx)).set("PinY", fmt(frame.pin.1 + dy)));
            }
        }
        Ok(patches)
    }

    /// Space page-level shapes evenly (Visio's Distribute horizontally / vertically): in the order of their centres, the
    /// first and the last stay, the gaps between the boxes between them become equal (a negative gap when the boxes
    /// overlap; probe-align.ps1).
    pub fn distribute_many(&self, ids: &[String], vertical: bool) -> Result<Vec<ShapeCellPatch>> {
        if ids.len() < 3 {
            return Err(Error("select at least three shapes to distribute them".to_string()));
        }
        let frames = self.page_level_frames(ids)?;
        let mut items: Vec<(usize, f64, f64)> = frames
            .iter()
            .enumerate()
            .map(|(i, (_, frame))| {
                let b = Self::page_extent(frame);
                if vertical { (i, b.1, b.3) } else { (i, b.0, b.2) }
            })
            .collect();
        items.sort_by(|a, b| ((a.1 + a.2) / 2.0).total_cmp(&((b.1 + b.2) / 2.0)));
        let total: f64 = items.iter().map(|(_, lo, hi)| hi - lo).sum();
        let span = items[items.len() - 1].2 - items[0].1;
        let gap = (span - total) / (items.len() - 1) as f64;
        let mut patches = Vec::new();
        let mut edge = items[0].2;
        for (i, lo, hi) in items.iter().skip(1).take(items.len() - 2) {
            let target = edge + gap;
            let delta = target - lo;
            edge = hi + delta;
            if delta.abs() > EPS {
                let (node, frame) = &frames[*i];
                let patch = ShapeCellPatch::new(node.id.clone());
                patches.push(if vertical { patch.set("PinY", fmt(frame.pin.1 + delta)) } else { patch.set("PinX", fmt(frame.pin.0 + delta)) });
            }
        }
        Ok(patches)
    }

    /// Put the shapes on layer `layer` (`member`) or take them off it: their `LayerMember` cell, the layer numbers separated by
    /// semicolons (`0;2`), without the others being touched. Any shape, also members of groups.
    pub fn set_layer_membership(&self, ids: &[String], layer: u32, member: bool) -> Result<Vec<ShapeCellPatch>> {
        if ids.is_empty() {
            return Err(Error("nothing selected".to_string()));
        }
        let mut patches = Vec::new();
        for id in ids {
            let chain = self.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
            let node = chain[chain.len() - 1];
            let mut layers: Vec<u32> = node
                .cells
                .get("LayerMember")
                .and_then(|c| c.value.as_deref())
                .map(|v| v.split(';').filter_map(|n| n.trim().parse::<u32>().ok()).collect())
                .unwrap_or_default();
            let has = layers.contains(&layer);
            if member && !has {
                layers.push(layer);
            } else if !member && has {
                layers.retain(|l| *l != layer);
            } else {
                continue;
            }
            layers.sort_unstable();
            let value = layers.iter().map(u32::to_string).collect::<Vec<_>>().join(";");
            patches.push(ShapeCellPatch::new(id.clone()).set("LayerMember", value));
        }
        Ok(patches)
    }

    /// Mirror page-level shapes about the vertical axis (or, with `vertical`, the horizontal axis)
    /// through the centre of their joint box: every shape's box goes to its mirror position, its
    /// `FlipX` (`FlipY`) toggles and its `Angle` becomes the negative (a mirror image of a turned
    /// shape is turned the other way). The text is not mirrored (the importer keeps it upright, as
    /// Visio does). A group is flipped by its own flip cell; its members are not touched.
    pub fn flip_many(&self, ids: &[String], vertical: bool) -> Result<Vec<ShapeCellPatch>> {
        let frames = self.page_level_frames(ids)?;
        let (cx, cy) = Self::bounds_centre(&frames);
        let turn = std::f64::consts::TAU;
        let flag = if vertical { "FlipY" } else { "FlipX" };
        let mut patches = Vec::new();
        for (node, frame) in &frames {
            let (w, h) = frame.size;
            let centre = frame.to_parent((w / 2.0, h / 2.0));
            let mirrored = if vertical { (centre.0, 2.0 * cy - centre.1) } else { (2.0 * cx - centre.0, centre.1) };
            let angle = (-frame.angle).rem_euclid(turn);
            let angle = if turn - angle < 1e-12 { 0.0 } else { angle };
            let now = num(node, flag).unwrap_or(0.0) != 0.0;
            // the shape after the flip: the mirror toggled, the box centre then sits where the mirror image puts it
            let flip = if vertical { (frame.flip.0, !frame.flip.1) } else { (!frame.flip.0, frame.flip.1) };
            let offset = Frame { pin: (0.0, 0.0), size: frame.size, loc: frame.loc, angle, flip }.to_parent((w / 2.0, h / 2.0));
            patches.push(
                ShapeCellPatch::new(node.id.clone())
                    .set(flag, if now { "0" } else { "1" })
                    .set("Angle", fmt(angle))
                    .set("PinX", fmt(mirrored.0 - offset.0))
                    .set("PinY", fmt(mirrored.1 - offset.1)),
            );
        }
        Ok(patches)
    }

    /// Rotate shape `id` about its pin so that its absolute (page) angle becomes `world_angle`
    /// (radians). Only the `Angle` cell changes, like turning the rotation handle in Visio; the
    /// stored angle is relative to the ancestors, normalised to [0, 2 pi).
    pub fn rotate_member(&self, id: &str, world_angle: f64) -> Result<Vec<ShapeCellPatch>> {
        let chain = self.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
        frame_of(chain[chain.len() - 1])?;
        // The member's page direction is `L * (cos a, sin a)` with `L` the maps of its ancestors (turns and mirrors): the
        // stored angle is that of `L^-1 * (cos w, sin w)`, so a mirrored group turns its members the other way round.
        let inverse = linear_inverse(parent_linear(&chain));
        let (sw, cw) = world_angle.sin_cos();
        let (x, y) = (inverse[0][0] * cw + inverse[0][1] * sw, inverse[1][0] * cw + inverse[1][1] * sw);
        let turn = std::f64::consts::TAU;
        let local = y.atan2(x).rem_euclid(turn);
        let local = if turn - local < 1e-12 { 0.0 } else { local };
        Ok(vec![ShapeCellPatch::new(id).set("Angle", fmt(local))])
    }

    /// Move member `id` by a page-space delta (inches). With `refit`, the parent
    /// group frames grow to keep covering their members (never shrink).
    pub fn move_member(&self, id: &str, world_delta: (f64, f64), refit: bool) -> Result<Vec<ShapeCellPatch>> {
        let chain = self.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
        let member = chain[chain.len() - 1];
        let mut frame = frame_of(member)?;
        // Into the parent's local axes: undo every ancestor's turn and mirror.
        let inverse = linear_inverse(parent_linear(&chain));
        frame.pin.0 += inverse[0][0] * world_delta.0 + inverse[0][1] * world_delta.1;
        frame.pin.1 += inverse[1][0] * world_delta.0 + inverse[1][1] * world_delta.1;
        let old_pin = frame_of(member)?.pin;
        let local = (frame.pin.0 - old_pin.0, frame.pin.1 - old_pin.1);
        let mut patches = self.finish_member_edit(&chain, frame, refit)?;
        // A one-dimensional shape or group (a stencil rack server, a line) carries its end points in BeginX / BeginY / EndX / EndY (cached values,
        // in its parent's axes): they move with the pin, or the end handles stay behind where the shape was.
        if let Some(patch) = patches.iter_mut().find(|p| p.visio_id == member.id) {
            for (name, shift) in [("BeginX", local.0), ("EndX", local.0), ("BeginY", local.1), ("EndY", local.1)] {
                if let Some(value) = num(member, name) {
                    *patch = std::mem::replace(patch, ShapeCellPatch::new(member.id.clone())).set_cached(name, fmt(value + shift));
                }
            }
        }
        Ok(patches)
    }

    /// Resize member `id` to `new_size` (along its own axes) keeping `anchor` fixed.
    /// The member's own children scale with it.
    pub fn resize_member(
        &self,
        id: &str,
        new_size: (f64, f64),
        anchor: (f64, f64),
        refit: bool,
    ) -> Result<Vec<ShapeCellPatch>> {
        let chain = self.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
        let member = chain[chain.len() - 1];
        let old = frame_of(member)?;
        // A one-dimensional shape or group (BeginX / EndX cells: the Dell rack servers are 19 inch long lines with a
        // height of 0.001 and the art in children) is resized by its LENGTH; its height follows the length (a guard
        // formula in the master). Taking the requested height literally scaled the children by thousands in y and
        // pushed the art out of the box.
        let one_d = member.cells.contains_key("BeginX") && member.cells.contains_key("EndX");
        let new_size = if one_d && old.size.0.abs() > EPS { (new_size.0, old.size.1 * new_size.0 / old.size.0) } else { new_size };
        let new = resize_frame(&old, new_size, anchor);
        let scale = (
            if old.size.0.abs() > EPS { new.size.0 / old.size.0 } else { 1.0 },
            if old.size.1.abs() > EPS { new.size.1 / old.size.1 } else { 1.0 },
        );
        let mut extra = Vec::new();
        if member.shape_type.as_deref() == Some("Group") {
            scale_children(member, scale, &mut extra)?;
        }
        let mut patches = self.finish_member_edit(&chain, new, refit)?;
        patches.extend(extra);
        // A one-dimensional member (a flat group such as an IBM rack console switch, a stencil line) carries its end points in
        // BeginX / BeginY / EndX / EndY: they scale with its box, or its end handles stay where the old shape ended (step 226).
        // Only when the cached ends really span the box (an instance whose ends the drop code wrote at another scale is left to the
        // caller: the end-handle drag writes its own).
        let span = match (num(member, "BeginX"), num(member, "EndX"), num(member, "BeginY"), num(member, "EndY")) {
            (Some(bx), Some(ex), Some(by), Some(ey)) => (ex - bx).hypot(ey - by),
            _ => 0.0,
        };
        if one_d && (span - old.size.0.abs()).abs() <= 0.01 * old.size.0.abs().max(EPS) {
            if let Some(patch) = patches.iter_mut().find(|p| p.visio_id == member.id) {
                scale_end_points(member, &old, &new, scale, patch);
            }
        }
        // An instance of a master carries the recalculated cells itself (Visio does not recalculate on load).
        if member.master_id.is_some() {
            if let Some(patch) = patches.iter_mut().find(|p| p.visio_id == member.id) {
                add_instance_size_overrides(patch, member, new.size);
                // The children a group master supplies are written out, scaled, as children of the instance.
                if !member.master_children.is_empty() && member.children.is_empty() {
                    if let Some(xml) = materialize_master_children(member, scale) {
                        *patch = std::mem::replace(patch, ShapeCellPatch::new(member.id.clone())).append_children(xml);
                    }
                }
            }
        }
        // A child that is a master-backed page shape (written out by an earlier resize) is scaled with its geometry rows.
        for child_patch in patches.iter_mut() {
            if child_patch.visio_id == member.id {
                continue;
            }
            if let Some(child) = find_node(member, &child_patch.visio_id) {
                if child.master_shape_id.is_some() {
                    add_scaled_child_overrides(child_patch, child, scale);
                }
            }
        }
        Ok(patches)
    }

    /// Emit the member's patch and, when asked, expand the ancestor frames.
    fn finish_member_edit(&self, chain: &[&ResolvedShape], frame: Frame, refit: bool) -> Result<Vec<ShapeCellPatch>> {
        let id = chain[chain.len() - 1].id.clone();
        let mut frames: BTreeMap<String, Frame> = BTreeMap::new();
        frames.insert(id.clone(), frame);
        if refit {
            // Walk up: each ancestor group grows to cover its (possibly updated) members.
            for depth in (0..chain.len() - 1).rev() {
                let group = chain[depth];
                let mut group_frame = *frames.get(&group.id).unwrap_or(&frame_of(group)?);
                let (mut min, mut max) = ((0.0f64, 0.0f64), (group_frame.size.0, group_frame.size.1));
                for child in &group.children {
                    let child_frame = match frames.get(&child.id) {
                        Some(frame) => *frame,
                        None => match frame_of(child) {
                            Ok(frame) => frame,
                            Err(_) => continue,
                        },
                    };
                    for (x, y) in child_frame.corners() {
                        min.0 = min.0.min(x);
                        min.1 = min.1.min(y);
                        max.0 = max.0.max(x);
                        max.1 = max.1.max(y);
                    }
                }
                if min == (0.0, 0.0) && max == (group_frame.size.0, group_frame.size.1) {
                    break; // already covers everything; ancestors are unaffected
                }
                // Grow the box to [min, max]; shift children and LocPin by -min so the
                // children (and the group's pin) stay put on the page.
                for child in &group.children {
                    let entry = match frames.get(&child.id) {
                        Some(frame) => *frame,
                        None => match frame_of(child) {
                            Ok(frame) => frame,
                            Err(_) => continue,
                        },
                    };
                    let mut shifted = entry;
                    shifted.pin = (entry.pin.0 - min.0, entry.pin.1 - min.1);
                    frames.insert(child.id.clone(), shifted);
                }
                group_frame.loc = (group_frame.loc.0 - min.0, group_frame.loc.1 - min.1);
                group_frame.size = (max.0 - min.0, max.1 - min.1);
                frames.insert(group.id.clone(), group_frame);
            }
        }
        Ok(frames.iter().map(|(id, frame)| frame_patch(id, frame)).collect())
    }
}

fn find_node<'a>(root: &'a ResolvedShape, id: &str) -> Option<&'a ResolvedShape> {
    for child in &root.children {
        if child.id == id {
            return Some(child);
        }
        if let Some(found) = find_node(child, id) {
            return Some(found);
        }
    }
    None
}

/// The factors a child turned by `angle` is scaled by (its width, its height) inside a group scaled by `scale`. Measured on
/// Visio (probe-group-rotated-child.ps1: a 1 x 2 child turned 30 degrees in a group made 2 x 0.5 as large became 1.625 x
/// 1.75, still turned 30 degrees, its pin scaled by the group's factors): the child keeps its angle and takes
/// `sx cos^2 + sy sin^2` of its width and `sx sin^2 + sy cos^2` of its height. At a quarter turn that is the swap of the
/// two factors, at a half turn or none the factors themselves.
fn own_scale(angle: f64, scale: (f64, f64)) -> Option<(f64, f64)> {
    let (sin, cos) = (angle.sin().powi(2), angle.cos().powi(2));
    Some((scale.0 * cos + scale.1 * sin, scale.0 * sin + scale.1 * cos))
}

/// The scaled geometry rows (as overrides) and the picture / text block cells of a child.
/// `None` when a geometry row has a type whose coordinates cannot simply be scaled.
fn scaled_geometry_and_cells(
    cells: &BTreeMap<String, crate::shapes::Cell>,
    sections: &[crate::shapes::Section],
    own: (f64, f64),
) -> Option<(Vec<(String, String)>, Vec<crate::writer::RowOverride>)> {
    let value = |name: &str| cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
    let mut out_cells: Vec<(String, String)> = Vec::new();
    for (name, factor) in [
        ("ImgWidth", own.0), ("ImgOffsetX", own.0), ("ImgHeight", own.1), ("ImgOffsetY", own.1),
        ("TxtPinX", own.0), ("TxtWidth", own.0), ("TxtLocPinX", own.0), ("TxtPinY", own.1), ("TxtHeight", own.1), ("TxtLocPinY", own.1),
    ] {
        if let Some(v) = value(name) {
            out_cells.push((name.to_string(), fmt(v * factor)));
        }
    }
    let mut rows: Vec<crate::writer::RowOverride> = Vec::new();
    for section in sections.iter().filter(|s| s.name == "Geometry") {
        for row in &section.rows {
            let kind = row.row_type.as_deref().unwrap_or("");
            // coordinate cells to scale: (x-like, y-like)
            let (xs, ys, scalars): (&[&str], &[&str], &[&str]) = match kind {
                "MoveTo" | "LineTo" => (&["X"], &["Y"], &[]),
                // a polyline: its end point scales; the points of `POLYLINE(xType, yType, x1, y1, ...)` are fractions of the shape when
                // the type is 0 (they scale by themselves) and inches when it is 1 (scaled below)
                "PolylineTo" => (&["X"], &["Y"], &[]),
                "ArcTo" => (&["X"], &["Y"], &["A"]),
                // a NURBS segment of fractional points (`NURBS(knot, degree, 0, 0, ...)`, what the stencil art uses): its end point scales and the
                // control points scale by themselves; one in inches (type 1) is not handled
                "NURBSTo" => {
                    let text = row.cells.get("E").and_then(|c| c.formula.as_deref().or(c.value.as_deref()))?;
                    let args: Vec<&str> = text.trim().strip_prefix("NURBS(")?.split(',').map(str::trim).collect();
                    if args.get(2) != Some(&"0") || args.get(3) != Some(&"0") {
                        return None;
                    }
                    (&["X"], &["Y"], &[])
                }
                "EllipticalArcTo" => (&["X", "A"], &["Y", "B"], &[]),
                "Ellipse" => (&["X", "A", "C"], &["Y", "B", "D"], &[]),
                "RelMoveTo" | "RelLineTo" | "RelCubBezTo" | "RelQuadBezTo" | "RelEllipticalArcTo" => continue,
                _ => return None,
            };
            // An elliptical arc under a non-uniform scale is still an elliptical arc: the end point and the control point scale,
            // the axes follow from the singular values of `scale * R(C) * diag(1, 1/D)` (`D` the ratio of the axes, `C` the
            // angle of the major axis): the new ratio is their quotient and the new angle that of the major left vector.
            let (arc_c, arc_d): (Option<f64>, Option<f64>) = if kind == "EllipticalArcTo" && (own.0 - own.1).abs() > 1e-9 {
                let c = row.cells.get("C").and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                let d = row.cells.get("D").and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
                if d.abs() < 1e-9 {
                    return None;
                }
                let (new_c, new_d) = scale_arc_axes(c, d, own)?;
                (Some(new_c), Some(new_d))
            } else {
                (None, None)
            };
            let mut cells_out: Vec<(String, String, String)> = Vec::new();
            if kind == "PolylineTo" {
                if let Some(cell) = row.cells.get("A") {
                    let text = cell.formula.as_deref().or(cell.value.as_deref())?;
                    let scaled = scale_polyline(text, own)?;
                    if scaled != text.replace(' ', "") && scaled != text {
                        cells_out.push(("A".to_string(), scaled.clone(), scaled));
                    }
                }
            }
            for (name, cell) in &row.cells {
                let Some(v) = cell.value.as_deref().and_then(|v| v.parse::<f64>().ok()) else { continue };
                if kind == "EllipticalArcTo" && (name == "D" || name == "C") {
                    match (name.as_str(), arc_c, arc_d) {
                        ("D", _, Some(d)) => cells_out.push((name.clone(), fmt(d), String::new())),
                        ("C", Some(c), _) => cells_out.push((name.clone(), fmt(c), String::new())),
                        _ => {}
                    }
                    continue;
                }
                let factor = if xs.contains(&name.as_str()) {
                    own.0
                } else if ys.contains(&name.as_str()) {
                    own.1
                } else if scalars.contains(&name.as_str()) {
                    (own.0 + own.1) / 2.0
                } else {
                    continue;
                };
                cells_out.push((name.clone(), fmt(v * factor), String::new()));
            }
            if !cells_out.is_empty() {
                rows.push(crate::writer::RowOverride {
                    section: "Geometry".to_string(),
                    section_index: section.index.clone().unwrap_or_else(|| "0".to_string()),
                    row_index: row.index.clone().unwrap_or_default(),
                    row_type: row.row_type.clone(),
                    cells: cells_out,
                });
            }
        }
    }
    Some((out_cells, rows))
}

/// The angle and axis ratio of an elliptical arc (`C`, `D`) after the plane is scaled by `own` (see the arc comment in
/// `scaled_geometry_and_cells`). `None` for a degenerate result.
fn scale_arc_axes(c: f64, d: f64, own: (f64, f64)) -> Option<(f64, f64)> {
    let (sin, cos) = c.sin_cos();
    let m = [[own.0 * cos, -own.0 * sin / d], [own.1 * sin, own.1 * cos / d]];
    let (p, q, r) = (m[0][0] * m[0][0] + m[0][1] * m[0][1], m[0][0] * m[1][0] + m[0][1] * m[1][1], m[1][0] * m[1][0] + m[1][1] * m[1][1]);
    let root = (((p - r) / 2.0).powi(2) + q * q).sqrt();
    let (big, small) = ((p + r) / 2.0 + root, (p + r) / 2.0 - root);
    if small <= 1e-12 {
        return None;
    }
    Some((0.5 * (2.0 * q).atan2(p - r), (big / small).sqrt()))
}

/// `POLYLINE(xType, yType, x1, y1, x2, y2, ...)` scaled by `own`: a type of 0 means fractions of the shape (nothing to do),
/// 1 means inches (multiplied by the factor of that axis). The text unchanged when nothing needs scaling; `None` for another
/// type or a text that is not a polyline.
fn scale_polyline(text: &str, own: (f64, f64)) -> Option<String> {
    let t = text.trim();
    let inner = t.strip_prefix("POLYLINE(")?.strip_suffix(')')?;
    let numbers: Vec<f64> = inner.split(',').map(|n| n.trim().parse::<f64>().ok()).collect::<Option<Vec<_>>>()?;
    if numbers.len() < 2 || numbers.len() % 2 != 0 {
        return None;
    }
    let (xt, yt) = (numbers[0], numbers[1]);
    if ![0.0, 1.0].contains(&xt) || ![0.0, 1.0].contains(&yt) {
        return None;
    }
    if xt == 0.0 && yt == 0.0 {
        return Some(t.to_string());
    }
    let mut out = format!("POLYLINE({xt}, {yt}");
    for pair in numbers[2..].chunks(2) {
        let x = if xt == 1.0 { pair[0] * own.0 } else { pair[0] };
        let y = if yt == 1.0 { pair[1] * own.1 } else { pair[1] };
        out.push_str(&format!(", {}, {}", fmt(x), fmt(y)));
    }
    out.push(')');
    Some(out)
}

/// Scale the picture / text block cells and the geometry rows of a master-backed child already on the page.
fn add_scaled_child_overrides(patch: &mut ShapeCellPatch, child: &ResolvedShape, scale: (f64, f64)) {
    let angle = child.cells.get("Angle").and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let Some(own) = own_scale(angle, scale) else { return };
    let Some((cells, rows)) = scaled_geometry_and_cells(&child.cells, &child.sections, own) else { return };
    let mut next = std::mem::replace(patch, ShapeCellPatch::new(child.id.clone()));
    for (name, value) in cells {
        next = next.set(name, value);
    }
    for row in rows {
        next = next.row_override(row);
    }
    // a one-dimensional child's end points (the group's coordinates) scale like its pin
    for (xn, yn) in [("BeginX", "BeginY"), ("EndX", "EndY")] {
        let v = |name: &str| child.cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
        if let (Some(x), Some(y)) = (v(xn), v(yn)) {
            next = next.set(xn, fmt(x * scale.0)).set(yn, fmt(y * scale.1));
        }
    }
    *patch = next;
}

/// The children a group master supplies, written out as page shapes scaled with the group (Visio's own files carry them
/// like this, each with explicit cells; it does not scale master children by itself when it loads an instance whose size
/// differs from the master's). IDs follow the instance's (Visio's implicit ids). `None` (nothing is written) when a child
/// is itself a group, has no frame cells or has geometry that cannot simply be scaled.
/// A master formula with its `Sheet.N!` references mapped to the page: the master's top shape is the group instance, a
/// sibling master child is the child written with id `first + its position`. `None` when it refers to something else.
fn remap_sheet_refs(formula: &str, group: &ResolvedShape, first: u64) -> Option<String> {
    let top = group.master_top_id.as_deref();
    let mut out = String::new();
    let mut rest = formula;
    while let Some(at) = rest.find("Sheet.") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "Sheet.".len()..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || !after[digits.len()..].starts_with('!') {
            return None;
        }
        let new_id = if Some(digits.as_str()) == top {
            group.id.parse::<u64>().ok()?
        } else {
            let position = group.master_children.iter().position(|c| c.id == digits)?;
            first + position as u64
        };
        out.push_str(&format!("Sheet.{new_id}!"));
        rest = &after[digits.len() + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn materialize_master_children(group: &ResolvedShape, scale: (f64, f64)) -> Option<String> {
    let first: u64 = group.id.parse::<u64>().ok()? + 1;
    // The master tree: a child whose parent is not another master child hangs directly below the group.
    let is_member = |id: &str| group.master_children.iter().any(|c| c.id == id);
    let xml = materialize_level(group, first, None, scale, &is_member)?;
    (!xml.is_empty()).then_some(xml)
}

/// The master children below `parent` (`None` = directly below the group), written as page shapes scaled by `scale`; a
/// child that is itself a group carries its own children in a nested `<Shapes>`, scaled by that group's own factors
/// (they live in its local axes). Ids are `first` plus the position in the master's flat list, so formulas can name them.
fn materialize_level(group: &ResolvedShape, first: u64, parent: Option<&str>, scale: (f64, f64), is_member: &dyn Fn(&str) -> bool) -> Option<String> {
    let mut xml = String::new();
    for (k, child) in group.master_children.iter().enumerate() {
        let belongs = match (&child.parent, parent) {
            (Some(p), Some(want)) => p == want,
            (Some(p), None) => !is_member(p),
            (None, None) => true,
            (None, Some(_)) => false,
        };
        if !belongs {
            continue;
        }
        let value = |name: &str| child.cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
        let angle = value("Angle").unwrap_or(0.0);
        let own = own_scale(angle, scale)?;
        let (pin_x, pin_y, w, h, loc_x, loc_y) = (value("PinX")?, value("PinY")?, value("Width")?, value("Height")?, value("LocPinX")?, value("LocPinY")?);
        let kind = child.shape_type.clone().unwrap_or_else(|| "Shape".to_string());
        let mut element = format!("<Shape ID='{}' Type='{kind}' MasterShape='{}'>", first + k as u64, child.id);
        let mut frame_cells: Vec<(&str, f64)> = vec![
            ("PinX", pin_x * scale.0), ("PinY", pin_y * scale.1), ("Width", w * own.0), ("Height", h * own.1), ("LocPinX", loc_x * own.0), ("LocPinY", loc_y * own.1),
        ];
        // a one-dimensional child (a decorative line) states its end points in the group's coordinates: they scale like its pin
        if let (Some(bx), Some(by), Some(ex), Some(ey)) = (value("BeginX"), value("BeginY"), value("EndX"), value("EndY")) {
            frame_cells.extend([("BeginX", bx * scale.0), ("BeginY", by * scale.1), ("EndX", ex * scale.0), ("EndY", ey * scale.1)]);
        }
        for (name, v) in frame_cells {
            // A cell the master states by a formula (`Sheet.5!Width*0.91`) keeps that formula, its sheet references mapped
            // to this page (the top shape is the group, a sibling is the child written with the next id). Without a formula
            // of its own Visio takes the master's cached value and the child keeps the master's size (probe: report 18).
            let formula = child.cells.get(name).and_then(|c| c.formula.as_deref()).map(str::trim).filter(|f| !f.is_empty()).and_then(|f| remap_sheet_refs(f, group, first));
            let flag = formula.map(|f| format!(" F='{}'", escape_xml_attr(&f))).unwrap_or_default();
            element.push_str(&format!("<Cell N='{name}' V='{}'{flag}/>", fmt(v)));
        }
        let (cells, rows) = scaled_geometry_and_cells(&child.cells, &child.sections, own)?;
        for (name, v) in cells {
            element.push_str(&format!("<Cell N='{name}' V='{v}'/>"));
        }
        // group the row overrides by section
        let mut sections: Vec<(String, Vec<&crate::writer::RowOverride>)> = Vec::new();
        for row in &rows {
            match sections.iter_mut().find(|(ix, _)| *ix == row.section_index) {
                Some((_, list)) => list.push(row),
                None => sections.push((row.section_index.clone(), vec![row])),
            }
        }
        for (ix, list) in sections {
            element.push_str(&format!("<Section N='Geometry' IX='{ix}'>"));
            for row in list {
                let t = row.row_type.as_ref().map(|t| format!(" T='{t}'")).unwrap_or_default();
                element.push_str(&format!("<Row{t} IX='{}'>", row.row_index));
                for (name, v, f) in &row.cells {
                    // a polyline's points are a formula (`POLYLINE(1, 1, ...)`): Visio needs it as the cell's formula and unit
                    let (flag, unit) = if f.is_empty() { (String::new(), "") } else { (format!(" F='{}'", escape_xml_attr(f)), " U='POLYLINE'") };
                    element.push_str(&format!("<Cell N='{name}' V='{}'{unit}{flag}/>", escape_xml_attr(v)));
                }
                element.push_str("</Row>");
            }
            element.push_str("</Section>");
        }
        if kind == "Group" {
            let inner = materialize_level(group, first, Some(&child.id), own, is_member)?;
            if !inner.is_empty() {
                element.push_str(&format!("<Shapes>{inner}</Shapes>"));
            }
        }
        element.push_str("</Shape>");
        xml.push_str(&element);
    }
    Some(xml)
}

/// Cells of an instance of a master that Visio derives from its size by formula (`Width*0.5` of a geometry or connection
/// point row, the picture and text block cells of a picture shape): after a resize they are written on the instance with
/// the new values and their formulas, geometry and connection point rows as overrides of the master's rows.
fn add_instance_size_overrides(patch: &mut ShapeCellPatch, node: &ResolvedShape, size: (f64, f64)) {
    use crate::shapes::{multiple_of, refresh_geometry_for_size};
    let mut quantities: BTreeMap<String, f64> = BTreeMap::new();
    quantities.insert("Width".to_string(), size.0);
    quantities.insert("Height".to_string(), size.1);
    // The top-level cells: Img*/Txt* cells whose formula is a multiple of the size or of another such cell (two passes).
    let skip = ["Width", "Height", "PinX", "PinY", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY"];
    let mut cells: Vec<(String, String, f64)> = Vec::new();
    for _ in 0..2 {
        cells.clear();
        for (name, cell) in &node.cells {
            if skip.contains(&name.as_str()) {
                continue;
            }
            let Some(formula) = cell.formula.as_deref() else { continue };
            if let Some(value) = multiple_of(formula, &quantities) {
                cells.push((name.clone(), formula.to_string(), value));
            }
        }
        for (name, _, value) in &cells {
            quantities.insert(name.clone(), *value);
        }
    }
    let mut next = std::mem::replace(patch, ShapeCellPatch::new(node.id.clone()));
    for (name, formula, value) in cells {
        next = next.set_with_formula(name, fmt(value), formula);
    }
    // Geometry and connection point rows.
    let mut refreshed = node.sections.clone();
    refresh_geometry_for_size(&mut refreshed, size.0, size.1);
    for (before, after) in node.sections.iter().zip(refreshed.iter()) {
        if after.name != "Geometry" && after.name != "Connection" {
            continue;
        }
        // Visio does NOT merge partial Connection rows of an instance with the master's (probe of step 243: an instance written with only the changed rows
        // has just those rows, and a connector glued to a later point loses its glue): when any point moves with the size, the whole section is written
        // out, every row with every cell, the way Visio saves an instance.
        let whole = after.name == "Connection"
            && before.rows.iter().zip(after.rows.iter()).any(|(b, a)| a.cells.iter().any(|(name, cell)| {
                let old = b.cells.get(name).and_then(|c| c.value.as_deref());
                cell.formula.is_some() && cell.value.as_deref().is_some_and(|v| old.and_then(|o| o.parse::<f64>().ok()) != v.parse::<f64>().ok())
            }));
        for (row_before, row_after) in before.rows.iter().zip(after.rows.iter()) {
            let mut changed: Vec<(String, String, String)> = Vec::new();
            for (name, cell) in &row_after.cells {
                let old = row_before.cells.get(name).and_then(|c| c.value.as_deref());
                if whole {
                    if let Some(value) = cell.value.as_deref() {
                        changed.push((name.clone(), value.to_string(), cell.formula.clone().unwrap_or_default()));
                    }
                } else if let (Some(formula), Some(value)) = (cell.formula.as_deref(), cell.value.as_deref()) {
                    let differs = old.and_then(|o| o.parse::<f64>().ok()) != value.parse::<f64>().ok();
                    if differs {
                        changed.push((name.clone(), value.to_string(), formula.to_string()));
                    }
                }
            }
            if !changed.is_empty() {
                next = next.row_override(crate::writer::RowOverride {
                    section: after.name.clone(),
                    // a Connection section has no IX (Visio writes `<Section N='Connection'>`): with IX='0' Visio does not merge the rows with the master's
                    section_index: if after.name == "Geometry" { after.index.clone().unwrap_or_else(|| "0".to_string()) } else { String::new() },
                    row_index: row_after.index.clone().unwrap_or_default(),
                    row_type: row_after.row_type.clone(),
                    cells: changed,
                });
            }
        }
    }
    *patch = next;
}

/// Scale every descendant of `group` for a group box scaled by `scale` (in the
/// group's own axes). Children are positioned in the group's box, so their pin,
/// size and LocPin scale; a child rotated by 90 degrees swaps the factors for its
/// own size and for its own children.
fn scale_children(group: &ResolvedShape, scale: (f64, f64), out: &mut Vec<ShapeCellPatch>) -> Result<()> {
    for child in &group.children {
        let Ok(frame) = frame_of(child) else { continue };
        // Scale in the child's own axes (see `own_scale`: a child turned by any angle keeps it).
        let own = own_scale(frame.angle, scale).ok_or_else(|| Error(format!("member {} cannot be scaled", child.id)))?;
        let scaled = Frame {
            pin: (frame.pin.0 * scale.0, frame.pin.1 * scale.1),
            size: (frame.size.0 * own.0, frame.size.1 * own.1),
            loc: (frame.loc.0 * own.0, frame.loc.1 * own.1),
            angle: frame.angle,
            flip: frame.flip,
        };
        let mut patch = frame_patch(&child.id, &scaled);
        // a one-dimensional member (a decorative line) keeps its end points in the group's own coordinates: they scale like its pin
        for (xn, yn) in [("BeginX", "BeginY"), ("EndX", "EndY")] {
            if let (Some(x), Some(y)) = (num(child, xn), num(child, yn)) {
                patch = patch.set(xn, fmt(x * scale.0)).set(yn, fmt(y * scale.1));
            }
        }
        out.push(patch);
        if child.shape_type.as_deref() == Some("Group") {
            scale_children(child, own, out)?;
        }
    }
    Ok(())
}

/// Hop size at factor 1: 2.5 mm wide, 1.25 mm high (Visio stores 0.0656 in x 0.0328 in at 2/3).
const HOP_WIDTH: f64 = 2.5 / 25.4;
const HOP_HEIGHT: f64 = 1.25 / 25.4;

/// A crossing of a connector's segment with another connector that makes this one hop.
struct Hop {
    segment: usize,
    /// Distance from the segment's start (inches).
    along: f64,
    horizontal: bool,
    /// Index of the connector crossed.
    other: usize,
}

/// Where each route hops over another, by the page's line-jump settings. `routes` are in the order
/// Visio routed them (document order here). Codes: 1/7 horizontal segments hop, 2 vertical ones,
/// 3/4 the connector routed last, 5 the one routed first; others none. Verified against Visio for
/// codes 0-7 with two crossing connectors (`make-linejump-pages.ps1`).
fn compute_hops(routes: &[Vec<(f64, f64)>], jump: &LineJump) -> Vec<Vec<Hop>> {
    let mut out: Vec<Vec<Hop>> = routes.iter().map(|_| Vec::new()).collect();
    if !matches!(jump.code, 1..=5 | 7) {
        return out;
    }
    let width = HOP_WIDTH * jump.factor_x;
    let eps = 0.005;
    for (i, a) in routes.iter().enumerate() {
        for (j, b) in routes.iter().enumerate() {
            if i == j {
                continue;
            }
            for (si, sa) in a.windows(2).enumerate() {
                // `a`'s horizontal segment against `b`'s vertical one.
                if (sa[0].1 - sa[1].1).abs() > 1e-9 || (sa[0].0 - sa[1].0).abs() < 1e-9 {
                    continue;
                }
                for sb in b.windows(2) {
                    if (sb[0].0 - sb[1].0).abs() > 1e-9 || (sb[0].1 - sb[1].1).abs() < 1e-9 {
                        continue;
                    }
                    let (x, y) = (sb[0].0, sa[0].1);
                    let h_jumps = match jump.code {
                        1 | 7 => true,
                        2 => false,
                        3 | 4 => i > j,
                        _ => i < j,
                    };
                    let between = |lo: f64, hi: f64, v: f64, margin: f64| v > lo.min(hi) + margin && v < lo.max(hi) - margin;
                    let (h_margin, v_margin) = if h_jumps { (width / 2.0 + eps, eps) } else { (eps, width / 2.0 + eps) };
                    if !between(sa[0].0, sa[1].0, x, h_margin) || !between(sb[0].1, sb[1].1, y, v_margin) {
                        continue;
                    }
                    if h_jumps {
                        out[i].push(Hop { segment: si, along: (x - sa[0].0).abs(), horizontal: true, other: j });
                    } else {
                        // The vertical segment of `b` hops over `a`: find its index in `b`.
                        if let Some(sj) = b.windows(2).position(|w| w[0] == sb[0] && w[1] == sb[1]) {
                            out[j].push(Hop { segment: sj, along: (y - sb[0].1).abs(), horizontal: false, other: i });
                        }
                    }
                }
            }
        }
    }
    for hops in &mut out {
        hops.sort_by(|p, q| (p.segment, p.along).partial_cmp(&(q.segment, q.along)).unwrap_or(std::cmp::Ordering::Equal));
        hops.dedup_by(|p, q| p.segment == q.segment && (p.along - q.along).abs() < 1e-9);
    }
    out
}

fn push_row(rows: &mut Vec<GeometryRow>, origin: (f64, f64), kind: &str, p: (f64, f64)) {
    let index = rows.len() + 1;
    rows.push(GeometryRow::new(kind, index.to_string()).set("X", fmt(p.0 - origin.0)).set("Y", fmt(p.1 - origin.1)));
}

/// Geometry rows of `route` (page inches, relative to `origin`) with its hops drawn in the
/// page's line-jump style: arcs as `ArcTo`, polygons as `PolylineTo` with absolute `POLYLINE`
/// points, gaps as `MoveTo` (the way Visio stores them).
fn hop_rows(route: &[(f64, f64)], hops: &[Hop], origin: (f64, f64), jump: &LineJump) -> Vec<GeometryRow> {
    let local = |p: (f64, f64)| (p.0 - origin.0, p.1 - origin.1);
    let (width, height) = (HOP_WIDTH * jump.factor_x, HOP_HEIGHT * jump.factor_y);
    let mut rows = Vec::new();
    push_row(&mut rows, origin, "MoveTo", route[0]);
    for (si, pair) in route.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let t = ((b.0 - a.0) / len, (b.1 - a.1) / len);
        for hop in hops.iter().filter(|h| h.segment == si) {
            // Hops bulge up from horizontal segments and left from vertical ones.
            let n = if hop.horizontal { (0.0, 1.0) } else { (-1.0, 0.0) };
            let at = (a.0 + t.0 * hop.along, a.1 + t.1 * hop.along);
            let start = (at.0 - t.0 * width / 2.0, at.1 - t.1 * width / 2.0);
            let end = (at.0 + t.0 * width / 2.0, at.1 + t.1 * width / 2.0);
            push_row(&mut rows, origin, "LineTo", start);
            match jump.style {
                2 => push_row(&mut rows, origin, "MoveTo", end),
                0 | 1 => {
                    // ArcTo's A is the sagitta, positive to the right of the direction of travel.
                    let right = (t.1, -t.0);
                    let a_value = height * (n.0 * right.0 + n.1 * right.1);
                    let l = local(end);
                    let index = rows.len() + 1;
                    rows.push(
                        GeometryRow::new("ArcTo", index.to_string())
                            .set("X", fmt(l.0))
                            .set("Y", fmt(l.1))
                            .set("A", fmt(a_value)),
                    );
                }
                style => {
                    // Square (3) has two corners; 4..=8 are polygons with 2..=6 sides.
                    let mut points = Vec::new();
                    if style == 3 {
                        points.push((start.0 + n.0 * height, start.1 + n.1 * height));
                        points.push((end.0 + n.0 * height, end.1 + n.1 * height));
                    } else {
                        let sides = (style.clamp(4, 8) - 2) as i32;
                        for k in 1..sides {
                            let angle = std::f64::consts::PI - f64::from(k) * std::f64::consts::PI / f64::from(sides);
                            let (s, c) = (angle.sin(), angle.cos());
                            points.push((
                                at.0 + t.0 * c * width / 2.0 + n.0 * s * height,
                                at.1 + t.1 * c * width / 2.0 + n.1 * s * height,
                            ));
                        }
                    }
                    let list: Vec<String> = points
                        .iter()
                        .map(|p| {
                            let l = local(*p);
                            format!("{},{}", fmt(l.0), fmt(l.1))
                        })
                        .collect();
                    let l = local(end);
                    let index = rows.len() + 1;
                    rows.push(
                        GeometryRow::new("PolylineTo", index.to_string())
                            .set("X", fmt(l.0))
                            .set("Y", fmt(l.1))
                            .formula("A", format!("POLYLINE(1, 1, {})", list.join(", "))),
                    );
                }
            }
        }
        push_row(&mut rows, origin, "LineTo", b);
    }
    rows
}

/// A connector's stored route without its hops (page inches), the origin of its local frame and
/// whether it stores any hop rows (`ArcTo`, `PolylineTo`, or a `MoveTo` after the first row).
fn stored_base_route(node: &ResolvedShape) -> Option<(Vec<(f64, f64)>, (f64, f64), bool)> {
    let section = node.sections.iter().find(|s| s.name == "Geometry")?;
    let origin = (num(node, "PinX")? - num(node, "LocPinX")?, num(node, "PinY")? - num(node, "LocPinY")?);
    let value = |row: &crate::shapes::SectionRow, name: &str| row.cells.get(name)?.value.as_deref()?.parse::<f64>().ok();
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut hops = false;
    for (i, row) in section.rows.iter().enumerate() {
        let kind = row.row_type.as_deref()?;
        if !matches!(kind, "MoveTo" | "LineTo" | "ArcTo" | "PolylineTo") {
            return None;
        }
        hops |= matches!(kind, "ArcTo" | "PolylineTo") || (kind == "MoveTo" && i > 0);
        let p = (origin.0 + value(row, "X")?, origin.1 + value(row, "Y")?);
        // Drop points that sit on the line between their neighbours (hop ends).
        while points.len() >= 2 {
            let (a, b) = (points[points.len() - 2], points[points.len() - 1]);
            let collinear = ((a.0 - b.0).abs() < 1e-6 && (b.0 - p.0).abs() < 1e-6)
                || ((a.1 - b.1).abs() < 1e-6 && (b.1 - p.1).abs() < 1e-6);
            if collinear {
                points.pop();
            } else {
                break;
            }
        }
        points.push(p);
    }
    (points.len() >= 2).then_some((points, origin, hops))
}

/// Whether the connector's stored geometry rows equal `rows` (kind and X/Y within 1e-6).
fn same_rows(node: &ResolvedShape, rows: &[GeometryRow]) -> bool {
    let Some(section) = node.sections.iter().find(|s| s.name == "Geometry") else { return false };
    if section.rows.len() != rows.len() {
        return false;
    }
    let value = |row: &crate::shapes::SectionRow, name: &str| row.cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
    section.rows.iter().zip(rows).all(|(stored, new)| {
        stored.row_type.as_deref() == Some(new.kind.as_str())
            && ["X", "Y"].iter().all(|name| match (value(stored, name), new.cells.get(*name).and_then(|v| v.parse::<f64>().ok())) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-6,
                _ => false,
            })
    })
}

#[cfg(test)]
mod hop_tests {
    use super::*;

    /// Visio saved `linejump-styles.vsdx` (one page per `LineJumpStyle` 0..8, a horizontal
    /// connector over a vertical one). Our hop rows must equal what Visio stored for the
    /// horizontal connector: same row kinds, end points, arc sagitta and polygon vertices.
    #[cfg(private_fixtures)]
    #[test]
    fn hop_rows_match_the_geometry_visio_stored() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/linejump-styles.vsdx");
        let package = Package::open(path).unwrap();
        for style in 0..9usize {
            let editor = GroupEditor::open(&package, style).unwrap();
            let jump = LineJump { code: 1, style: style as i32, factor_x: 2.0 / 3.0, factor_y: 2.0 / 3.0 };
            let connectors: Vec<&ResolvedShape> = editor
                .roots
                .iter()
                .filter(|n| num(n, "BeginX").is_some() && num(n, "EndX").is_some())
                .collect();
            assert_eq!(connectors.len(), 2, "style {style}");
            let bases: Vec<_> = connectors.iter().map(|n| stored_base_route(n).unwrap()).collect();
            let routes: Vec<Vec<(f64, f64)>> = bases.iter().map(|b| b.0.clone()).collect();
            let hops = compute_hops(&routes, &jump);
            // The horizontal connector is the first one created.
            let rows = hop_rows(&routes[0], &hops[0], bases[0].1, &jump);
            let section = connectors[0].sections.iter().find(|s| s.name == "Geometry").unwrap();
            assert_eq!(rows.len(), section.rows.len(), "style {style}: row count");
            let (w, h) = (num(connectors[0], "Width").unwrap_or(0.0), num(connectors[0], "Height").unwrap_or(0.0));
            for (ours, stored) in rows.iter().zip(&section.rows) {
                assert_eq!(Some(ours.kind.as_str()), stored.row_type.as_deref(), "style {style}");
                for name in ["X", "Y"] {
                    let a: f64 = ours.cells[name].parse().unwrap();
                    let b: f64 = stored.cells[name].value.as_deref().unwrap().parse().unwrap();
                    assert!((a - b).abs() < 1e-4, "style {style} {} {name}: {a} vs {b}", ours.kind);
                }
                if ours.kind == "ArcTo" {
                    let a: f64 = ours.cells["A"].parse().unwrap();
                    let b: f64 = stored.cells["A"].value.as_deref().unwrap().parse().unwrap();
                    assert!((a - b).abs() < 1e-4, "style {style} arc: {a} vs {b}");
                }
                if ours.kind == "PolylineTo" {
                    let mine: Vec<f64> = ours.formulas["A"]
                        .trim_start_matches("POLYLINE(1, 1, ")
                        .trim_end_matches(')')
                        .split(',')
                        .map(|v| v.trim().parse().unwrap())
                        .collect();
                    let formula = stored.cells["A"].formula.clone().unwrap();
                    let theirs: Vec<f64> = formula
                        .trim_start_matches("POLYLINE(")
                        .trim_end_matches(')')
                        .split(',')
                        .map(|v| v.trim().parse().unwrap())
                        .skip(2)
                        .collect();
                    assert_eq!(mine.len(), theirs.len(), "style {style}: polygon vertices");
                    for (i, (m, t)) in mine.iter().zip(&theirs).enumerate() {
                        // Visio stores fractions of Width (x) and Height (y), from the shape's origin.
                        let absolute = if i % 2 == 0 { t * w } else { t * h };
                        // Visio rounds the polygon heights to 2 decimals (0.86 for sin 60 deg), hence the tolerance.
                        assert!((m - absolute).abs() < 6e-4, "style {style} vertex {i}: {m} vs {absolute}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod hop_code_tests {
    use super::*;

    /// `linejump-codes.vsdx`: pages 0..7 have page `LineJumpCode` 0..7 with the horizontal
    /// connector created first, pages 8..15 the vertical one first. Visio stores hop rows only
    /// in the connector that hops; `compute_hops` must pick the same one.
    #[cfg(private_fixtures)]
    #[test]
    fn hopping_connector_matches_visio_for_every_code_and_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/linejump-codes.vsdx");
        let package = Package::open(path).unwrap();
        for page in 0..16usize {
            let editor = GroupEditor::open(&package, page).unwrap();
            let connectors: Vec<&ResolvedShape> = editor
                .roots
                .iter()
                .filter(|n| num(n, "BeginX").is_some() && num(n, "EndX").is_some())
                .collect();
            let bases: Vec<_> = connectors.iter().map(|n| stored_base_route(n).unwrap()).collect();
            let routes: Vec<Vec<(f64, f64)>> = bases.iter().map(|b| b.0.clone()).collect();
            let jump = LineJump { code: (page % 8) as i32, ..LineJump::default() };
            let ours: Vec<bool> = compute_hops(&routes, &jump).iter().map(|h| !h.is_empty()).collect();
            let visio: Vec<bool> = bases.iter().map(|b| b.2).collect();
            assert_eq!(ours, visio, "page {page} (code {}, {} first)", page % 8, if page >= 8 { "vertical" } else { "horizontal" });
        }
    }
}

#[cfg(test)]
mod hop_acceptance {
    use super::*;

    /// Writes `target/hop-acceptance/style-N.vsdx`: `linejump-styles.vsdx` page N with the horizontal
    /// connector's geometry replaced by rows from `hop_rows` (absolute POLYLINE points). Open them
    /// in Visio (`hop-acceptance.ps1`) and compare the export with Visio's own hop drawing.
    #[cfg(private_fixtures)]
    #[test]
    fn write_acceptance_files() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/linejump-styles.vsdx");
        let package = Package::open(path).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/hop-acceptance");
        std::fs::create_dir_all(&out).unwrap();
        for style in [0usize, 2, 3, 4, 5, 6, 7, 8] {
            let editor = GroupEditor::open(&package, style).unwrap();
            let jump = LineJump { code: 1, style: style as i32, ..LineJump::default() };
            let connectors: Vec<&ResolvedShape> = editor.roots.iter().filter(|n| num(n, "BeginX").is_some()).collect();
            let bases: Vec<_> = connectors.iter().map(|n| stored_base_route(n).unwrap()).collect();
            let routes: Vec<Vec<(f64, f64)>> = bases.iter().map(|b| b.0.clone()).collect();
            let hops = compute_hops(&routes, &jump);
            let rows = hop_rows(&routes[0], &hops[0], bases[0].1, &jump);
            let patch = ShapeCellPatch::new(connectors[0].id.clone()).geometry_rows("0", rows);
            let bytes = crate::writer::patch_page_cells(&package, style, &[patch]).unwrap();
            std::fs::write(out.join(format!("style-{style}.vsdx")), bytes).unwrap();
        }
    }
}

#[cfg(test)]
mod hop_reroute_tests {
    use super::*;
    use std::io::Cursor;

    /// `linejump-glued-before.vsdx`: a horizontal connector hops over a vertical one (both glued
    /// to rectangles). In `-after` Visio moved shape 4 (the vertical connector's end) 0.7 in to
    /// the right, rerouted the vertical connector and moved the hop. Doing the same move here and
    /// rerouting must give the horizontal connector the same geometry rows.
    #[cfg(private_fixtures)]
    #[test]
    fn moving_a_shape_moves_the_hop_like_visio() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus");
        let before = Package::open(dir.join("linejump-glued-before.vsdx")).unwrap();
        let after = Package::open(dir.join("linejump-glued-after.vsdx")).unwrap();
        let pin = {
            let editor = GroupEditor::open(&before, 0).unwrap();
            let node = editor.roots.iter().find(|n| n.id == "4").unwrap();
            (num(node, "PinX").unwrap(), num(node, "PinY").unwrap())
        };
        let patch = ShapeCellPatch::new("4").set("PinX", format!("{}", pin.0 + 0.7)).set("PinY", format!("{}", pin.1));
        let moved = Package::from_reader(Cursor::new(crate::writer::patch_page_cells(&before, 0, &[patch]).unwrap())).unwrap();
        let part = moved.pages().unwrap()[0].part.clone();
        let connects = moved.connects(&part).unwrap();
        let patches = GroupEditor::open(&moved, 0).unwrap().reroute_connectors(&connects, &LineJump::default());
        let ours = patches.iter().find(|p| p.visio_id == "5").expect("the horizontal connector is rewritten");
        let rows = &ours.geometry_rows.as_ref().unwrap().1;
        let editor = GroupEditor::open(&after, 0).unwrap();
        let theirs = editor.roots.iter().find(|n| n.id == "5").unwrap();
        let stored = &theirs.sections.iter().find(|s| s.name == "Geometry").unwrap().rows;
        assert_eq!(rows.len(), stored.len());
        let value = |row: &crate::shapes::SectionRow, name: &str| row.cells[name].value.as_deref().unwrap().parse::<f64>().unwrap();
        for (a, b) in rows.iter().zip(stored) {
            assert_eq!(Some(a.kind.as_str()), b.row_type.as_deref());
            for name in ["X", "Y"] {
                let ours: f64 = a.cells[name].parse().unwrap();
                assert!((ours - value(b, name)).abs() < 2e-3, "{} {name}: {ours} vs {}", a.kind, value(b, name));
            }
        }
        // The vertical connector was rerouted too and must not hop over the horizontal one.
        let vertical = patches.iter().find(|p| p.visio_id == "6").expect("the vertical connector is rewritten");
        assert!(vertical.geometry_rows.as_ref().unwrap().1.iter().all(|r| r.kind != "ArcTo"));
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    /// A point of the ellipse with major axis angle `c` and axis ratio `d` (semi-axes d and 1) at parameter `t`.
    fn ellipse_point(c: f64, d: f64, t: f64) -> (f64, f64) {
        let (x, y) = (d * t.cos(), t.sin());
        (x * c.cos() - y * c.sin(), x * c.sin() + y * c.cos())
    }

    #[test]
    fn a_scaled_ellipse_has_the_axes_the_singular_values_give() {
        for (c, d, own) in [(0.0, 2.0, (2.0, 0.5)), (0.4, 2.5, (1.5, 0.6)), (1.2, 1.5, (0.7, 1.8)), (0.9, 1.0, (3.0, 1.0)), (2.5, 3.0, (0.5, 0.5))] {
            let (new_c, new_d) = scale_arc_axes(c, d, own).unwrap();
            // every scaled point of the old ellipse lies on the new one (x'/a)^2 + (y'/b)^2 = 1 in the new axes
            let mut a_sq = 0.0f64;
            let mut points = Vec::new();
            for k in 0..720 {
                let (x, y) = ellipse_point(c, d, k as f64 * 0.00872665);
                let (sx, sy) = (x * own.0, y * own.1);
                // coordinates along the new major / minor axis
                let u = sx * new_c.cos() + sy * new_c.sin();
                let v = -sx * new_c.sin() + sy * new_c.cos();
                a_sq = a_sq.max(u * u);
                points.push((u, v));
            }
            let b_sq = a_sq / (new_d * new_d);
            for (u, v) in points {
                let value = u * u / a_sq + v * v / b_sq;
                assert!((value - 1.0).abs() < 2e-4, "c {c} d {d} scale {own:?}: {value}");
            }
        }
    }

    #[test]
    fn polylines_scale_only_the_absolute_axes() {
        assert_eq!(scale_polyline("POLYLINE(0, 0, 1,1, 1,0, 0,0)", (2.0, 3.0)).unwrap(), "POLYLINE(0, 0, 1,1, 1,0, 0,0)");
        assert_eq!(scale_polyline("POLYLINE(1, 1, 0.5, 0.25, 1, 2)", (2.0, 3.0)).unwrap(), "POLYLINE(1, 1, 1, 0.75, 2, 6)");
        assert_eq!(scale_polyline("POLYLINE(1, 0, 0.5, 0.25)", (2.0, 3.0)).unwrap(), "POLYLINE(1, 0, 1, 0.25)");
        assert!(scale_polyline("POLYLINE(2, 0, 1, 1)", (2.0, 3.0)).is_none());
        assert!(scale_polyline("Width*1", (2.0, 3.0)).is_none());
    }

    #[test]
    fn a_turned_child_takes_the_factors_visio_gives_it() {
        // Visio: a 1 x 2 child turned 30 degrees in a group scaled 2 x 0.5 became 1.625 x 1.75
        let (a, b) = own_scale(30f64.to_radians(), (2.0, 0.5)).unwrap();
        assert!((a - 1.625).abs() < 1e-9 && (b * 2.0 - 1.75).abs() < 1e-9 + 0.0, "{a} {b}");
        // quarter and half turns
        assert_eq!(own_scale(0.0, (2.0, 0.5)).unwrap(), (2.0, 0.5));
        let (x, y) = own_scale(std::f64::consts::FRAC_PI_2, (2.0, 0.5)).unwrap();
        assert!((x - 0.5).abs() < 1e-9 && (y - 2.0).abs() < 1e-9);
    }
}

/// Multiply two 2 x 2 matrices.
fn mat_mul(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    [
        [a[0][0] * b[0][0] + a[0][1] * b[1][0], a[0][0] * b[0][1] + a[0][1] * b[1][1]],
        [a[1][0] * b[0][0] + a[1][1] * b[1][0], a[1][0] * b[0][1] + a[1][1] * b[1][1]],
    ]
}

/// Group page-level shapes: a new `Type='Group'` shape (the next free id) whose box is the page box the shapes cover, with
/// the shapes moved into its `<Shapes>` (their pins in the group's coordinates, their size and pin cells stated by formulas
/// on the group's `Width` / `Height` the way Visio's own groups do, so a resize of the group in Visio scales them). The
/// group takes the place of the topmost shape in the drawing order. Connectors glued to the shapes keep their glue (ids do
/// not change). Connectors in the selection go into the group too (their ends, pin and cached values move into the group's
/// coordinates), when every shape they are glued to is in the selection as well. Shapes not on the page itself are refused.
/// Returns the package and the new group's id.
pub fn group_shapes(package: &Package, page_index: usize, ids: &[String]) -> Result<(Vec<u8>, String)> {
    if ids.len() < 2 {
        return Err(Error("select at least two shapes to group them".to_string()));
    }
    let mut distinct = ids.to_vec();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != ids.len() {
        return Err(Error("a shape is listed twice".to_string()));
    }
    let editor = GroupEditor::open(package, page_index)?;
    let is_connector = |node: &ResolvedShape| ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|name| num(node, name).is_some());
    let mut shape_ids: Vec<String> = Vec::new();
    let mut connectors: Vec<&ResolvedShape> = Vec::new();
    // Members of groups (Visio: select them with the sub-select and Group): they leave their group and, with the other
    // shapes, form a new group on the page itself. Only groups that neither turn nor mirror their members are handled.
    let mut nested: Vec<(&ResolvedShape, Frame, String)> = Vec::new(); // (member, frame on the page, id of the top group)
    for id in ids {
        let chain = editor.chain(id).ok_or_else(|| Error(format!("shape {id} not found")))?;
        if chain.len() > 1 {
            let node = chain[chain.len() - 1];
            if is_connector(node) {
                return Err(Error(format!("shape {id} is a connector inside a group; it cannot be grouped with other shapes here")));
            }
            if chain[..chain.len() - 1].iter().any(|a| ids.contains(&a.id)) {
                return Err(Error(format!("shape {id} is inside a group that is selected too")));
            }
            let mut frame = frame_of(node)?;
            for ancestor in chain[..chain.len() - 1].iter().rev() {
                let af = frame_of(ancestor)?;
                if af.angle.abs() > 1e-9 || af.flip != (false, false) {
                    return Err(Error(format!("shape {id} is in a turned or flipped group; it cannot be grouped out of it here")));
                }
                frame.pin = af.to_parent(frame.pin);
            }
            let parent = chain[chain.len() - 2];
            let taken = ids.iter().filter(|other| parent.children.iter().any(|c| c.id == **other)).count();
            if taken >= parent.children.len() {
                return Err(Error(format!("grouping would take every member out of group {}", parent.id)));
            }
            nested.push((node, frame, chain[0].id.clone()));
            continue;
        }
        match editor.roots.iter().find(|n| n.id == *id) {
            Some(node) if is_connector(node) => connectors.push(node),
            _ => shape_ids.push(id.clone()),
        }
    }
    let mut frames = if shape_ids.is_empty() { Vec::new() } else { editor.page_level_frames(&shape_ids)? };
    let page_level_count = frames.len();
    for (node, frame, _) in &nested {
        frames.push((*node, *frame));
    }
    {
        let pages = package.pages()?;
        let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?;
        for connect in package.connects(&page.part)? {
            if connectors.iter().any(|c| c.id == connect.from_sheet) && !shape_ids.contains(&connect.to_sheet) {
                return Err(Error(format!("connector {} is glued to shape {}, which is not in the selection", connect.from_sheet, connect.to_sheet)));
            }
        }
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (_, frame) in &frames {
        let b = GroupEditor::page_extent(frame);
        x0 = x0.min(b.0);
        y0 = y0.min(b.1);
        x1 = x1.max(b.2);
        y1 = y1.max(b.3);
    }
    for c in &connectors {
        for (xn, yn) in [("BeginX", "BeginY"), ("EndX", "EndY")] {
            let (x, y) = (num(c, xn).unwrap_or(0.0), num(c, yn).unwrap_or(0.0));
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let gid = crate::writer::next_shape_id(package, page_index)?.to_string();
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone();
    let mut xml = package.text(&part)?.to_string();
    // the shapes' places in the page, in drawing order
    let mut items: Vec<(usize, usize, usize)> = Vec::new(); // (start, end, index into frames, then connectors after the frames)
    for (k, (node, _)) in frames.iter().enumerate() {
        let (s, e) = crate::writer::find_shape_range(&xml, &node.id).ok_or_else(|| Error(format!("shape {} not found", node.id)))?;
        items.push((s, e, k));
    }
    for (k, node) in connectors.iter().enumerate() {
        let (s, e) = crate::writer::find_shape_range(&xml, &node.id).ok_or_else(|| Error(format!("shape {} not found", node.id)))?;
        items.push((s, e, frames.len() + k));
    }
    items.sort();
    // where the new group goes: in front of the topmost page-level shape, or of the group a member was taken from
    let roots_of_nested: Vec<&str> = nested.iter().map(|(_, _, root)| root.as_str()).collect();
    let mut children = String::new();
    for (s, e, k) in &items {
        if *k >= frames.len() {
            // a connector: its ends and pin are in the parent's coordinates, so they move by the group's corner
            let node = connectors[*k - frames.len()];
            let mut patch = ShapeCellPatch::new(node.id.clone());
            for (name, delta) in [("BeginX", x0), ("BeginY", y0), ("EndX", x0), ("EndY", y0), ("PinX", x0), ("PinY", y0)] {
                if let Some(value) = num(node, name) {
                    let formula = node.cells.get(name).and_then(|c| c.formula.clone()).filter(|f| !f.is_empty() && f != "Inh" && f != "No Formula");
                    patch = match formula {
                        Some(f) => patch.set_with_formula(name, fmt(value - delta), f),
                        None => patch.set(name, fmt(value - delta)),
                    };
                }
            }
            children.push_str(&crate::writer::patch_shape_xml(&xml[*s..*e], &patch)?);
            continue;
        }
        let (node, frame) = &frames[*k];
        let local = (frame.pin.0 - x0, frame.pin.1 - y0);
        let mut patch = ShapeCellPatch::new(node.id.clone());
        let pairs = [("PinX", local.0, "Width", w), ("PinY", local.1, "Height", h)];
        for (name, value, axis, extent) in pairs {
            patch = if extent > EPS { patch.set_with_formula(name, fmt(value), format!("Sheet.{gid}!{axis}*{}", fmt(value / extent))) } else { patch.set(name, fmt(value)) };
        }
        if w > EPS {
            patch = patch.set_with_formula("Width", fmt(frame.size.0), format!("Sheet.{gid}!Width*{}", fmt(frame.size.0 / w)));
        }
        if h > EPS {
            patch = patch.set_with_formula("Height", fmt(frame.size.1), format!("Sheet.{gid}!Height*{}", fmt(frame.size.1 / h)));
        }
        children.push_str(&crate::writer::patch_shape_xml(&xml[*s..*e], &patch)?);
    }
    // take the shapes out (last first) and put the group where the topmost one was
    let removed: Vec<(usize, usize)> = items.iter().map(|(s, e, _)| (*s, *e)).collect();
    for (s, e, _) in items.iter().rev() {
        xml.replace_range(*s..*e, "");
    }
    let shifted = |start: usize| start - removed.iter().filter(|(s, _)| *s < start).map(|(s, e)| e - s).sum::<usize>();
    let mut insert_at = 0usize;
    for (s, e, k) in &items {
        let _ = e;
        if *k < page_level_count || *k >= frames.len() {
            insert_at = insert_at.max(shifted(*s));
        }
    }
    for root in &roots_of_nested {
        let (_, end) = crate::writer::find_shape_range(&xml, root).ok_or_else(|| Error(format!("shape {root} not found")))?;
        insert_at = insert_at.max(end);
    }
    let group = format!(
        "<Shape ID='{gid}' NameU='Group.{gid}' Name='Group.{gid}' Type='Group' LineStyle='0' FillStyle='0' TextStyle='0'><Cell N='PinX' V='{}'/><Cell N='PinY' V='{}'/><Cell N='Width' V='{}'/><Cell N='Height' V='{}'/><Cell N='LocPinX' V='{}' F='Width*0.5'/><Cell N='LocPinY' V='{}' F='Height*0.5'/><Cell N='Angle' V='0'/><Cell N='FlipX' V='0'/><Cell N='FlipY' V='0'/><Cell N='ResizeMode' V='0'/><Shapes>{children}</Shapes></Shape>",
        fmt(x0 + w / 2.0),
        fmt(y0 + h / 2.0),
        fmt(w),
        fmt(h),
        fmt(w / 2.0),
        fmt(h / 2.0),
    );
    xml.insert_str(insert_at, &group);
    let mut extras = BTreeMap::new();
    extras.insert(part, xml.into_bytes());
    Ok((crate::writer::write_package(package, extras)?, gid))
}

/// How `reorder_shapes` moves shapes in the drawing order (the order of `<Shape>` elements among their siblings: later is in front).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZOrder {
    ToFront,
    ToBack,
    Forward,
    Backward,
}

/// Change the drawing order of the shapes `ids` (each among its own siblings, on the page or inside its group; connectors are shapes
/// like the others): to the front or back of the siblings, or one place forward / backward past a sibling that is not selected.
/// The selected shapes keep their order among themselves. Returns the new package bytes.
pub fn reorder_shapes(package: &Package, page_index: usize, ids: &[String], order: ZOrder) -> Result<Vec<u8>> {
    if ids.is_empty() {
        return Err(Error("nothing selected".to_string()));
    }
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone();
    let xml = package.text(&part)?.to_string();
    let doc = roxmltree::Document::parse(&xml).map_err(|e| Error(format!("page xml: {e}")))?;
    let shape_by_id = |id: &str| doc.descendants().find(|n| n.is_element() && n.tag_name().name() == "Shape" && n.attribute("ID") == Some(id));
    // the sibling lists (as ranges of the text), once per parent
    let mut lists: Vec<(Vec<std::ops::Range<usize>>, Vec<bool>)> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for id in ids {
        let node = shape_by_id(id).ok_or_else(|| Error(format!("shape {id} not found")))?;
        let parent = node.parent().filter(|p| p.is_element()).ok_or_else(|| Error(format!("shape {id} has no parent")))?;
        let key = parent.range().start;
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let siblings: Vec<roxmltree::Node> = parent.children().filter(|n| n.is_element() && n.tag_name().name() == "Shape").collect();
        let selected: Vec<bool> = siblings.iter().map(|s| s.attribute("ID").is_some_and(|sid| ids.iter().any(|i| i == sid))).collect();
        lists.push((siblings.iter().map(|s| s.range()).collect(), selected));
    }
    let mut replacements: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for (ranges, mut selected) in lists {
        let mut index: Vec<usize> = (0..ranges.len()).collect();
        match order {
            ZOrder::ToFront => index.sort_by_key(|i| (selected[*i], *i)),
            ZOrder::ToBack => index.sort_by_key(|i| (!selected[*i], *i)),
            ZOrder::Forward => {
                for i in (0..index.len().saturating_sub(1)).rev() {
                    if selected[i] && !selected[i + 1] {
                        index.swap(i, i + 1);
                        selected.swap(i, i + 1);
                    }
                }
            }
            ZOrder::Backward => {
                for i in 1..index.len() {
                    if selected[i] && !selected[i - 1] {
                        index.swap(i, i - 1);
                        selected.swap(i, i - 1);
                    }
                }
            }
        }
        if index.iter().enumerate().all(|(position, i)| position == *i) {
            continue;
        }
        let (first, last) = (ranges[0].start, ranges[ranges.len() - 1].end);
        let joined: String = index.iter().map(|i| &xml[ranges[*i].clone()]).collect();
        replacements.push((first..last, joined));
    }
    if replacements.is_empty() {
        return crate::writer::write_package(package, BTreeMap::new());
    }
    let mut out = xml.clone();
    replacements.sort_by_key(|(range, _)| range.start);
    for (range, text) in replacements.into_iter().rev() {
        out.replace_range(range, &text);
    }
    let mut extras = BTreeMap::new();
    extras.insert(part, out.into_bytes());
    crate::writer::write_package(package, extras)
}

/// Ungroup a plain group: its members go back to the container it sits in (the page, or - for a group inside another group -
/// that group), in the group's place in the drawing order, with their pins, angles and mirrors composed with the group's
/// (`decompose`) and the formulas that tied them to the group dropped. Connectors of the group go back by the group's offset (a
/// turned or flipped group holding connectors is refused, like a group that is an instance of a master: its members come from the
/// master). Returns the package and the ids of the freed shapes.
///
/// A group INSIDE another group (Visio, probed on groups-nested.vsdx): the members go into the outer group, and a member whose
/// size and place were formulas of the inner group (`Sheet.<inner>!Width*0.2`) gets them against the outer group
/// (`Sheet.<outer>!Width*k`, k = value / outer size) when the inner group only translates. At any depth a group that has text (or
/// geometry) of its own stays behind as an ordinary shape after its members (Type 'Shape') - Visio keeps it; a bare group is removed.
pub fn ungroup_shape(package: &Package, page_index: usize, id: &str) -> Result<(Vec<u8>, Vec<String>)> {
    let editor = GroupEditor::open(package, page_index)?;
    let chain = editor.chain(id).ok_or_else(|| Error(format!("no shape {id}")))?;
    let group = chain[chain.len() - 1];
    let parent = (chain.len() >= 2).then(|| chain[chain.len() - 2]);
    if let Some(parent) = parent {
        if parent.master_id.is_some() || !parent.master_children.is_empty() {
            return Err(Error("a group inside an instance of a master cannot be ungrouped here".to_string()));
        }
    }
    if group.shape_type.as_deref() != Some("Group") || group.children.is_empty() {
        return Err(Error(format!("shape {id} is not a group of shapes")));
    }
    if group.master_id.is_some() || !group.master_children.is_empty() {
        return Err(Error("a group that is an instance of a master cannot be ungrouped here".to_string()));
    }
    let is_connector = |node: &ResolvedShape| ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|name| num(node, name).is_some());
    let gframe = frame_of(group)?;
    // Connectors (1-D shapes) measure their ends in the group's coordinates: they go back to the page by the group's offset. In a
    // turned or flipped group Visio (probed) moves the two ends through the group's transform and keeps the connector itself
    // unturned and unmirrored: Pin = middle of the ends, Width / Height = End - Begin (signed).
    let group_moves_points = gframe.angle.abs() > 1e-9 || gframe.flip != (false, false);
    if let Some(c) = group.children.iter().find(|c| is_connector(c)) {
        if group_moves_points && (num(c, "Angle").unwrap_or(0.0).abs() > EPS || num(c, "FlipX").unwrap_or(0.0) != 0.0 || num(c, "FlipY").unwrap_or(0.0) != 0.0) {
            return Err(Error(format!("the connector {} is itself turned or mirrored inside a turned or mirrored group; it cannot be moved back to the page here", c.id)));
        }
    }
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone();
    let mut xml = package.text(&part)?.to_string();
    let (gs, ge) = crate::writer::find_shape_range(&xml, id).ok_or_else(|| Error(format!("shape {id} not found")))?;
    let group_xml = xml[gs..ge].to_string();
    let mut freed = String::new();
    let mut ids = Vec::new();
    for child in &group.children {
        if is_connector(child) {
            let offset = gframe.to_parent((0.0, 0.0));
            let mut patch = ShapeCellPatch::new(child.id.clone());
            if group_moves_points {
                let ends: Option<Vec<f64>> = ["BeginX", "BeginY", "EndX", "EndY"].iter().map(|name| num(child, name)).collect();
                let ends = ends.ok_or_else(|| Error(format!("connector {} has no ends", child.id)))?;
                let (b, e) = (gframe.to_parent((ends[0], ends[1])), gframe.to_parent((ends[2], ends[3])));
                let values = [("BeginX", b.0), ("BeginY", b.1), ("EndX", e.0), ("EndY", e.1), ("PinX", (b.0 + e.0) / 2.0), ("PinY", (b.1 + e.1) / 2.0), ("Width", e.0 - b.0), ("Height", e.1 - b.1), ("Angle", 0.0), ("FlipX", 0.0), ("FlipY", 0.0)];
                for (name, value) in values {
                    let formula = child.cells.get(name).and_then(|c| c.formula.clone()).filter(|f| !f.is_empty() && f != "Inh" && f != "No Formula" && !f.starts_with("Sheet."));
                    patch = match formula {
                        Some(f) => patch.set_with_formula(name, fmt(value), f),
                        None => patch.set(name, fmt(value)),
                    };
                }
                let (cs, ce) = crate::writer::find_shape_range(&group_xml, &child.id).ok_or_else(|| Error(format!("member {} not found", child.id)))?;
                freed.push_str(&crate::writer::patch_shape_xml(&group_xml[cs..ce], &patch)?);
                ids.push(child.id.clone());
                continue;
            }
            for (name, delta) in [("BeginX", offset.0), ("BeginY", offset.1), ("EndX", offset.0), ("EndY", offset.1), ("PinX", offset.0), ("PinY", offset.1)] {
                if let Some(value) = num(child, name) {
                    let formula = child.cells.get(name).and_then(|c| c.formula.clone()).filter(|f| !f.is_empty() && f != "Inh" && f != "No Formula");
                    patch = match formula {
                        Some(f) => patch.set_with_formula(name, fmt(value + delta), f),
                        None => patch.set(name, fmt(value + delta)),
                    };
                }
            }
            let (cs, ce) = crate::writer::find_shape_range(&group_xml, &child.id).ok_or_else(|| Error(format!("member {} not found", child.id)))?;
            freed.push_str(&crate::writer::patch_shape_xml(&group_xml[cs..ce], &patch)?);
            ids.push(child.id.clone());
            continue;
        }
        let cframe = frame_of(child)?;
        let pin = gframe.to_parent(cframe.pin);
        let (angle, mirrored) = decompose(mat_mul(gframe.linear(), cframe.linear()));
        let mut patch = ShapeCellPatch::new(child.id.clone())
            .set("PinX", fmt(pin.0))
            .set("PinY", fmt(pin.1))
            .set("Angle", fmt(angle.rem_euclid(std::f64::consts::TAU)))
            .set("FlipX", if mirrored { "1" } else { "0" })
            .set("FlipY", "0");
        // Into an outer group: what was tied to the inner group is tied to the outer one (an inner group that only translates).
        if let (Some(outer), false) = (parent, group_moves_points) {
            let oframe = frame_of(outer)?;
            let tied = |name: &str| child.cells.get(name).and_then(|c| c.formula.as_deref()).is_some_and(|f| f.starts_with(&format!("Sheet.{id}!")));
            let cells = [("PinX", pin.0, "Width", oframe.size.0), ("PinY", pin.1, "Height", oframe.size.1), ("Width", cframe.size.0, "Width", oframe.size.0), ("Height", cframe.size.1, "Height", oframe.size.1)];
            for (name, value, axis, extent) in cells {
                if tied(name) && extent > EPS {
                    patch = patch.set_with_formula(name, fmt(value), format!("Sheet.{}!{axis}*{}", outer.id, fmt(value / extent)));
                }
            }
        }
        let (cs, ce) = crate::writer::find_shape_range(&group_xml, &child.id).ok_or_else(|| Error(format!("member {} not found", child.id)))?;
        let mut child_xml = crate::writer::patch_shape_xml(&group_xml[cs..ce], &patch)?;
        // the formulas that tied the member to the group (`Sheet.<group>!Width*0.3`) are dropped, the cached values stay
        for quote in ['\'', '"'] {
            let needle = format!(" F={quote}Sheet.{id}!");
            while let Some(at) = child_xml.find(&needle) {
                let end = child_xml[at + needle.len()..].find(quote).map(|n| at + needle.len() + n + 1).unwrap_or(child_xml.len());
                child_xml.replace_range(at..end, "");
            }
        }
        freed.push_str(&child_xml);
        ids.push(child.id.clone());
    }
    if group.has_text || group.own_sections.iter().any(|s| s.name == "Geometry") {
        // the group's own text / geometry: it stays, without its members, as an ordinary shape after them (Visio, probed for text at
        // the page and in a group: probe-ungroup-text.ps1, probe-ungroup-nested.ps1; own geometry is assumed to behave the same)
        let mut kept = group_xml.clone();
        if let (Some(a), Some(b)) = (kept.find("<Shapes>"), kept.rfind("</Shapes>")) {
            kept.replace_range(a..b + "</Shapes>".len(), "");
        }
        for quote in ['\'', '"'] {
            kept = kept.replacen(&format!("Type={quote}Group{quote}"), &format!("Type={quote}Shape{quote}"), 1);
        }
        freed.push_str(&kept);
    }
    xml.replace_range(gs..ge, &freed);
    let mut extras = BTreeMap::new();
    extras.insert(part, xml.into_bytes());
    Ok((crate::writer::write_package(package, extras)?, ids))
}
