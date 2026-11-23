//! Shapes drawn from scratch (no master): rectangle, rounded rectangle, ellipse, diamond, triangle, text box
//! and line, written the way Visio writes the shapes of its drawing tools (probe-draw-and-pages.ps1, measured on Visio 16.0.14332: DrawRectangle,
//! DrawOval, DrawLine on a blank drawing, SaveAsEx): a `Shape` with its frame cells (`LocPinX = Width*0.5`),
//! a `Geometry` section whose coordinates are formulas of `Width` and `Height` (so the shape scales when it is
//! resized), an `Ellipse` row for the ellipse, `Rounding` for the rounded rectangle and, for the line, a 1-D shape
//! with `BeginX/BeginY/EndX/EndY`. Line and fill are written as cells (a blank drawing has only the "No Style"
//! sheet), and every closed shape gets four connection points so connectors can be glued to it.
use crate::writer::{next_shape_id, num_text, NewPicture};
use crate::{Error, Package, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawKind {
    Rectangle,
    Rounded,
    Ellipse,
    Diamond,
    Triangle,
    TextBox,
    Line,
}

impl DrawKind {
    pub fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::Rectangle,
            1 => Self::Rounded,
            2 => Self::Ellipse,
            3 => Self::Diamond,
            4 => Self::Triangle,
            5 => Self::TextBox,
            6 => Self::Line,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Rounded => "Rounded Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Diamond => "Diamond",
            Self::Triangle => "Triangle",
            Self::TextBox => "Text Box",
            Self::Line => "Line",
        }
    }
}

const LINE_WEIGHT: &str = "0.01388888888888889"; // 1 pt
const FONT_SIZE: &str = "0.1388888888888889"; // 10 pt

fn row(kind: &str, index: usize, x: (f64, &str), y: (f64, &str)) -> String {
    format!(
        "<Row T='{kind}' IX='{index}'><Cell N='X' V='{}' F='Width*{}'/><Cell N='Y' V='{}' F='Height*{}'/></Row>",
        num_text(x.0),
        x.1,
        num_text(y.0),
        y.1
    )
}

/// A polygon through the given corner points (fractions of the box), closed back to the first.
pub(crate) fn polygon(points: &[(f64, f64)], w: f64, h: f64) -> String {
    let frac = |v: f64| num_text(v);
    let mut out = String::new();
    for (i, (fx, fy)) in points.iter().enumerate() {
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        out.push_str(&row(kind, i + 1, (fx * w, &frac(*fx)), (fy * h, &frac(*fy))));
    }
    out.push_str(&format!(
        "<Row T='LineTo' IX='{}'><Cell N='X' V='{}' F='Geometry1.X1'/><Cell N='Y' V='{}' F='Geometry1.Y1'/></Row>",
        points.len() + 1,
        num_text(points[0].0 * w),
        num_text(points[0].1 * h)
    ));
    out
}

/// The `Connection` rows (fractions of the box, direction INTO the shape as Visio stores it).
pub(crate) fn connection_rows(points: &[((f64, f64), (i32, i32))], w: f64, h: f64) -> String {
    let mut out = String::from("<Section N='Connection'>");
    for (i, ((fx, fy), (dx, dy))) in points.iter().enumerate() {
        out.push_str(&format!(
            "<Row T='Connection' IX='{i}'><Cell N='X' V='{}' F='Width*{}'/><Cell N='Y' V='{}' F='Height*{}'/><Cell N='DirX' V='{dx}'/><Cell N='DirY' V='{dy}'/><Cell N='Type' V='0'/><Cell N='AutoGen' V='0'/></Row>",
            num_text(fx * w),
            num_text(*fx),
            num_text(fy * h),
            num_text(*fy)
        ));
    }
    out.push_str("</Section>");
    out
}

pub(crate) const SIDES: [((f64, f64), (i32, i32)); 4] = [((0.5, 1.0), (0, -1)), ((1.0, 0.5), (-1, 0)), ((0.5, 0.0), (0, 1)), ((0.0, 0.5), (1, 0))];

/// The id of the document's theme-based "Normal" style: what Visio gives every new shape (`LineStyle` = `FillStyle` = `TextStyle` = that id; the style chain ends
/// in the "Theme" style whose cells are `THEMEVAL()` formulas), so a shape drawn here changes its colours when a theme is applied (Q7, round 12). `None` when the
/// document has no such style (a file from another writer): shapes then keep explicit cells and style 0.
pub(crate) fn themed_style(package: &Package) -> Option<u32> {
    let document = package.text("visio/document.xml").ok()?;
    if !document.contains("THEMEVAL") {
        return None;
    }
    let at = document.find("NameU='Normal'")?;
    let open = document[..at].rfind("<StyleSheet ")?;
    let tag = &document[open..at];
    let id = tag.split("ID='").nth(1)?.split('\'').next()?;
    id.parse::<u32>().ok()
}

/// The cells Visio writes on every shape it draws: matrix 1 = the theme's first line / fill / effect / font style (without them Visio applies no theme style list).
pub(crate) const QUICK_STYLE: &str = "<Cell N='QuickStyleLineMatrix' V='1'/><Cell N='QuickStyleFillMatrix' V='1'/><Cell N='QuickStyleEffectsMatrix' V='1'/><Cell N='QuickStyleFontMatrix' V='1'/>";

fn style_attrs(style: Option<u32>) -> String {
    let s = style.unwrap_or(0);
    format!("LineStyle='{s}' FillStyle='{s}' TextStyle='{s}'")
}

/// The XML of a shape of `kind` filling the box from (`x0`, `y0`) to (`x1`, `y1`) (page inches, y up), with the
/// given ID. A line runs from the first point to the second.
fn shape_xml(id: u64, kind: DrawKind, x0: f64, y0: f64, x1: f64, y1: f64, style: Option<u32>) -> String {
    let name = format!("{}.{id}", kind.name());
    let styles = style_attrs(style);
    let quick = if style.is_some() { QUICK_STYLE } else { "" };
    let line_cells = if style.is_some() { String::new() } else { format!("<Cell N='LineColor' V='#000000'/><Cell N='LinePattern' V='1'/><Cell N='LineWeight' V='{LINE_WEIGHT}'/>") };
    if kind == DrawKind::Line {
        let (w, h) = (x1 - x0, y1 - y0);
        return format!(
            "<Shape ID='{id}' NameU='{name}' Name='{name}' Type='Shape' {styles}>\
<Cell N='PinX' V='{}' F='(BeginX+EndX)/2'/><Cell N='PinY' V='{}' F='(BeginY+EndY)/2'/>\
<Cell N='Width' V='{}' F='GUARD(EndX-BeginX)'/><Cell N='Height' V='{}' F='GUARD(EndY-BeginY)'/>\
<Cell N='LocPinX' V='{}' F='Width*0.5'/><Cell N='LocPinY' V='{}' F='Height*0.5'/><Cell N='Angle' V='0'/><Cell N='FlipX' V='0'/><Cell N='FlipY' V='0'/><Cell N='ResizeMode' V='0'/>{quick}\
<Cell N='BeginX' V='{}'/><Cell N='BeginY' V='{}'/><Cell N='EndX' V='{}'/><Cell N='EndY' V='{}'/>\
{line_cells}<Cell N='FillPattern' V='0'/><Cell N='ObjType' V='2'/>\
<Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/><Cell N='NoLine' V='0'/><Cell N='NoShow' V='0'/><Cell N='NoSnap' V='0'/><Cell N='NoQuickDrag' V='0'/>\
<Row T='MoveTo' IX='1'><Cell N='X' V='0'/><Cell N='Y' V='0'/></Row>\
<Row T='LineTo' IX='2'><Cell N='X' V='{}' F='Width*1'/><Cell N='Y' V='{}' F='Height*1'/></Row></Section></Shape>",
            num_text(x0 + w / 2.0),
            num_text(y0 + h / 2.0),
            num_text(w),
            num_text(h),
            num_text(w / 2.0),
            num_text(h / 2.0),
            num_text(x0),
            num_text(y0),
            num_text(x1),
            num_text(y1),
            num_text(w),
            num_text(h),
        );
    }
    let quick = ""; // closed shapes: see closed_shape_xml
    let (left, right) = (x0.min(x1), x0.max(x1));
    let (bottom, top) = (y0.min(y1), y0.max(y1));
    let (w, h) = (right - left, top - bottom);
    let (fill, line) = match kind {
        DrawKind::TextBox => ("<Cell N='FillPattern' V='0'/>", "<Cell N='LinePattern' V='0'/>"),
        _ if style.is_some() => ("", ""),
        _ => ("<Cell N='FillForegnd' V='#ffffff'/><Cell N='FillPattern' V='1'/>", "<Cell N='LineColor' V='#000000'/><Cell N='LinePattern' V='1'/>"),
    };
    let (weight, colour) = if style.is_some() { (String::new(), String::new()) } else { (format!("<Cell N='LineWeight' V='{LINE_WEIGHT}'/>"), "<Cell N='Color' V='#000000'/>".to_string()) };
    let rounding = if kind == DrawKind::Rounded { format!("<Cell N='Rounding' V='{}' U='IN'/>", num_text((0.15f64).min(w.min(h) / 3.0))) } else { String::new() };
    let (geometry, connections) = match kind {
        DrawKind::Ellipse => (
            format!(
                "<Row T='Ellipse' IX='1'><Cell N='X' V='{}' F='Width*0.5'/><Cell N='Y' V='{}' F='Height*0.5'/><Cell N='A' V='{}' U='DL' F='Width*1'/><Cell N='B' V='{}' U='DL' F='Height*0.5'/><Cell N='C' V='{}' U='DL' F='Width*0.5'/><Cell N='D' V='{}' U='DL' F='Height*1'/></Row>",
                num_text(w / 2.0),
                num_text(h / 2.0),
                num_text(w),
                num_text(h / 2.0),
                num_text(w / 2.0),
                num_text(h)
            ),
            connection_rows(&SIDES, w, h),
        ),
        DrawKind::Diamond => (polygon(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)], w, h), connection_rows(&SIDES, w, h)),
        DrawKind::Triangle => (
            polygon(&[(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)], w, h),
            connection_rows(&[((0.5, 1.0), (0, -1)), ((0.75, 0.5), (-1, 0)), ((0.5, 0.0), (0, 1)), ((0.25, 0.5), (1, 0))], w, h),
        ),
        _ => (polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], w, h), connection_rows(&SIDES, w, h)),
    };
    format!(
        "<Shape ID='{id}' NameU='{name}' Name='{name}' Type='Shape' {styles}>\
<Cell N='PinX' V='{}'/><Cell N='PinY' V='{}'/><Cell N='Width' V='{}'/><Cell N='Height' V='{}'/>\
<Cell N='LocPinX' V='{}' F='Width*0.5'/><Cell N='LocPinY' V='{}' F='Height*0.5'/><Cell N='Angle' V='0'/><Cell N='FlipX' V='0'/><Cell N='FlipY' V='0'/><Cell N='ResizeMode' V='0'/>{quick}\
{fill}{line}{weight}{rounding}\
<Section N='Character'><Row IX='0'><Cell N='Size' V='{FONT_SIZE}'/>{colour}</Row></Section><Section N='Paragraph'><Row IX='0'><Cell N='HorzAlign' V='1'/></Row></Section>\
<Section N='Geometry' IX='0'><Cell N='NoFill' V='0'/><Cell N='NoLine' V='0'/><Cell N='NoShow' V='0'/><Cell N='NoSnap' V='0'/><Cell N='NoQuickDrag' V='0'/>{geometry}</Section>\
{connections}</Shape>",
        num_text(left + w / 2.0),
        num_text(bottom + h / 2.0),
        num_text(w),
        num_text(h),
        num_text(w / 2.0),
        num_text(h / 2.0),
    )
}

/// The XML of a closed shape: frame cells, line and fill, the given geometry and connection rows.
pub(crate) fn closed_shape_xml(id: u64, name: &str, left: f64, bottom: f64, w: f64, h: f64, text_box: bool, rounding: Option<f64>, geometry: &str, connections: &str, style: Option<u32>) -> String {
    let styles = style_attrs(style);
    let quick = ""; // Visio writes no QuickStyle cells on a drawn closed shape (probe, step 264: with them the fill resolved to the theme's first fill style, white in Integral)
    let (weight, colour) = if style.is_some() { (String::new(), String::new()) } else { (format!("<Cell N='LineWeight' V='{LINE_WEIGHT}'/>"), "<Cell N='Color' V='#000000'/>".to_string()) };
    let (fill, line) = if text_box {
        ("<Cell N='FillPattern' V='0'/>", "<Cell N='LinePattern' V='0'/>")
    } else if style.is_some() {
        ("", "")
    } else {
        ("<Cell N='FillForegnd' V='#ffffff'/><Cell N='FillPattern' V='1'/>", "<Cell N='LineColor' V='#000000'/><Cell N='LinePattern' V='1'/>")
    };
    let rounding = rounding.map(|r| format!("<Cell N='Rounding' V='{}' U='IN'/>", num_text(r))).unwrap_or_default();
    format!(
        "<Shape ID='{id}' NameU='{name}' Name='{name}' Type='Shape' {styles}>\
<Cell N='PinX' V='{}'/><Cell N='PinY' V='{}'/><Cell N='Width' V='{}'/><Cell N='Height' V='{}'/>\
<Cell N='LocPinX' V='{}' F='Width*0.5'/><Cell N='LocPinY' V='{}' F='Height*0.5'/><Cell N='Angle' V='0'/><Cell N='FlipX' V='0'/><Cell N='FlipY' V='0'/><Cell N='ResizeMode' V='0'/>{quick}\
{fill}{line}{weight}{rounding}\
<Section N='Character'><Row IX='0'><Cell N='Size' V='{FONT_SIZE}'/>{colour}</Row></Section><Section N='Paragraph'><Row IX='0'><Cell N='HorzAlign' V='1'/></Row></Section>\
<Section N='Geometry' IX='0'><Cell N='NoFill' V='0'/><Cell N='NoLine' V='0'/><Cell N='NoShow' V='0'/><Cell N='NoSnap' V='0'/><Cell N='NoQuickDrag' V='0'/>{geometry}</Section>\
{connections}</Shape>",
        num_text(left + w / 2.0),
        num_text(bottom + h / 2.0),
        num_text(w),
        num_text(h),
        num_text(w / 2.0),
        num_text(h / 2.0),
    )
}

/// The XML of a line shape from (`x0`, `y0`) to (`x1`, `y1`) named `name`.
pub(crate) fn line_shape_xml(id: u64, name: &str, x0: f64, y0: f64, x1: f64, y1: f64, style: Option<u32>) -> String {
    let mut xml = shape_xml(id, DrawKind::Line, x0, y0, x1, y1, style);
    let default_name = format!("Line.{id}");
    xml = xml.replace(&format!("NameU='{default_name}' Name='{default_name}'"), &format!("NameU='{name}' Name='{name}'"));
    xml
}

/// Draw a shape of `kind` on page `page_index`: the box from (`x0`, `y0`) to (`x1`, `y1`) in page inches (y up);
/// a line goes from the first point to the second. Returns the new package bytes and the new shape's ID.
pub fn draw_shape(package: &Package, page_index: usize, kind: DrawKind, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<(Vec<u8>, String)> {
    if ![x0, y0, x1, y1].iter().all(|v| v.is_finite() && v.abs() < 10_000.0) {
        return Err(Error("the shape's position is not a usable number".to_string()));
    }
    let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
    if kind == DrawKind::Line {
        if (dx * dx + dy * dy).sqrt() < 0.05 {
            return Err(Error("a line must be at least 0.05 in long".to_string()));
        }
    } else if dx < 0.05 || dy < 0.05 {
        return Err(Error("a shape must be at least 0.05 in wide and high".to_string()));
    }
    if dx > 200.0 || dy > 200.0 {
        return Err(Error("a shape can be at most 200 in wide and high".to_string()));
    }
    let id = next_shape_id(package, page_index)?;
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let mut xml = package.text(&page.part)?.to_string();
    let shape = shape_xml(id, kind, x0, y0, x1, y1, themed_style(package));
    insert_shape(package, &page.part, xml, &shape, id)
}

/// Draw a freeform shape through `points` (page inches, y up): an open polyline, or a closed polygon when `closed` (then the last point is joined to the first
/// by a closing `LineTo` that follows the first corner, as the other drawn polygons do). The box is the box of the points, the rows are `MoveTo` / `LineTo` with
/// `Width*f` / `Height*f` formulas, i.e. the form the edit-points editor reads and writes. Returns the new package bytes and the new shape's ID.
pub fn draw_freeform(package: &Package, page_index: usize, points: &[(f64, f64)], closed: bool) -> Result<(Vec<u8>, String)> {
    if !points.iter().all(|p| p.0.is_finite() && p.1.is_finite() && p.0.abs() < 10_000.0 && p.1.abs() < 10_000.0) {
        return Err(Error("a point is not a usable number".to_string()));
    }
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for p in points {
        if pts.last().map_or(true, |q| (q.0 - p.0).abs() > 1e-6 || (q.1 - p.1).abs() > 1e-6) {
            pts.push(*p);
        }
    }
    if closed && pts.len() > 1 && (pts[0].0 - pts[pts.len() - 1].0).abs() < 1e-6 && (pts[0].1 - pts[pts.len() - 1].1).abs() < 1e-6 {
        pts.pop(); // the last point sits on the first: the closing row joins them
    }
    if pts.len() < if closed { 3 } else { 2 } || pts.len() > 2000 {
        return Err(Error(format!("a {} needs {} to 2000 distinct points", if closed { "polygon" } else { "polyline" }, if closed { 3 } else { 2 })));
    }
    let (min_x, max_x) = pts.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (min_y, max_y) = pts.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let (w, h) = (max_x - min_x, max_y - min_y);
    if w < 1e-4 && h < 1e-4 {
        return Err(Error("the points are all in one place".to_string()));
    }
    if w > 200.0 || h > 200.0 {
        return Err(Error("a shape can be at most 200 in wide and high".to_string()));
    }
    let frac = |v: f64, size: f64| if size < 1e-4 { 0.0 } else { v / size };
    let mut geometry = String::new();
    for (i, p) in pts.iter().enumerate() {
        let (lx, ly) = (p.0 - min_x, p.1 - min_y);
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        geometry.push_str(&row(kind, i + 1, (lx, &num_text(frac(lx, w))), (ly, &num_text(frac(ly, h)))));
    }
    if closed {
        geometry.push_str(&format!(
            "<Row T='LineTo' IX='{}'><Cell N='X' V='{}' F='Geometry1.X1'/><Cell N='Y' V='{}' F='Geometry1.Y1'/></Row>",
            pts.len() + 1,
            num_text(pts[0].0 - min_x),
            num_text(pts[0].1 - min_y)
        ));
    }
    let id = next_shape_id(package, page_index)?;
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let xml = package.text(&page.part)?.to_string();
    let name = format!("{}.{id}", if closed { "Polygon" } else { "Polyline" });
    let mut shape = closed_shape_xml(id, &name, min_x, min_y, w, h, false, None, &geometry, "", themed_style(package));
    if !closed {
        shape = shape.replacen("<Section N='Geometry' IX='0'><Cell N='NoFill' V='0'/>", "<Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/>", 1);
    }
    insert_shape(package, &page.part, xml, &shape, id)
}

/// Draw a smooth curve through `points` (page inches, 4 to 200): the spline Visio's DrawSpline writes, one `MoveTo` and one `NURBSTo` row (groupedit/nurbs.rs). The curve
/// passes through every point; the box is the true extent of the curve. Returns the new package bytes and the new shape's ID.
pub fn draw_curve(package: &Package, page_index: usize, points: &[(f64, f64)]) -> Result<(Vec<u8>, String)> {
    if !points.iter().all(|p| p.0.is_finite() && p.1.is_finite() && p.0.abs() < 10_000.0 && p.1.abs() < 10_000.0) {
        return Err(Error("a point is not a usable number".to_string()));
    }
    let ((min_x, min_y), (w, h), rows) = crate::groupedit::vertices::fitted_curve_rows(points)?;
    if w > 200.0 || h > 200.0 {
        return Err(Error("a shape can be at most 200 in wide and high".to_string()));
    }
    let id = next_shape_id(package, page_index)?;
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let xml = package.text(&page.part)?.to_string();
    let shape = closed_shape_xml(id, &format!("Freeform.{id}"), min_x, min_y, w, h, false, None, "", "", themed_style(package))
        .replacen("<Section N='Geometry' IX='0'><Cell N='NoFill' V='0'/>", "<Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/>", 1);
    let (bytes, id) = insert_shape(package, &page.part, xml, &shape, id)?;
    let with_shape = Package::from_bytes(&bytes)?;
    let patched = crate::writer::patch_page_cells(&with_shape, page_index, &[crate::writer::ShapeCellPatch::new(id.clone()).geometry_rows("0", rows)])?;
    Ok((patched, id))
}

/// Draw a circular arc from `a` to `b` (page inches) bowed by `bow` inches (the signed sagitta: positive bulges to the right of the direction a -> b): a freeform of two
/// vertices with one `ArcTo` row, so it has Edit Points (the arc handle changes the bow). Returns the new package bytes and the shape's ID.
pub fn draw_arc(package: &Package, page_index: usize, a: (f64, f64), b: (f64, f64), bow: f64) -> Result<(Vec<u8>, String)> {
    if !bow.is_finite() || bow.abs() > 1000.0 {
        return Err(Error("the bow of an arc is not a usable number".to_string()));
    }
    if ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() < 0.05 {
        return Err(Error("an arc must be at least 0.05 in long".to_string()));
    }
    let (bytes, id) = draw_freeform(package, page_index, &[a, b], false)?;
    if bow.abs() < 1e-9 {
        return Ok((bytes, id));
    }
    let with_line = Package::from_bytes(&bytes)?;
    let bent = crate::groupedit::vertices::edit_vertices(&with_line, page_index, &id, crate::groupedit::vertices::VertexEdit::Bulge { segment: 0, bow })?;
    Ok((bent, id))
}

/// Put the XML of a new shape into the page part `part` (whose text is `xml`) and return the new package bytes and the shape's ID.
fn insert_shape(package: &Package, part: &str, mut xml: String, shape: &str, id: u64) -> Result<(Vec<u8>, String)> {
    if let Some(at) = xml.rfind("</Shapes>") {
        xml.insert_str(at, shape);
    } else if let Some(at) = xml.find("<Shapes/>") {
        xml.replace_range(at..at + "<Shapes/>".len(), &format!("<Shapes>{shape}</Shapes>"));
    } else if let Some(at) = xml.rfind("</PageContents>") {
        xml.insert_str(at, &format!("<Shapes>{shape}</Shapes>"));
    } else if let Some(at) = xml.rfind("/>") {
        // A page Visio saved empty: `<PageContents .../>`.
        xml.replace_range(at..at + 2, &format!("><Shapes>{shape}</Shapes></PageContents>"));
    } else {
        return Err(Error("page has no root Shapes element".to_string()));
    }
    Ok((crate::writer::with_part(package, part, xml.into_bytes())?, id.to_string()))
}

/// The picture kind of `bytes` by its first bytes: (file extension, media type), or None when it is not a PNG, JPEG, GIF or BMP.
pub fn picture_kind(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(("png", "image/png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("jpg", "image/jpeg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("gif", "image/gif"))
    } else if bytes.starts_with(b"BM") && bytes.len() > 26 {
        Some(("bmp", "image/bmp"))
    } else {
        None
    }
}

/// Insert a picture (PNG, JPEG, GIF or BMP, recognised by its first bytes, at most 30 MB) on `page_index` in the box from (`x0`, `y0`) to (`x1`, `y1`) (page inches, y up):
/// a Foreign shape with a bitmap `ForeignData`, the media part and its relationship written the way the editor writes pictures (`writer::add_picture`).
/// Returns the new package bytes and the new shape's ID.
pub fn insert_picture(package: &Package, page_index: usize, bytes: &[u8], x0: f64, y0: f64, x1: f64, y1: f64) -> Result<(Vec<u8>, String)> {
    if ![x0, y0, x1, y1].iter().all(|v| v.is_finite() && v.abs() < 10_000.0) {
        return Err(Error("the picture's position is not a usable number".to_string()));
    }
    let (left, right, bottom, top) = (x0.min(x1), x0.max(x1), y0.min(y1), y0.max(y1));
    if right - left < 0.05 || top - bottom < 0.05 {
        return Err(Error("a picture must be at least 0.05 in wide and high".to_string()));
    }
    if right - left > 200.0 || top - bottom > 200.0 {
        return Err(Error("a picture can be at most 200 in wide and high".to_string()));
    }
    if bytes.len() > 30 * 1024 * 1024 {
        return Err(Error("the picture is larger than 30 MB".to_string()));
    }
    let (extension, media_type) = picture_kind(bytes).ok_or_else(|| Error("the picture is not a PNG, JPEG, GIF or BMP file".to_string()))?;
    let id = next_shape_id(package, page_index)?;
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let page_height = page.height.unwrap_or(11.0);
    // NewPicture works in pixels (96 per inch) with the origin at the top left corner of the page
    let picture = NewPicture::new(id.to_string(), left * 96.0, (page_height - top) * 96.0, (right - left) * 96.0, (top - bottom) * 96.0, bytes.to_vec())
        .extension(extension)
        .media_type(media_type);
    let out = crate::writer::patch_page_with_media(package, page_index, &[], &[], &[], &[], &[picture])?;
    Ok((out, id.to_string()))
}
