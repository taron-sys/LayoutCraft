//! Edit points of a freeform shape (GEOM-15; Visio: Shape > Edit Vertices, Pencil / Freeform).
//!
//! Measured with probe-edit-points.ps1 (round 12): a polyline Visio draws is a `MoveTo` row and ONE `PolylineTo` row whose `A` cell is
//! `POLYLINE(0, 0, x1,y1, x2,y2, ...)` (fractions of Width / Height; type 1 = inches) and whose X / Y are the last point; plain
//! `MoveTo` / `LineTo` rows with `Width*k` / `Height*k` formulas are read as well. Setting a vertex through its cell does not change
//! Pin / Width / Height; Visio's own vertex editor refits the box. Here the box is refitted to the vertices (Pin, Width, Height, LocPin at the
//! middle) and the geometry is written as one `MoveTo` and `LineTo` rows with `Width*fx` / `Height*fy` formulas, so a later resize of the
//! shape in Visio scales the drawing. Only plain freeform shapes qualify: page level, no master, no turn or flip, rows of `MoveTo`, `LineTo`,
//! `ArcTo` and `PolylineTo` only, a single Geometry section, no Begin / End cells.
//!
//! Arcs (step 276): an `ArcTo` row ends at its X / Y and bows by `A`, the signed sagitta (the arc's middle is the chord's middle moved by `A` along the right-hand
//! normal of the direction start -> end; renderer: `circular_arc_points`). Each segment keeps its own bow (0 = straight). Moving a vertex keeps the bows (the cells
//! stay as they are), inserting a vertex on an arc splits it into two arcs of the same circle, deleting a vertex straightens the merged segment.

use super::*;

/// One edit of the vertex list (page inches).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VertexEdit {
    /// Move vertex `index` to the point (a closed shape's first and last vertex move together).
    Move { index: usize, to: (f64, f64) },
    /// Insert a vertex at the point after vertex `after` (on the segment from `after` to `after + 1`).
    Insert { after: usize, at: (f64, f64) },
    /// Delete vertex `index` (an open shape keeps at least 2, a closed one at least 3 corners).
    Delete { index: usize },
    /// Set the bow (signed sagitta, inches) of segment `segment` (from vertex `segment` to `segment + 1`); 0 makes it straight.
    Bulge { segment: usize, bow: f64 },
}

const MIN_BOX: f64 = 1e-4;

fn plain_freeform<'a>(editor: &'a GroupEditor, id: &str) -> Result<&'a ResolvedShape> {
    let node = editor.roots.iter().find(|n| n.id == id).ok_or_else(|| Error(format!("shape {id} is not a plain page-level shape")))?;
    if node.shape_type.as_deref() == Some("Group") || node.master_id.is_some() || !node.children.is_empty() {
        return Err(Error("only a drawn freeform shape has edit points (not a group or a master instance)".to_string()));
    }
    if node.cells.contains_key("BeginX") && !plain_one_d(node) {
        return Err(Error("a glued or formula-driven line or connector has no edit points here".to_string()));
    }
    Ok(node)
}

/// A plain 1-D line: Begin / End cells that are plain numbers (a glued connector has formulas there), no flip. Its box runs along the line (Width = length,
/// Height 0, Angle = the direction), so its vertices are stored in that frame (probe: a Visio line is `MoveTo(Width*0, 0)`, `LineTo(Width*1, 0)`).
fn plain_one_d(node: &ResolvedShape) -> bool {
    ["BeginX", "BeginY", "EndX", "EndY"].iter().all(|n| node.cells.get(*n).is_some_and(|c| c.formula.is_none() && c.value.as_deref().and_then(|v| v.trim().parse::<f64>().ok()).is_some()))
        && num(node, "FlipX").unwrap_or(0.0) == 0.0
        && num(node, "FlipY").unwrap_or(0.0) == 0.0
        && one_d_along_line(node).is_some()
}

/// Which frame a 1-D line uses: `Some(true)` = along the line (a line Visio draws: Width = SQRT(...) the length, Angle = ATAN2(...), Height 0),
/// `Some(false)` = the box from Begin to End (a line this engine draws: Width = GUARD(EndX-BeginX), Height = GUARD(EndY-BeginY), no turn); None = unknown.
fn one_d_along_line(node: &ResolvedShape) -> Option<bool> {
    let formula = |n: &str| node.cells.get(n).and_then(|c| c.formula.as_deref()).map(|f| f.trim().to_uppercase());
    let width = formula("Width")?;
    if width.starts_with("SQRT(") && formula("Angle").is_some_and(|a| a.starts_with("ATAN2(")) {
        Some(true)
    } else if width.starts_with("GUARD(ENDX") && num(node, "Angle").unwrap_or(0.0) == 0.0 {
        Some(false)
    } else {
        None
    }
}

fn is_one_d(node: &ResolvedShape) -> bool {
    node.cells.contains_key("BeginX")
}

fn row_number(row: &crate::shapes::SectionRow, name: &str) -> Option<f64> {
    row.cells.get(name)?.value.as_deref()?.trim().parse::<f64>().ok()
}

/// The points of `POLYLINE(xType, yType, x1,y1, ...)` in inches of the shape's own frame.
pub(super) fn polyline_points(text: &str, size: (f64, f64)) -> Option<Vec<(f64, f64)>> {
    let inner = text.trim().strip_prefix("POLYLINE(")?.strip_suffix(')')?;
    let numbers: Vec<f64> = inner.split(',').map(|t| t.trim().parse::<f64>().ok()).collect::<Option<_>>()?;
    if numbers.len() < 2 || (numbers.len() - 2) % 2 != 0 {
        return None;
    }
    let (xt, yt) = (numbers[0], numbers[1]);
    if (xt != 0.0 && xt != 1.0) || (yt != 0.0 && yt != 1.0) {
        return None;
    }
    Some(numbers[2..].chunks(2).map(|p| (if xt == 0.0 { p[0] * size.0 } else { p[0] }, if yt == 0.0 { p[1] * size.1 } else { p[1] })).collect())
}

/// The vertices of the shape in page inches, the bow of each segment (one fewer than the vertices) and whether the outline is closed (last vertex = first).
fn read_vertices(node: &ResolvedShape) -> Result<(Vec<(f64, f64)>, Vec<f64>, bool)> {
    let (w, h) = (num(node, "Width").unwrap_or(0.0), num(node, "Height").unwrap_or(0.0));
    let frame = frame_of(node)?;
    let mut sections = node.sections.iter().filter(|s| s.name == "Geometry");
    let section = sections.next().ok_or_else(|| Error("the shape has no geometry of its own".to_string()))?;
    if sections.next().is_some() {
        return Err(Error("a shape with several geometry sections cannot be edited point by point yet".to_string()));
    }
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut bows: Vec<f64> = Vec::new();
    for row in section.rows.iter().filter(|r| !r.deleted) {
        let kind = row.row_type.as_deref().unwrap_or("");
        let end = || -> Result<(f64, f64)> {
            Ok((row_number(row, "X").ok_or_else(|| Error("a geometry row has no X".to_string()))?, row_number(row, "Y").ok_or_else(|| Error("a geometry row has no Y".to_string()))?))
        };
        match kind {
            "MoveTo" if points.is_empty() => points.push(end()?),
            "RelMoveTo" if points.is_empty() => { let e = end()?; points.push((e.0 * w, e.1 * h)); }
            "LineTo" if !points.is_empty() => { points.push(end()?); bows.push(0.0); }
            "RelLineTo" if !points.is_empty() => { let e = end()?; points.push((e.0 * w, e.1 * h)); bows.push(0.0); }
            "ArcTo" if !points.is_empty() => {
                let bow = row_number(row, "A").ok_or_else(|| Error("an arc row has no numeric bow (A)".to_string()))?;
                points.push(end()?);
                bows.push(bow);
            }
            "PolylineTo" if !points.is_empty() => {
                let a = row.cells.get("A").ok_or_else(|| Error("a polyline row has no points".to_string()))?;
                let text = a.formula.as_deref().or(a.value.as_deref()).unwrap_or("");
                let inner = polyline_points(text, (w, h)).ok_or_else(|| Error("unsupported POLYLINE points".to_string()))?;
                bows.extend(std::iter::repeat(0.0).take(inner.len() + 1));
                points.extend(inner);
                points.push(end()?);
            }
            _ => return Err(Error(format!("a geometry row of kind {kind} cannot be edited point by point (only MoveTo, LineTo, ArcTo and PolylineTo)"))),
        }
    }
    if points.len() < 2 {
        return Err(Error("the shape has fewer than two vertices".to_string()));
    }
    let closed = points.len() >= 4 && (points[0].0 - points[points.len() - 1].0).abs() < 1e-9 && (points[0].1 - points[points.len() - 1].1).abs() < 1e-9;
    // a single mirror turns the arcs around: the bows are measured in the shape's own frame, the handles in the page
    let orient = orientation(&frame);
    Ok((points.into_iter().map(|p| frame.to_parent(p)).collect(), bows.into_iter().map(|b| b * orient).collect(), closed))
}

/// -1 when exactly one of FlipX / FlipY is set (a mirror), else 1: the sign between a bow in the shape's frame and the same bow on the page.
fn orientation(frame: &Frame) -> f64 {
    if frame.flip.0 != frame.flip.1 { -1.0 } else { 1.0 }
}

/// The circle of an arc segment: centre, radius, the angle of the start point and the signed sweep (positive = counter-clockwise).
fn arc_circle(a: (f64, f64), b: (f64, f64), bow: f64) -> Option<((f64, f64), f64, f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let chord = dx.hypot(dy);
    if chord <= 1e-9 || bow.abs() < 1e-9 || !bow.is_finite() {
        return None;
    }
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (nx, ny) = (dy / chord, -dx / chord);
    let mid = (mx + nx * bow, my + ny * bow);
    let h = (bow * bow - chord * chord / 4.0) / (2.0 * bow);
    let c = (mx + nx * h, my + ny * h);
    let radius = (chord / 2.0).hypot(h);
    if radius <= 1e-9 {
        return None;
    }
    let ang = |p: (f64, f64)| (p.1 - c.1).atan2(p.0 - c.0);
    let tau = std::f64::consts::PI * 2.0;
    let norm = |v: f64| ((v % tau) + tau) % tau;
    let (ps, pe, pm) = (ang(a), ang(b), ang(mid));
    let (ccw_span, ccw_mid) = (norm(pe - ps), norm(pm - ps));
    let sweep = if ccw_mid < ccw_span { ccw_span } else { -norm(ps - pe) };
    Some((c, radius, ps, sweep))
}

/// Points along an arc segment (page inches), the ends included; a straight segment gives its two ends.
fn arc_samples(a: (f64, f64), b: (f64, f64), bow: f64, steps: usize) -> Vec<(f64, f64)> {
    match arc_circle(a, b, bow) {
        None => vec![a, b],
        Some((c, r, start, sweep)) => (0..=steps).map(|i| { let t = start + sweep * i as f64 / steps as f64; (c.0 + r * t.cos(), c.1 + r * t.sin()) }).collect(),
    }
}

/// The signed bow of the sub-arc from `a` to `b` whose middle lies on the circle at `mid_angle`.
fn bow_through(a: (f64, f64), b: (f64, f64), c: (f64, f64), r: f64, mid_angle: f64) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let chord = dx.hypot(dy);
    if chord <= 1e-9 {
        return 0.0;
    }
    let (nx, ny) = (dy / chord, -dx / chord);
    let m = (c.0 + r * mid_angle.cos(), c.1 + r * mid_angle.sin());
    let cm = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    (m.0 - cm.0) * nx + (m.1 - cm.1) * ny
}

/// Split the arc segment `a` -> `b` (bow `bow`) at the point of the arc nearest to `at`: the new vertex and the bows of both halves.
fn split_arc(a: (f64, f64), b: (f64, f64), bow: f64, at: (f64, f64)) -> Option<((f64, f64), f64, f64)> {
    let (c, r, start, sweep) = arc_circle(a, b, bow)?;
    let angle = (at.1 - c.1).atan2(at.0 - c.0);
    let tau = std::f64::consts::PI * 2.0;
    let along = if sweep >= 0.0 { ((angle - start) % tau + tau) % tau } else { -(((start - angle) % tau + tau) % tau) };
    let total = sweep.abs();
    let t = along.abs().clamp(0.05 * total, 0.95 * total) * sweep.signum();
    let split = start + t;
    let p = (c.0 + r * split.cos(), c.1 + r * split.sin());
    Some((p, bow_through(a, p, c, r, start + t / 2.0), bow_through(p, b, c, r, split + (sweep - t) / 2.0)))
}

/// `{points: [[x, y], ...], closed}` for `describe` when the node is a plain freeform with at most 300 vertices, else null.
pub(crate) fn describe_vertices(node: &ResolvedShape) -> serde_json::Value {
    if node.shape_type.as_deref() == Some("Group") || node.master_id.is_some() || !node.children.is_empty() || (is_one_d(node) && !plain_one_d(node)) {
        return serde_json::Value::Null;
    }
    if is_one_d(node) {
        return match read_vertices(node) {
            Ok((points, bows, closed)) if points.len() <= 300 => serde_json::json!({ "points": points.iter().map(|p| [p.0, p.1]).collect::<Vec<_>>(), "bows": bows, "closed": closed, "line": true }),
            _ => serde_json::Value::Null,
        };
    }
    if let Ok(oval) = read_oval(node) {
        let outline: Vec<[f64; 2]> = oval_samples(&oval.v, 16).iter().map(|p| { let q = oval.frame.to_parent(*p); [q.0, q.1] }).collect();
        return serde_json::json!({ "points": oval.page_points().iter().map(|p| [p.0, p.1]).collect::<Vec<_>>(), "bows": vec![0.0; 4], "closed": true, "oval": true, "outline": outline });
    }
    if let Ok(curve) = read_curve(node) {
        let outline: Vec<[f64; 2]> = curve.samples(&curve.local_points(), 120).unwrap_or_default().iter().map(|p| { let q = curve.frame.to_parent(*p); [q.0, q.1] }).collect();
        return serde_json::json!({ "points": curve.page_points().iter().map(|p| [p.0, p.1]).collect::<Vec<_>>(), "bows": Vec::<f64>::new(), "closed": false, "curve": true, "outline": outline });
    }
    match read_vertices(node) {
        Ok((points, bows, closed)) if points.len() <= 300 => serde_json::json!({ "points": points.iter().map(|p| [p.0, p.1]).collect::<Vec<_>>(), "bows": bows, "closed": closed }),
        _ => serde_json::Value::Null,
    }
}

/// The vertices (page inches) and the closed flag of a freeform shape, for the UI's edit-points handles.
pub fn freeform_vertices(package: &Package, page_index: usize, id: &str) -> Result<(Vec<(f64, f64)>, bool)> {
    let editor = GroupEditor::open(package, page_index)?;
    let (points, _, closed) = read_vertices(plain_freeform(&editor, id)?)?;
    Ok((points, closed))
}

/// The vertices, the bow of every segment (0 = straight) and the closed flag of a freeform shape.
pub fn freeform_segments(package: &Package, page_index: usize, id: &str) -> Result<(Vec<(f64, f64)>, Vec<f64>, bool)> {
    let editor = GroupEditor::open(package, page_index)?;
    read_vertices(plain_freeform(&editor, id)?)
}

/// A curve Visio draws (DrawBezier / DrawSpline / the Spline tool): `MoveTo` and ONE `NURBSTo` row whose `E` cell is
/// `NURBS(knotLast, degree, xType, yType, x, y, knot, weight, ...)`; the listed points are the control points between the start (the MoveTo) and the end
/// (the row's X / Y). xType / yType 0 = fractions of Width / Height, 1 = inches (probe-curves.ps1, step 276).
struct Curve {
    start: (f64, f64),
    end: (f64, f64),
    nums: Vec<f64>,
    extra: Vec<(String, String)>,
    frame: Frame,
    size: (f64, f64),
}

impl Curve {
    fn scale(&self) -> (f64, f64) {
        (if self.nums[2] == 0.0 { self.size.0 } else { 1.0 }, if self.nums[3] == 0.0 { self.size.1 } else { 1.0 })
    }
    fn controls(&self) -> usize {
        (self.nums.len() - 4) / 4
    }
    /// The curve itself in the shape's own coordinates for the points `pts` (start, the control points, end): `steps + 1` samples, evaluated with the knot rules measured on
    /// Visio's curves (groupedit/nurbs.rs); None when the formula does not fit the rules (then the control polygon is used).
    fn samples(&self, pts: &[(f64, f64)], steps: usize) -> Option<Vec<(f64, f64)>> {
        let extra = |name: &str| self.extra.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.trim().parse::<f64>().ok());
        let degree = self.nums[1] as usize;
        let m = self.controls();
        if !(1..=5).contains(&degree) || pts.len() != m + 2 {
            return None;
        }
        let mut weights = vec![extra("D").unwrap_or(1.0)];
        weights.extend((0..m).map(|i| self.nums[7 + 4 * i]));
        weights.push(extra("B").unwrap_or(1.0));
        let mut interior: Vec<f64> = (0..m).map(|i| self.nums[6 + 4 * i]).filter(|k| *k > 0.0).collect();
        if let Some(a) = extra("A").filter(|a| *a > 0.0) {
            interior.push(a);
        }
        let knots = super::nurbs::knot_vector(degree, &interior, self.nums[0]);
        super::nurbs::sample(pts, &weights, &knots, degree, steps)
    }
    /// Start, control points and end in the shape's own coordinates.
    fn local_points(&self) -> Vec<(f64, f64)> {
        let (sx, sy) = self.scale();
        let mut out = vec![self.start];
        for i in 0..self.controls() {
            out.push((self.nums[4 + 4 * i] * sx, self.nums[5 + 4 * i] * sy));
        }
        out.push(self.end);
        out
    }
    /// Start, control points and end in page inches.
    fn page_points(&self) -> Vec<(f64, f64)> {
        let (sx, sy) = self.scale();
        let mut out = vec![self.frame.to_parent(self.start)];
        for i in 0..self.controls() {
            out.push(self.frame.to_parent((self.nums[4 + 4 * i] * sx, self.nums[5 + 4 * i] * sy)));
        }
        out.push(self.frame.to_parent(self.end));
        out
    }
}

fn read_curve(node: &ResolvedShape) -> Result<Curve> {
    let frame = frame_of(node)?;
    let (w, h) = frame.size;
    let mut sections = node.sections.iter().filter(|s| s.name == "Geometry");
    let section = sections.next().ok_or_else(|| Error("the shape has no geometry of its own".to_string()))?;
    if sections.next().is_some() {
        return Err(Error("a shape with several geometry sections cannot be edited point by point yet".to_string()));
    }
    let rows: Vec<_> = section.rows.iter().filter(|r| !r.deleted).collect();
    if rows.len() != 2 || rows[0].row_type.as_deref() != Some("MoveTo") || rows[1].row_type.as_deref() != Some("NURBSTo") {
        return Err(Error("not a MoveTo + NURBSTo curve".to_string()));
    }
    let xy = |row: &crate::shapes::SectionRow| -> Result<(f64, f64)> {
        Ok((row_number(row, "X").ok_or_else(|| Error("a curve row has no X".to_string()))?, row_number(row, "Y").ok_or_else(|| Error("a curve row has no Y".to_string()))?))
    };
    let e = rows[1].cells.get("E").ok_or_else(|| Error("the curve row has no E cell".to_string()))?;
    let text = e.formula.as_deref().or(e.value.as_deref()).unwrap_or("");
    let inner = text.trim().strip_prefix("NURBS(").and_then(|t| t.strip_suffix(')')).ok_or_else(|| Error("the E cell is not a NURBS formula".to_string()))?;
    let nums: Vec<f64> = inner.split(',').map(|t| t.trim().parse::<f64>().ok()).collect::<Option<_>>().ok_or_else(|| Error("unsupported NURBS numbers".to_string()))?;
    if nums.len() < 8 || (nums.len() - 4) % 4 != 0 || (nums[2] != 0.0 && nums[2] != 1.0) || (nums[3] != 0.0 && nums[3] != 1.0) || nums.len() > 4 + 4 * 60 {
        return Err(Error("unsupported NURBS formula".to_string()));
    }
    let extra = ["A", "B", "C", "D"].iter().filter_map(|n| rows[1].cells.get(*n).and_then(|c| c.value.clone()).map(|v| (n.to_string(), v))).collect();
    Ok(Curve { start: xy(rows[0])?, end: xy(rows[1])?, nums, extra, frame, size: (w, h) })
}

/// Move the start (index 0), a control point or the end (the last index) of a curve to a page point; the box is refitted to the control polygon.
fn edit_curve(package: &Package, page_index: usize, id: &str, mut curve: Curve, index: usize, to: (f64, f64)) -> Result<Vec<u8>> {
    let last = curve.controls() + 1;
    if index > last || !to.0.is_finite() || !to.1.is_finite() {
        return Err(Error(format!("point {index} does not exist")));
    }
    let local = curve.frame.from_parent(to);
    let (sx, sy) = curve.scale();
    if sx < MIN_BOX || sy < MIN_BOX {
        if index != 0 && index != last {
            return Err(Error("the shape has no width or height to scale a control point by".to_string()));
        }
    }
    if index == 0 {
        curve.start = local;
    } else if index == last {
        curve.end = local;
    } else {
        curve.nums[4 + 4 * (index - 1)] = local.0 / sx;
        curve.nums[5 + 4 * (index - 1)] = local.1 / sy;
    }
    // the box is refitted to the curve itself (the tight extent of the evaluated NURBS, step 285; Visio's own box is a little smaller because it flattens the curve
    // coarsely); when the formula does not fit the measured knot rules the control polygon is used (a curve with positive weights never leaves its hull)
    let (sx, sy) = curve.scale();
    let mut local: Vec<(f64, f64)> = vec![curve.start];
    for i in 0..curve.controls() {
        local.push((curve.nums[4 + 4 * i] * sx, curve.nums[5 + 4 * i] * sy));
    }
    local.push(curve.end);
    let extent: Vec<(f64, f64)> = curve.samples(&local, 400).unwrap_or_else(|| local.clone());
    let (mut min_x, mut max_x) = extent.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (mut min_y, mut max_y) = extent.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    // a flat axis keeps the box it has (the numbers are fractions of it)
    if max_x - min_x < MIN_BOX {
        (min_x, max_x) = (0.0, curve.size.0);
    }
    if max_y - min_y < MIN_BOX {
        (min_y, max_y) = (0.0, curve.size.1);
    }
    let (w, h) = (max_x - min_x, max_y - min_y);
    let frac = |v: f64, size: f64| if size < MIN_BOX { 0.0 } else { v / size };
    let shifted: Vec<(f64, f64)> = local.iter().map(|p| (p.0 - min_x, p.1 - min_y)).collect();
    let cell_x = |row: GeometryRow, p: (f64, f64)| {
        let (fx, fy) = (frac(p.0, w), frac(p.1, h));
        row.with_formula("X", fmt(p.0), format!("Width*{}", fmt(fx))).with_formula("Y", fmt(p.1), format!("Height*{}", fmt(fy)))
    };
    let mut nums = curve.nums.clone();
    for i in 0..curve.controls() {
        let q = shifted[1 + i];
        nums[4 + 4 * i] = if nums[2] == 0.0 { frac(q.0, w) } else { q.0 };
        nums[5 + 4 * i] = if nums[3] == 0.0 { frac(q.1, h) } else { q.1 };
    }
    let nurbs = format!("NURBS({})", nums.iter().map(|n| fmt(*n)).collect::<Vec<_>>().join(", "));
    let mut end_row = cell_x(GeometryRow::new("NURBSTo", "2"), shifted[shifted.len() - 1]).formula("E", nurbs);
    for (name, value) in &curve.extra {
        end_row = end_row.set(name.clone(), value.clone());
    }
    let rows = vec![cell_x(GeometryRow::new("MoveTo", "1"), shifted[0]), end_row];
    // the new box has its LocPin in the middle; the pin moves so that every point stays where it is on the page
    let frame = &curve.frame;
    let lin = frame.linear();
    let shift = (min_x + w / 2.0 - frame.loc.0, min_y + h / 2.0 - frame.loc.1);
    let pin = (frame.pin.0 + lin[0][0] * shift.0 + lin[0][1] * shift.1, frame.pin.1 + lin[1][0] * shift.0 + lin[1][1] * shift.1);
    let patch = ShapeCellPatch::new(id)
        .set_cached("PinX", fmt(pin.0))
        .set_cached("PinY", fmt(pin.1))
        .set_cached("Width", fmt(w))
        .set_cached("Height", fmt(h))
        .set_cached("LocPinX", fmt(w / 2.0))
        .set_cached("LocPinY", fmt(h / 2.0))
        .geometry_rows("0", rows);
    crate::writer::patch_page_cells(package, page_index, &[patch])
}

/// An oval (GEOM-15, step 282): Visio's `DrawOval` and this engine write ONE `Ellipse` row (X, Y = centre, A, B = the right point, C, D = the top point); its four cardinal
/// points are the vertices [left, top, right, bottom]. The first edit rewrites it as a closed path, a `MoveTo` and four `EllipticalArcTo` rows (X, Y = the end, A, B = a point
/// on the arc, C = the angle of the major axis, D = the ratio major / minor). Rule of the arcs: left and right keep a vertical tangent, top and bottom a horizontal one, so the
/// arc between two neighbours is a quarter of the axis-aligned ellipse whose centre is (x of the horizontal-tangent vertex, y of the vertical-tangent vertex); a moved vertex
/// therefore bends the two arcs next to it and every arc stays an exact quarter ellipse (the box of the shape is the box of the four vertices).
struct Oval {
    /// left, top, right, bottom in the shape's own coordinates
    v: [(f64, f64); 4],
    frame: Frame,
}

/// The arc from vertex `i` to vertex `i + 1` (mod 4): its middle point, C, D, the centre and the radii; None when an axis collapses.
fn oval_arc(v: &[(f64, f64); 4], i: usize) -> Option<((f64, f64), f64, f64, (f64, f64), f64, f64)> {
    let (p, q) = (v[i % 4], v[(i + 1) % 4]);
    // vertex index 0 and 2 (left, right) have the vertical tangent
    let (vert, horiz) = if i % 2 == 0 { (p, q) } else { (q, p) };
    let c = (horiz.0, vert.1);
    let (rx, ry) = ((vert.0 - c.0).abs(), (horiz.1 - c.1).abs());
    if rx < MIN_BOX || ry < MIN_BOX {
        return None;
    }
    let k = std::f64::consts::FRAC_1_SQRT_2;
    let mid = (c.0 + (p.0 - c.0) * k + (q.0 - c.0) * k, c.1 + (p.1 - c.1) * k + (q.1 - c.1) * k);
    let (angle, ratio) = if rx >= ry { (0.0, rx / ry) } else { (std::f64::consts::FRAC_PI_2, ry / rx) };
    Some((mid, angle, ratio, c, rx, ry))
}

/// The four arcs of an oval sampled (shape's own coordinates), `steps` points per arc, ending where the first one starts.
fn oval_samples(v: &[(f64, f64); 4], steps: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for i in 0..4 {
        let Some((_, _, _, c, _, _)) = oval_arc(v, i) else { continue };
        let (a, b) = ((v[i].0 - c.0, v[i].1 - c.1), (v[(i + 1) % 4].0 - c.0, v[(i + 1) % 4].1 - c.1));
        for k in 0..steps {
            let t = std::f64::consts::FRAC_PI_2 * k as f64 / steps as f64;
            out.push((c.0 + a.0 * t.cos() + b.0 * t.sin(), c.1 + a.1 * t.cos() + b.1 * t.sin()));
        }
    }
    out.push(v[0]);
    out
}

fn read_oval(node: &ResolvedShape) -> Result<Oval> {
    let frame = frame_of(node)?;
    let mut sections = node.sections.iter().filter(|s| s.name == "Geometry");
    let section = sections.next().ok_or_else(|| Error("the shape has no geometry of its own".to_string()))?;
    if sections.next().is_some() {
        return Err(Error("a shape with several geometry sections cannot be edited point by point yet".to_string()));
    }
    let rows: Vec<_> = section.rows.iter().filter(|r| !r.deleted).collect();
    let num = |row: &crate::shapes::SectionRow, n: &str| row_number(row, n).ok_or_else(|| Error(format!("an oval row has no {n}")));
    if rows.len() == 1 && rows[0].row_type.as_deref() == Some("Ellipse") {
        let r = rows[0];
        let (x, y, a, b, c, d) = (num(r, "X")?, num(r, "Y")?, num(r, "A")?, num(r, "B")?, num(r, "C")?, num(r, "D")?);
        let (rx, ry) = (a - x, d - y);
        if (b - y).abs() > 1e-6 || (c - x).abs() > 1e-6 || rx < MIN_BOX || ry < MIN_BOX {
            return Err(Error("only an upright oval (right point and top point on its axes) has edit points".to_string()));
        }
        return Ok(Oval { v: [(x - rx, y), (x, y + ry), (x + rx, y), (x, y - ry)], frame });
    }
    if rows.len() == 5 && rows[0].row_type.as_deref() == Some("MoveTo") && rows[1..].iter().all(|r| r.row_type.as_deref() == Some("EllipticalArcTo")) {
        let mut v = [(0.0, 0.0); 4];
        v[0] = (num(rows[0], "X")?, num(rows[0], "Y")?);
        for i in 1..4 {
            v[i] = (num(rows[i], "X")?, num(rows[i], "Y")?);
        }
        let back = (num(rows[4], "X")?, num(rows[4], "Y")?);
        if (back.0 - v[0].0).abs() > 1e-6 || (back.1 - v[0].1).abs() > 1e-6 {
            return Err(Error("not a closed four-arc oval".to_string()));
        }
        for i in 0..4 {
            let (mid, angle, ratio, _, _, _) = oval_arc(&v, i).ok_or_else(|| Error("an arc of the oval has no extent".to_string()))?;
            let r = rows[i + 1];
            let ok = (num(r, "A")? - mid.0).abs() < 1e-6 && (num(r, "B")? - mid.1).abs() < 1e-6 && (num(r, "C")? - angle).abs() < 1e-6 && (num(r, "D")? - ratio).abs() < 1e-6 * ratio.max(1.0);
            if !ok {
                return Err(Error("the arcs are not the quarter ellipses of an oval".to_string()));
            }
        }
        return Ok(Oval { v, frame });
    }
    Err(Error("not an oval".to_string()))
}

impl Oval {
    /// The five points of the closed outline (the first again at the end) on the page.
    fn page_points(&self) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = self.v.iter().map(|p| self.frame.to_parent(*p)).collect();
        out.push(out[0]);
        out
    }
}

/// Move one of the four vertices of an oval (index 4 is the first again) to a page point; the oval becomes four `EllipticalArcTo` rows and its box the box of the vertices.
fn edit_oval(package: &Package, page_index: usize, id: &str, oval: Oval, index: usize, to: (f64, f64)) -> Result<Vec<u8>> {
    if index > 4 || !to.0.is_finite() || !to.1.is_finite() {
        return Err(Error(format!("vertex {index} does not exist")));
    }
    let mut v = oval.v;
    v[index % 4] = oval.frame.from_parent(to);
    for i in 0..4 {
        if oval_arc(&v, i).is_none() {
            return Err(Error("an oval needs room between its vertices on both axes".to_string()));
        }
    }
    let (min_x, max_x) = v.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (min_y, max_y) = v.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let (w, h) = (max_x - min_x, max_y - min_y);
    if w < MIN_BOX || h < MIN_BOX {
        return Err(Error("the oval would collapse".to_string()));
    }
    let at = |p: (f64, f64)| (p.0 - min_x, p.1 - min_y);
    let xy = |row: GeometryRow, p: (f64, f64)| {
        let q = at(p);
        row.with_formula("X", fmt(q.0), format!("Width*{}", fmt(q.0 / w))).with_formula("Y", fmt(q.1), format!("Height*{}", fmt(q.1 / h)))
    };
    let mut rows = vec![xy(GeometryRow::new("MoveTo", "1"), v[0])];
    for i in 0..4 {
        let (mid, angle, ratio, _, _, _) = oval_arc(&v, i).expect("checked above");
        let end = v[(i + 1) % 4];
        let q = at(mid);
        let row = xy(GeometryRow::new("EllipticalArcTo", (i + 2).to_string()), end)
            .with_formula("A", fmt(q.0), format!("Width*{}", fmt(q.0 / w)))
            .with_formula("B", fmt(q.1), format!("Height*{}", fmt(q.1 / h)))
            .set("C", fmt(angle))
            .set("D", fmt(ratio));
        rows.push(row);
    }
    let frame = &oval.frame;
    let lin = frame.linear();
    let shift = (min_x + w / 2.0 - frame.loc.0, min_y + h / 2.0 - frame.loc.1);
    let pin = (frame.pin.0 + lin[0][0] * shift.0 + lin[0][1] * shift.1, frame.pin.1 + lin[1][0] * shift.0 + lin[1][1] * shift.1);
    let patch = ShapeCellPatch::new(id)
        .set_cached("PinX", fmt(pin.0))
        .set_cached("PinY", fmt(pin.1))
        .set_cached("Width", fmt(w))
        .set_cached("Height", fmt(h))
        .set_cached("LocPinX", fmt(w / 2.0))
        .set_cached("LocPinY", fmt(h / 2.0))
        .geometry_rows("0", rows);
    crate::writer::patch_page_cells(package, page_index, &[patch])
}

/// The rows of a Visio spline through `controls` (all control points including the start and the end, relative to the box origin, the shape's own coordinates), the way
/// Visio's DrawSpline writes them (probe, step 285): a `MoveTo` and ONE `NURBSTo` whose `E` cell lists the inner controls with their knots (the first three are 0, then the
/// interior knots) and weight 1, `A` = the second-to-last knot, `B` = 1, `C` = 0, `D` = 1; xType / yType 0 (fractions of the box) unless that axis is flat (then inches).
fn nurbs_rows(controls: &[(f64, f64)], interior: &[f64], knot_last: f64, w: f64, h: f64) -> Vec<GeometryRow> {
    let m = controls.len() - 2;
    let (xt, yt) = (if w < MIN_BOX { 1 } else { 0 }, if h < MIN_BOX { 1 } else { 0 });
    let fx = |v: f64| if xt == 0 { v / w } else { v };
    let fy = |v: f64| if yt == 0 { v / h } else { v };
    let xy = |row: GeometryRow, p: (f64, f64)| {
        let row = if xt == 0 { row.with_formula("X", fmt(p.0), format!("Width*{}", fmt(p.0 / w))) } else { row.set("X", fmt(p.0)) };
        if yt == 0 { row.with_formula("Y", fmt(p.1), format!("Height*{}", fmt(p.1 / h))) } else { row.set("Y", fmt(p.1)) }
    };
    let mut listed = Vec::new();
    for i in 0..m {
        let p = controls[1 + i];
        let knot = if i < 3 { 0.0 } else { interior[i - 3] };
        listed.push(format!("{},{},{},1", fmt(fx(p.0)), fmt(fy(p.1)), fmt(knot)));
    }
    let a = if m >= 3 { interior[m - 3] } else { 0.0 };
    let e = format!("NURBS({}, 3, {}, {}, {})", fmt(knot_last), xt, yt, listed.join(", "));
    let end = xy(GeometryRow::new("NURBSTo", "2"), controls[controls.len() - 1]).set("A", fmt(a)).set("B", "1").set("C", "0").set("D", "1").formula("E", e);
    vec![xy(GeometryRow::new("MoveTo", "1"), controls[0]), end]
}

/// A Visio spline through `data` (the DrawSpline fit: 4 to 200 points): the box origin, the box size and the rows of the shape's geometry; the box is the true extent of the curve.
pub(crate) fn fitted_curve_rows(data: &[(f64, f64)]) -> Result<((f64, f64), (f64, f64), Vec<GeometryRow>)> {
    let fit = super::nurbs::interpolate(data).ok_or_else(|| Error("a curve needs 4 to 200 points that are not on top of each other".to_string()))?;
    let knots = super::nurbs::knot_vector(3, &fit.interior, fit.knot_last);
    let samples = super::nurbs::sample(&fit.controls, &vec![1.0; fit.controls.len()], &knots, 3, 400).ok_or_else(|| Error("the curve cannot be evaluated".to_string()))?;
    let (min_x, max_x) = samples.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (min_y, max_y) = samples.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let (w, h) = (max_x - min_x, max_y - min_y);
    if w < MIN_BOX && h < MIN_BOX {
        return Err(Error("the curve would collapse into one point".to_string()));
    }
    let rel: Vec<(f64, f64)> = fit.controls.iter().map(|p| (p.0 - min_x, p.1 - min_y)).collect();
    Ok(((min_x, min_y), (w.max(0.0), h.max(0.0)), nurbs_rows(&rel, &fit.interior, fit.knot_last, w, h)))
}

/// Fit Curve (Visio's command of that name; run from a script it does nothing, so the result is ours): an OPEN polyline of 4 to 200 vertices becomes the smooth Visio spline
/// through its vertices (DrawSpline's cubic), the other shapes (closed, with arcs, curves, ovals, lines) are refused. The box is the true extent of the curve; the shape's
/// turn and mirror stay.
pub fn fit_curve(package: &Package, page_index: usize, id: &str) -> Result<Vec<u8>> {
    let editor = GroupEditor::open(package, page_index)?;
    let node = plain_freeform(&editor, id)?;
    if is_one_d(node) || read_curve(node).is_ok() || read_oval(node).is_ok() {
        return Err(Error("Fit Curve needs an open polyline".to_string()));
    }
    let (v, bows, closed) = read_vertices(node)?;
    if closed || bows.iter().any(|b| b.abs() > 1e-9) || !(4..=200).contains(&v.len()) {
        return Err(Error("Fit Curve needs an open polyline of 4 to 200 straight segments' vertices".to_string()));
    }
    let frame = frame_of(node)?;
    let local: Vec<(f64, f64)> = v.iter().map(|p| frame.from_parent(*p)).collect();
    let ((min_x, min_y), (w, h), rows) = fitted_curve_rows(&local)?;
    let lin = frame.linear();
    let shift = (min_x + w / 2.0 - frame.loc.0, min_y + h / 2.0 - frame.loc.1);
    let pin = (frame.pin.0 + lin[0][0] * shift.0 + lin[0][1] * shift.1, frame.pin.1 + lin[1][0] * shift.0 + lin[1][1] * shift.1);
    let patch = ShapeCellPatch::new(id)
        .set_cached("PinX", fmt(pin.0))
        .set_cached("PinY", fmt(pin.1))
        .set_cached("Width", fmt(w))
        .set_cached("Height", fmt(h))
        .set_cached("LocPinX", fmt(w / 2.0))
        .set_cached("LocPinY", fmt(h / 2.0))
        .geometry_rows("0", rows);
    crate::writer::patch_page_cells(package, page_index, &[patch])
}

/// Write the vertices of a 1-D line: Begin / End are the first and the last vertex, the frame is rebuilt along the line (Pin = the middle, Width = the length, Angle = the
/// direction, LocPin = (Width / 2, 0), Height 0), the vertices are stored in that frame (X = `Width*f`, Y in inches), a bow is kept as it is on the page.
fn write_one_d(package: &Package, page_index: usize, id: &str, node: &ResolvedShape, v: &[(f64, f64)], bows: &[f64], closed: bool) -> Result<Vec<u8>> {
    let (b, e) = (v[0], v[v.len() - 1]);
    let len = ((e.0 - b.0).powi(2) + (e.1 - b.1).powi(2)).sqrt();
    if closed || len < MIN_BOX {
        return Err(Error("the two ends of a line cannot meet".to_string()));
    }
    let along = one_d_along_line(node).ok_or_else(|| Error("this line's frame is not one this editor knows".to_string()))?;
    let pin = ((b.0 + e.0) / 2.0, (b.1 + e.1) / 2.0);
    let frame = if along {
        Frame { pin, size: (len, 0.0), loc: (len / 2.0, 0.0), angle: (e.1 - b.1).atan2(e.0 - b.0), flip: (false, false) }
    } else {
        let (w, h) = (e.0 - b.0, e.1 - b.1);
        Frame { pin, size: (w, h), loc: (w / 2.0, h / 2.0), angle: 0.0, flip: (false, false) }
    };
    let (w, h) = frame.size;
    let mut rows = Vec::new();
    for (i, p) in v.iter().enumerate() {
        let q = frame.from_parent(*p);
        let tidy = |x: f64| if x.abs() < 1e-12 { 0.0 } else { x };
        let mut row = GeometryRow::new(if i == 0 { "MoveTo" } else if bows[i.saturating_sub(1)].abs() > 1e-9 && i > 0 { "ArcTo" } else { "LineTo" }, (i + 1).to_string());
        row = if w.abs() >= MIN_BOX { row.with_formula("X", fmt(tidy(q.0)), format!("Width*{}", fmt(q.0 / w))) } else { row.set("X", fmt(tidy(q.0))) };
        row = if !along && h.abs() >= MIN_BOX { row.with_formula("Y", fmt(tidy(q.1)), format!("Height*{}", fmt(q.1 / h))) } else { row.set("Y", fmt(tidy(q.1))) };
        if i > 0 && bows[i - 1].abs() > 1e-9 {
            row = row.set("A", fmt(bows[i - 1]));
        }
        rows.push(row);
    }
    let mut patch = ShapeCellPatch::new(id)
        .set_cached("BeginX", fmt(b.0))
        .set_cached("BeginY", fmt(b.1))
        .set_cached("EndX", fmt(e.0))
        .set_cached("EndY", fmt(e.1))
        .set_cached("PinX", fmt(pin.0))
        .set_cached("PinY", fmt(pin.1))
        .set_cached("Width", fmt(w))
        .set_cached("LocPinX", fmt(w / 2.0));
    patch = if along { patch.set_cached("Angle", fmt(frame.angle)) } else { patch.set_cached("Height", fmt(h)).set_cached("LocPinY", fmt(h / 2.0)) };
    crate::writer::patch_page_cells(package, page_index, &[patch.geometry_rows("0", rows)])
}

/// Apply one vertex edit to a freeform shape; returns the new package bytes. The box is refitted to the vertices.
pub fn edit_vertices(package: &Package, page_index: usize, id: &str, edit: VertexEdit) -> Result<Vec<u8>> {
    let editor = GroupEditor::open(package, page_index)?;
    let node = plain_freeform(&editor, id)?;
    if let Ok(oval) = read_oval(node) {
        return match edit {
            VertexEdit::Move { index, to } => edit_oval(package, page_index, id, oval, index, to),
            _ => Err(Error("an oval only has four movable vertices".to_string())),
        };
    }
    if let Some(curve) = if is_one_d(node) { None } else { read_curve(node).ok() } {
        return match edit {
            VertexEdit::Move { index, to } => edit_curve(package, page_index, id, curve, index, to),
            _ => Err(Error("a curve only has movable points (its ends and control points)".to_string())),
        };
    }
    let (mut v, mut bows, closed) = read_vertices(node)?;
    let last = v.len() - 1;
    let finite = |p: (f64, f64)| p.0.is_finite() && p.1.is_finite();
    match edit {
        VertexEdit::Move { index, to } => {
            if index > last || !finite(to) {
                return Err(Error(format!("vertex {index} does not exist")));
            }
            v[index] = to;
            if closed && index == 0 {
                v[last] = to;
            } else if closed && index == last {
                v[0] = to;
            }
        }
        VertexEdit::Insert { after, at } => {
            if after >= last || !finite(at) {
                return Err(Error(format!("there is no segment after vertex {after}")));
            }
            if v.len() >= 2000 {
                return Err(Error("a freeform shape can have at most 2000 vertices".to_string()));
            }
            if bows[after].abs() > 1e-9 {
                let (a, b) = (v[after], v[after + 1]);
                let (p, bow1, bow2) = split_arc(a, b, bows[after], at).ok_or_else(|| Error("the arc cannot be split".to_string()))?;
                v.insert(after + 1, p);
                bows[after] = bow1;
                bows.insert(after + 1, bow2);
            } else {
                v.insert(after + 1, at);
                bows.insert(after + 1, 0.0);
            }
        }
        VertexEdit::Bulge { segment, bow } => {
            if segment >= last || !bow.is_finite() || bow.abs() > 1000.0 {
                return Err(Error(format!("there is no segment {segment} (or the bow is not a usable number)")));
            }
            bows[segment] = if bow.abs() < 1e-6 { 0.0 } else { bow };
        }
        VertexEdit::Delete { index } => {
            if index > last {
                return Err(Error(format!("vertex {index} does not exist")));
            }
            if (!closed && v.len() <= 2) || (closed && v.len() <= 4) {
                return Err(Error("the shape needs at least two vertices (three corners when closed)".to_string()));
            }
            if closed && (index == 0 || index == last) {
                v.remove(0);
                bows.remove(0);
                let first = v[0];
                let n = v.len();
                v[n - 1] = first;
                bows[n - 2] = 0.0;
            } else if index == 0 {
                v.remove(0);
                bows.remove(0);
            } else if index == last {
                v.remove(index);
                bows.remove(index - 1);
            } else {
                v.remove(index);
                bows.remove(index);
                bows[index - 1] = 0.0;
            }
        }
    }
    if is_one_d(node) {
        return write_one_d(package, page_index, id, node, &v, &bows, closed);
    }
    // the box encloses the curve, not only the vertices; it is refitted in the shape's OWN coordinates, so a turned or flipped shape keeps its turn and flips
    let frame = frame_of(node)?;
    let mut outline: Vec<(f64, f64)> = v.clone();
    for (i, bow) in bows.iter().enumerate() {
        if bow.abs() > 1e-9 {
            outline.extend(arc_samples(v[i], v[i + 1], *bow, 48));
        }
    }
    let outline: Vec<(f64, f64)> = outline.into_iter().map(|p| frame.from_parent(p)).collect();
    let local: Vec<(f64, f64)> = v.iter().map(|p| frame.from_parent(*p)).collect();
    let orient = orientation(&frame);
    let (min_x, max_x) = outline.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (min_y, max_y) = outline.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let (w, h) = (max_x - min_x, max_y - min_y);
    if w < MIN_BOX && h < MIN_BOX {
        return Err(Error("the vertices would collapse into one point".to_string()));
    }
    let (w, h) = (w.max(0.0), h.max(0.0));
    let frac = |value: f64, min: f64, size: f64| if size < MIN_BOX { 0.0 } else { (value - min) / size };
    let mut rows = Vec::new();
    for (i, p) in local.iter().enumerate() {
        let (fx, fy) = (frac(p.0, min_x, w), frac(p.1, min_y, h));
        let arc = i > 0 && bows[i - 1].abs() > 1e-9;
        let mut row = GeometryRow::new(if i == 0 { "MoveTo" } else if arc { "ArcTo" } else { "LineTo" }, (i + 1).to_string())
            .with_formula("X", fmt(fx * w), format!("Width*{}", fmt(fx)))
            .with_formula("Y", fmt(fy * h), format!("Height*{}", fmt(fy)));
        if arc {
            row = row.set("A", fmt(bows[i - 1] * orient));
        }
        rows.push(row);
    }
    // the new box has its LocPin in the middle; the pin moves so that every point stays where it is on the page
    let lin = frame.linear();
    let shift = (min_x + w / 2.0 - frame.loc.0, min_y + h / 2.0 - frame.loc.1);
    let pin = (frame.pin.0 + lin[0][0] * shift.0 + lin[0][1] * shift.1, frame.pin.1 + lin[1][0] * shift.0 + lin[1][1] * shift.1);
    let patch = ShapeCellPatch::new(id)
        .set_cached("PinX", fmt(pin.0))
        .set_cached("PinY", fmt(pin.1))
        .set_cached("Width", fmt(w))
        .set_cached("Height", fmt(h))
        .set_cached("LocPinX", fmt(w / 2.0))
        .set_cached("LocPinY", fmt(h / 2.0))
        .geometry_rows("0", rows);
    crate::writer::patch_page_cells(package, page_index, &[patch])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blank drawing with one freeform shape (id 7) whose outline is the polyline `points` (page inches; the first one is the start).
    fn drawing(points: &[(f64, f64)], polyline_row: bool) -> Package {
        let blank = crate::fresh::write_blank_document(&crate::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".to_string() }).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(blank)).unwrap();
        let (min_x, max_x) = points.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
        let (min_y, max_y) = points.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
        let (w, h) = (max_x - min_x, max_y - min_y);
        let local = |p: &(f64, f64)| (p.0 - min_x, p.1 - min_y);
        let mut rows = String::new();
        let s = local(&points[0]);
        rows.push_str(&format!("<Row T='MoveTo' IX='1'><Cell N='X' V='{}' F='Width*{}'/><Cell N='Y' V='{}' F='Height*{}'/></Row>", s.0, s.0 / w, s.1, s.1 / h));
        if polyline_row {
            let mid: Vec<String> = points[1..points.len() - 1].iter().map(|p| { let l = local(p); format!("{},{}", l.0 / w, l.1 / h) }).collect();
            let e = local(&points[points.len() - 1]);
            rows.push_str(&format!("<Row T='PolylineTo' IX='2'><Cell N='X' V='{}' F='Width*{}'/><Cell N='Y' V='{}' F='Height*{}'/><Cell N='A' V='POLYLINE(0, 0, {})' U='POLYLINE' F='POLYLINE(0, 0, {})'/></Row>", e.0, e.0 / w, e.1, e.1 / h, mid.join(", "), mid.join(", ")));
        } else {
            for (i, p) in points[1..].iter().enumerate() {
                let l = local(p);
                rows.push_str(&format!("<Row T='LineTo' IX='{}'><Cell N='X' V='{}' F='Width*{}'/><Cell N='Y' V='{}' F='Height*{}'/></Row>", i + 2, l.0, l.0 / w, l.1, l.1 / h));
            }
        }
        let shape = format!(
            "<Shape ID='7' Type='Shape'><Cell N='PinX' V='{}'/><Cell N='PinY' V='{}'/><Cell N='Width' V='{w}'/><Cell N='Height' V='{h}'/><Cell N='LocPinX' V='{}' F='Width*0.5'/><Cell N='LocPinY' V='{}' F='Height*0.5'/><Cell N='Angle' V='0'/><Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/><Cell N='NoLine' V='0'/>{rows}</Section></Shape>",
            min_x + w / 2.0, min_y + h / 2.0, w / 2.0, h / 2.0
        );
        let part = package.pages().unwrap()[0].part.clone();
        let mut xml = package.text(&part).unwrap().to_string();
        let at = xml.rfind("</Shapes>").expect("the blank page has a Shapes element");
        xml.insert_str(at, &shape);
        let bytes = crate::writer::with_part(&package, &part, xml.into_bytes()).unwrap();
        Package::from_reader(std::io::Cursor::new(bytes)).unwrap()
    }

    fn near(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn a_visio_polyline_row_and_plain_rows_read_as_the_same_vertices() {
        let pts = [(1.0, 1.0), (2.0, 3.0), (4.0, 3.0), (5.0, 1.0)];
        for polyline in [true, false] {
            let (v, closed) = freeform_vertices(&drawing(&pts, polyline), 0, "7").unwrap();
            assert!(!closed);
            assert_eq!(v.len(), 4, "polyline row: {polyline}");
            assert!(v.iter().zip(pts.iter()).all(|(a, b)| near(*a, *b)), "{v:?}");
        }
    }

    #[test]
    fn moving_a_vertex_refits_the_box_and_survives_reimport() {
        let package = drawing(&[(1.0, 1.0), (2.0, 3.0), (4.0, 3.0), (5.0, 1.0)], true);
        let bytes = edit_vertices(&package, 0, "7", VertexEdit::Move { index: 3, to: (7.0, 0.5) }).unwrap();
        let after = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let (v, _) = freeform_vertices(&after, 0, "7").unwrap();
        assert!(near(v[3], (7.0, 0.5)) && near(v[0], (1.0, 1.0)) && near(v[1], (2.0, 3.0)));
        let editor = GroupEditor::open(&after, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == "7").unwrap();
        // the box is the bounding box of the vertices: x 1..7, y 0.5..3
        assert!((num(node, "Width").unwrap() - 6.0).abs() < 1e-6 && (num(node, "Height").unwrap() - 2.5).abs() < 1e-6);
        assert!((num(node, "PinX").unwrap() - 4.0).abs() < 1e-6 && (num(node, "PinY").unwrap() - 1.75).abs() < 1e-6);
    }

    #[test]
    fn inserting_and_deleting_vertices_changes_their_count_and_keeps_the_others() {
        let package = drawing(&[(1.0, 1.0), (2.0, 3.0), (4.0, 3.0), (5.0, 1.0)], false);
        let bytes = edit_vertices(&package, 0, "7", VertexEdit::Insert { after: 1, at: (3.0, 4.0) }).unwrap();
        let five = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let (v, _) = freeform_vertices(&five, 0, "7").unwrap();
        assert_eq!(v.len(), 5);
        assert!(near(v[2], (3.0, 4.0)) && near(v[1], (2.0, 3.0)) && near(v[3], (4.0, 3.0)));
        let bytes = edit_vertices(&five, 0, "7", VertexEdit::Delete { index: 2 }).unwrap();
        let four = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let (v, _) = freeform_vertices(&four, 0, "7").unwrap();
        assert_eq!(v.len(), 4);
        assert!(near(v[2], (4.0, 3.0)));
        // an open shape keeps two vertices
        let two = drawing(&[(1.0, 1.0), (3.0, 2.0)], false);
        assert!(edit_vertices(&two, 0, "7", VertexEdit::Delete { index: 0 }).is_err());
    }

    #[test]
    fn a_closed_shape_moves_first_and_last_vertex_together_and_keeps_three_corners() {
        let package = drawing(&[(1.0, 1.0), (3.0, 4.0), (5.0, 1.0), (1.0, 1.0)], true);
        let (v, closed) = freeform_vertices(&package, 0, "7").unwrap();
        assert!(closed && v.len() == 4);
        let bytes = edit_vertices(&package, 0, "7", VertexEdit::Move { index: 3, to: (0.5, 0.5) }).unwrap();
        let moved = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let (v, closed) = freeform_vertices(&moved, 0, "7").unwrap();
        assert!(closed && near(v[0], (0.5, 0.5)) && near(v[3], (0.5, 0.5)));
        assert!(edit_vertices(&moved, 0, "7", VertexEdit::Delete { index: 1 }).is_err(), "a triangle cannot lose a corner");
    }

    #[test]
    fn shapes_that_are_not_plain_freeforms_are_refused() {
        let package = drawing(&[(1.0, 1.0), (2.0, 3.0), (4.0, 3.0)], false);
        assert!(freeform_vertices(&package, 0, "99").is_err());
        assert!(edit_vertices(&package, 0, "7", VertexEdit::Move { index: 9, to: (1.0, 1.0) }).is_err());
        assert!(edit_vertices(&package, 0, "7", VertexEdit::Insert { after: 2, at: (1.0, 1.0) }).is_err());
    }

    fn reopen(bytes: Vec<u8>) -> Package {
        Package::from_reader(std::io::Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn a_segment_can_be_bowed_into_an_arc_and_straightened_again() {
        let package = drawing(&[(1.0, 1.0), (5.0, 2.0)], false);
        let (_, bows, _) = freeform_segments(&package, 0, "7").unwrap();
        assert_eq!(bows, vec![0.0]);
        let arc = reopen(edit_vertices(&package, 0, "7", VertexEdit::Bulge { segment: 0, bow: 1.0 }).unwrap());
        let (v, bows, _) = freeform_segments(&arc, 0, "7").unwrap();
        assert!(near(v[0], (1.0, 1.0)) && near(v[1], (5.0, 2.0)), "the ends stay where they are: {v:?}");
        assert!((bows[0] - 1.0).abs() < 1e-9, "{bows:?}");
        // the stored row is an ArcTo with A = the bow, and the box encloses the curve (the vertices alone span only 1 inch of height)
        let editor = GroupEditor::open(&arc, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == "7").unwrap();
        let section = node.sections.iter().find(|s| s.name == "Geometry").unwrap();
        assert_eq!(section.rows.iter().filter(|r| r.row_type.as_deref() == Some("ArcTo")).count(), 1);
        assert!(num(node, "Height").unwrap() > 1.5, "box height {:?}", num(node, "Height"));
        // a bow of 0 straightens it again
        let flat = reopen(edit_vertices(&arc, 0, "7", VertexEdit::Bulge { segment: 0, bow: 0.0 }).unwrap());
        let (_, bows, _) = freeform_segments(&flat, 0, "7").unwrap();
        assert_eq!(bows, vec![0.0]);
        assert!(edit_vertices(&arc, 0, "7", VertexEdit::Bulge { segment: 1, bow: 1.0 }).is_err(), "there is no segment 1");
        assert!(edit_vertices(&arc, 0, "7", VertexEdit::Bulge { segment: 0, bow: f64::NAN }).is_err());
    }

    #[test]
    fn inserting_on_an_arc_keeps_both_halves_on_the_same_circle() {
        let package = drawing(&[(1.0, 1.0), (5.0, 2.0)], false);
        let arc = reopen(edit_vertices(&package, 0, "7", VertexEdit::Bulge { segment: 0, bow: 1.0 }).unwrap());
        let (v, bows, _) = freeform_segments(&arc, 0, "7").unwrap();
        let (c, r, _, _) = arc_circle(v[0], v[1], bows[0]).unwrap();
        let split = reopen(edit_vertices(&arc, 0, "7", VertexEdit::Insert { after: 0, at: (3.0, 0.0) }).unwrap());
        let (v2, bows2, _) = freeform_segments(&split, 0, "7").unwrap();
        assert_eq!(v2.len(), 3);
        assert!(bows2.iter().all(|b| b.abs() > 1e-6), "both halves stay arcs: {bows2:?}");
        let on_circle = |p: (f64, f64)| ((p.0 - c.0).hypot(p.1 - c.1) - r).abs() < 1e-5;
        assert!(on_circle(v2[1]), "the new vertex lies on the arc: {:?} centre {c:?} r {r}", v2[1]);
        for i in 0..2 {
            let (c2, r2, _, _) = arc_circle(v2[i], v2[i + 1], bows2[i]).unwrap();
            assert!((c2.0 - c.0).abs() < 1e-4 && (c2.1 - c.1).abs() < 1e-4 && (r2 - r).abs() < 1e-4, "half {i} is on the same circle");
        }
        // deleting the middle vertex gives a straight segment again; moving a vertex keeps the bow
        let gone = reopen(edit_vertices(&split, 0, "7", VertexEdit::Delete { index: 1 }).unwrap());
        let (_, bows3, _) = freeform_segments(&gone, 0, "7").unwrap();
        assert_eq!(bows3, vec![0.0]);
        let moved = reopen(edit_vertices(&arc, 0, "7", VertexEdit::Move { index: 1, to: (6.0, 3.0) }).unwrap());
        let (v4, bows4, _) = freeform_segments(&moved, 0, "7").unwrap();
        assert!(near(v4[1], (6.0, 3.0)) && (bows4[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn arc_rows_read_with_their_bow_and_the_other_rows_stay_straight() {
        // MoveTo, LineTo, ArcTo with A = -0.5
        let blank = crate::fresh::write_blank_document(&crate::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".to_string() }).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(blank)).unwrap();
        let rows = "<Row T='MoveTo' IX='1'><Cell N='X' V='0'/><Cell N='Y' V='0'/></Row><Row T='LineTo' IX='2'><Cell N='X' V='2'/><Cell N='Y' V='0'/></Row><Row T='ArcTo' IX='3'><Cell N='X' V='2'/><Cell N='Y' V='2'/><Cell N='A' V='-0.5'/></Row>";
        let shape = format!("<Shape ID='7' Type='Shape'><Cell N='PinX' V='3'/><Cell N='PinY' V='3'/><Cell N='Width' V='2'/><Cell N='Height' V='2'/><Cell N='LocPinX' V='1'/><Cell N='LocPinY' V='1'/><Cell N='Angle' V='0'/><Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/><Cell N='NoLine' V='0'/>{rows}</Section></Shape>");
        let part = package.pages().unwrap()[0].part.clone();
        let mut xml = package.text(&part).unwrap().to_string();
        let at = xml.rfind("</Shapes>").unwrap();
        xml.insert_str(at, &shape);
        let drawn = reopen(crate::writer::with_part(&package, &part, xml.into_bytes()).unwrap());
        let (v, bows, closed) = freeform_segments(&drawn, 0, "7").unwrap();
        assert!(!closed && v.len() == 3 && bows == vec![0.0, -0.5], "{v:?} {bows:?}");
        assert!(near(v[0], (2.0, 2.0)) && near(v[2], (4.0, 4.0)));
        // editing another vertex keeps the arc (ArcTo row written back with its bow)
        let after = reopen(edit_vertices(&drawn, 0, "7", VertexEdit::Move { index: 0, to: (2.0, 1.5) }).unwrap());
        let (_, bows2, _) = freeform_segments(&after, 0, "7").unwrap();
        assert_eq!(bows2, vec![0.0, -0.5]);
    }

    /// A Visio Bezier (DrawBezier(1,5, 2,7, 3,4, 4,6)): a MoveTo and ONE NURBSTo row (the probe of step 276).
    fn bezier_drawing() -> Package {
        let blank = crate::fresh::write_blank_document(&crate::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".to_string() }).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(blank)).unwrap();
        let rows = "<Row T='MoveTo' IX='1'><Cell N='X' V='0' F='Width*0'/><Cell N='Y' V='0' F='Height*0'/></Row><Row T='NURBSTo' IX='2'><Cell N='X' V='3' F='Width*1'/><Cell N='Y' V='1' F='Height*1'/><Cell N='A' V='0'/><Cell N='B' V='1'/><Cell N='C' V='0'/><Cell N='D' V='1'/><Cell N='E' V='NURBS(1, 3, 0, 0, 0.33333333333333,2,0,1, 0.66666666666667,-1,0,1)' F='NURBS(1, 3, 0, 0, 0.33333333333333,2,0,1, 0.66666666666667,-1,0,1)'/></Row>";
        let shape = format!("<Shape ID='9' Type='Shape'><Cell N='PinX' V='2.5'/><Cell N='PinY' V='5.5'/><Cell N='Width' V='3'/><Cell N='Height' V='1'/><Cell N='LocPinX' V='1.5'/><Cell N='LocPinY' V='0.5'/><Cell N='Angle' V='0'/><Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/><Cell N='NoLine' V='0'/>{rows}</Section></Shape>");
        let part = package.pages().unwrap()[0].part.clone();
        let mut xml = package.text(&part).unwrap().to_string();
        let at = xml.rfind("</Shapes>").unwrap();
        xml.insert_str(at, &shape);
        reopen(crate::writer::with_part(&package, &part, xml.into_bytes()).unwrap())
    }

    #[test]
    fn a_nurbs_curve_reports_its_start_control_points_and_end() {
        let package = bezier_drawing();
        let editor = GroupEditor::open(&package, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == "9").unwrap();
        let json = describe_vertices(node);
        assert_eq!(json["curve"], serde_json::json!(true));
        let pts: Vec<(f64, f64)> = json["points"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
        // DrawBezier(1,5, 2,7, 3,4, 4,6): the start, the two control points and the end
        assert!(pts.len() == 4 && near(pts[0], (1.0, 5.0)) && near(pts[1], (2.0, 7.0)) && near(pts[2], (3.0, 4.0)) && near(pts[3], (4.0, 6.0)), "{pts:?}");
    }

    #[test]
    fn moving_a_control_point_changes_only_that_point_and_refits_the_box_to_the_control_polygon() {
        let package = bezier_drawing();
        let bytes = edit_vertices(&package, 0, "9", VertexEdit::Move { index: 1, to: (2.5, 7.5) }).unwrap();
        let after = reopen(bytes);
        let editor = GroupEditor::open(&after, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == "9").unwrap();
        let json = describe_vertices(node);
        let pts: Vec<(f64, f64)> = json["points"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
        assert!(near(pts[0], (1.0, 5.0)) && near(pts[1], (2.5, 7.5)) && near(pts[2], (3.0, 4.0)) && near(pts[3], (4.0, 6.0)), "{pts:?}");
        let (bw, bh) = (num(node, "Width").unwrap(), num(node, "Height").unwrap());
        assert!((bw - 3.0).abs() < 1e-6 && bh >= 1.0 - 1e-6 && bh < 3.5 - 0.1, "the box is the curve's (tighter than the control polygon, which is 3 x 3.5): {bw} x {bh}");
        // moving the point back inside shrinks the box again, and the other points never move on the page
        let back = reopen(edit_vertices(&after, 0, "9", VertexEdit::Move { index: 1, to: (2.0, 5.5) }).unwrap());
        let editor = GroupEditor::open(&back, 0).unwrap();
        let node2 = editor.roots.iter().find(|n| n.id == "9").unwrap();
        let j2 = describe_vertices(node2);
        let p2 = |i: usize| (j2["points"][i][0].as_f64().unwrap(), j2["points"][i][1].as_f64().unwrap());
        assert!({ let h2 = num(node2, "Height").unwrap(); h2 > 0.9 && h2 < 2.0 - 0.05 } && near(p2(0), (1.0, 5.0)) && near(p2(2), (3.0, 4.0)) && near(p2(3), (4.0, 6.0)) && near(p2(1), (2.0, 5.5)), "{j2}");
        // the degree and the A..D cells survive
        let section = node.sections.iter().find(|s| s.name == "Geometry").unwrap();
        let row = section.rows.iter().find(|r| r.row_type.as_deref() == Some("NURBSTo")).unwrap();
        assert!(row.cells.get("E").and_then(|c| c.formula.as_deref().or(c.value.as_deref())).unwrap().starts_with("NURBS(1, 3, 0, 0,"));
        assert_eq!(row.cells.get("B").and_then(|c| c.value.as_deref()), Some("1"));
        // the ends move too, but a curve cannot gain or lose points
        let ends = reopen(edit_vertices(&after, 0, "9", VertexEdit::Move { index: 3, to: (5.0, 6.5) }).unwrap());
        let editor = GroupEditor::open(&ends, 0).unwrap();
        let json = describe_vertices(editor.roots.iter().find(|n| n.id == "9").unwrap());
        assert!(near((json["points"][3][0].as_f64().unwrap(), json["points"][3][1].as_f64().unwrap()), (5.0, 6.5)));
        assert!(edit_vertices(&after, 0, "9", VertexEdit::Delete { index: 1 }).is_err());
        assert!(edit_vertices(&after, 0, "9", VertexEdit::Insert { after: 0, at: (2.0, 6.0) }).is_err());
        assert!(edit_vertices(&after, 0, "9", VertexEdit::Move { index: 9, to: (2.0, 6.0) }).is_err());
    }

    /// The same drawing with shape 7 turned by `angle` (radians) and mirrored in x when `flip_x`.
    fn turned(package: &Package, angle: f64, flip_x: bool) -> Package {
        let part = package.pages().unwrap()[0].part.clone();
        let xml = package.text(&part).unwrap().to_string();
        let xml = xml.replacen("<Cell N='Angle' V='0'/>", &format!("<Cell N='Angle' V='{angle}'/><Cell N='FlipX' V='{}'/>", if flip_x { 1 } else { 0 }), 1);
        reopen(crate::writer::with_part(package, &part, xml.into_bytes()).unwrap())
    }

    #[test]
    fn a_turned_and_mirrored_shape_keeps_its_other_points_where_they_are_on_the_page() {
        for (angle, flip) in [(0.6_f64, false), (0.6, true), (-1.2, true), (std::f64::consts::PI, false)] {
            let package = turned(&drawing(&[(1.0, 1.0), (2.0, 3.0), (4.0, 3.0), (5.0, 1.0)], false), angle, flip);
            let (v0, _) = freeform_vertices(&package, 0, "7").unwrap();
            let target = (v0[2].0 + 0.7, v0[2].1 - 0.4);
            let moved = reopen(edit_vertices(&package, 0, "7", VertexEdit::Move { index: 2, to: target }).unwrap());
            let (v1, _) = freeform_vertices(&moved, 0, "7").unwrap();
            assert!(near(v1[2], target), "angle {angle} flip {flip}: the moved vertex is on the target: {:?} vs {target:?}", v1[2]);
            assert!([0, 1, 3].iter().all(|i| near(v1[*i], v0[*i])), "angle {angle} flip {flip}: the others stay on the page: {v1:?} vs {v0:?}");
            let editor = GroupEditor::open(&moved, 0).unwrap();
            let node = editor.roots.iter().find(|n| n.id == "7").unwrap();
            assert!((num(node, "Angle").unwrap() - angle).abs() < 1e-9, "the turn is kept");
            assert_eq!(num(node, "FlipX").unwrap_or(0.0) != 0.0, flip, "the mirror is kept");
            // inserting and deleting keep the others in place as well
            let more = reopen(edit_vertices(&moved, 0, "7", VertexEdit::Insert { after: 0, at: (2.0, 2.0) }).unwrap());
            let (v2, _) = freeform_vertices(&more, 0, "7").unwrap();
            assert!(v2.len() == 5 && near(v2[0], v1[0]) && near(v2[2], v1[1]) && near(v2[4], v1[3]), "angle {angle} flip {flip}: insert: {v2:?}");
        }
    }

    #[test]
    fn a_bow_is_the_same_on_the_page_whatever_the_turn_and_mirror() {
        for (angle, flip) in [(0.0_f64, false), (0.9, false), (0.9, true), (0.0, true)] {
            let package = turned(&drawing(&[(1.0, 1.0), (5.0, 2.0)], false), angle, flip);
            let bent = reopen(edit_vertices(&package, 0, "7", VertexEdit::Bulge { segment: 0, bow: 0.8 }).unwrap());
            let (v, bows, _) = freeform_segments(&bent, 0, "7").unwrap();
            let (v0, _) = freeform_vertices(&package, 0, "7").unwrap();
            assert!(near(v[0], v0[0]) && near(v[1], v0[1]), "the ends stay on the page");
            assert!((bows[0] - 0.8).abs() < 1e-9, "angle {angle} flip {flip}: the bow reads back as 0.8 on the page: {bows:?}");
            // a mirror stores the opposite sign in the shape's own frame
            let editor = GroupEditor::open(&bent, 0).unwrap();
            let node = editor.roots.iter().find(|n| n.id == "7").unwrap();
            let section = node.sections.iter().find(|s| s.name == "Geometry").unwrap();
            let stored = section.rows.iter().find(|r| r.row_type.as_deref() == Some("ArcTo")).and_then(|r| row_number(r, "A")).unwrap();
            assert!((stored - if flip { -0.8 } else { 0.8 }).abs() < 1e-9, "stored {stored}");
        }
    }

    #[test]
    fn a_turned_curve_moves_a_control_point_on_the_page() {
        let package = turned(&bezier_drawing(), 0.5, true);
        let editor = GroupEditor::open(&package, 0).unwrap();
        let before = describe_vertices(editor.roots.iter().find(|n| n.id == "9").unwrap());
        let p = |j: &serde_json::Value, i: usize| (j["points"][i][0].as_f64().unwrap(), j["points"][i][1].as_f64().unwrap());
        let target = (p(&before, 1).0 + 0.5, p(&before, 1).1 + 0.25);
        let moved = reopen(edit_vertices(&package, 0, "9", VertexEdit::Move { index: 1, to: target }).unwrap());
        let editor = GroupEditor::open(&moved, 0).unwrap();
        let after = describe_vertices(editor.roots.iter().find(|n| n.id == "9").unwrap());
        assert!(near(p(&after, 1), target), "{:?} vs {target:?}", p(&after, 1));
        assert!(near(p(&after, 0), p(&before, 0)) && near(p(&after, 2), p(&before, 2)) && near(p(&after, 3), p(&before, 3)));
    }

    /// A plain 1-D line: `along` = the frame a line Visio draws has (Width = SQRT(..), Angle = ATAN2(..)), else the one this engine draws (box from Begin to End).
    fn line(along: bool, b: (f64, f64), e: (f64, f64)) -> Package {
        let blank = crate::fresh::write_blank_document(&crate::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".to_string() }).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(blank)).unwrap();
        let (dx, dy) = (e.0 - b.0, e.1 - b.1);
        let len = (dx * dx + dy * dy).sqrt();
        let (pin, ends) = (((b.0 + e.0) / 2.0, (b.1 + e.1) / 2.0), format!("<Cell N='BeginX' V='{}'/><Cell N='BeginY' V='{}'/><Cell N='EndX' V='{}'/><Cell N='EndY' V='{}'/>", b.0, b.1, e.0, e.1));
        let head = if along {
            format!("<Cell N='PinX' V='{}' F='(BeginX+EndX)/2'/><Cell N='PinY' V='{}' F='(BeginY+EndY)/2'/><Cell N='Width' V='{len}' F='SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)'/><Cell N='Height' V='0'/><Cell N='LocPinX' V='{}' F='Width*0.5'/><Cell N='LocPinY' V='0' F='Height*0.5'/><Cell N='Angle' V='{}' F='ATAN2(EndY-BeginY,EndX-BeginX)'/>", pin.0, pin.1, len / 2.0, dy.atan2(dx))
        } else {
            format!("<Cell N='PinX' V='{}' F='(BeginX+EndX)/2'/><Cell N='PinY' V='{}' F='(BeginY+EndY)/2'/><Cell N='Width' V='{dx}' F='GUARD(EndX-BeginX)'/><Cell N='Height' V='{dy}' F='GUARD(EndY-BeginY)'/><Cell N='LocPinX' V='{}' F='Width*0.5'/><Cell N='LocPinY' V='{}' F='Height*0.5'/><Cell N='Angle' V='0'/>", pin.0, pin.1, dx / 2.0, dy / 2.0)
        };
        let rows = if along {
            format!("<Row T='MoveTo' IX='1'><Cell N='X' V='0' F='Width*0'/><Cell N='Y' V='0'/></Row><Row T='LineTo' IX='2'><Cell N='X' V='{len}' F='Width*1'/><Cell N='Y' V='0'/></Row>")
        } else {
            format!("<Row T='MoveTo' IX='1'><Cell N='X' V='0'/><Cell N='Y' V='0'/></Row><Row T='LineTo' IX='2'><Cell N='X' V='{dx}' F='Width*1'/><Cell N='Y' V='{dy}' F='Height*1'/></Row>")
        };
        let shape = format!("<Shape ID='7' Type='Shape'>{head}<Cell N='FlipX' V='0'/><Cell N='FlipY' V='0'/>{ends}<Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/><Cell N='NoLine' V='0'/>{rows}</Section></Shape>");
        let part = package.pages().unwrap()[0].part.clone();
        let mut xml = package.text(&part).unwrap().to_string();
        let at = xml.rfind("</Shapes>").unwrap();
        xml.insert_str(at, &shape);
        reopen(crate::writer::with_part(&package, &part, xml.into_bytes()).unwrap())
    }

    #[test]
    fn a_plain_one_d_line_has_edit_points_in_both_frames() {
        for along in [true, false] {
            for (b, e) in [((1.0, 10.0), (3.0, 11.0)), ((5.0, 4.0), (2.0, 1.0)), ((2.0, 2.0), (2.0, 6.0))] {
                let package = line(along, b, e);
                let (v, bows, closed) = freeform_segments(&package, 0, "7").unwrap();
                assert!(v.len() == 2 && !closed && bows == vec![0.0] && near(v[0], b) && near(v[1], e), "along {along}: {v:?}");
                // a vertex added on the line and dragged off it: the ends stay, the line becomes a polyline
                let mid = ((b.0 + e.0) / 2.0, (b.1 + e.1) / 2.0);
                let more = reopen(edit_vertices(&package, 0, "7", VertexEdit::Insert { after: 0, at: mid }).unwrap());
                let target = (mid.0 + 0.8, mid.1 - 0.6);
                let bent = reopen(edit_vertices(&more, 0, "7", VertexEdit::Move { index: 1, to: target }).unwrap());
                let (v2, _, _) = freeform_segments(&bent, 0, "7").unwrap();
                assert!(v2.len() == 3 && near(v2[0], b) && near(v2[1], target) && near(v2[2], e), "along {along} {b:?}: {v2:?}");
                // the ends are the Begin / End cells
                let editor = GroupEditor::open(&bent, 0).unwrap();
                let node = editor.roots.iter().find(|n| n.id == "7").unwrap();
                assert!((num(node, "BeginX").unwrap() - b.0).abs() < 1e-9 && (num(node, "EndY").unwrap() - e.1).abs() < 1e-9);
                // moving an END moves End, and the middle vertex stays where it is on the page
                let end_to = (e.0 + 1.0, e.1 + 0.5);
                let longer = reopen(edit_vertices(&bent, 0, "7", VertexEdit::Move { index: 2, to: end_to }).unwrap());
                let (v3, _, _) = freeform_segments(&longer, 0, "7").unwrap();
                assert!(near(v3[0], b) && near(v3[1], target) && near(v3[2], end_to), "along {along} {b:?}: {v3:?}");
                let editor = GroupEditor::open(&longer, 0).unwrap();
                let node = editor.roots.iter().find(|n| n.id == "7").unwrap();
                assert!((num(node, "EndX").unwrap() - end_to.0).abs() < 1e-9 && (num(node, "EndY").unwrap() - end_to.1).abs() < 1e-9);
                // a bow keeps its size on the page
                let arc = reopen(edit_vertices(&package, 0, "7", VertexEdit::Bulge { segment: 0, bow: 0.4 }).unwrap());
                let (_, bows, _) = freeform_segments(&arc, 0, "7").unwrap();
                assert!((bows[0] - 0.4).abs() < 1e-9, "along {along}: {bows:?}");
            }
        }
    }

    #[test]
    fn a_glued_or_formula_driven_line_is_refused() {
        let package = line(true, (1.0, 1.0), (4.0, 2.0));
        let part = package.pages().unwrap()[0].part.clone();
        let xml = package.text(&part).unwrap().replacen("<Cell N='BeginX' V='1'/>", "<Cell N='BeginX' V='1' F='PAR(PNT(Sheet.3!Connections.X1,Sheet.3!Connections.Y1))'/>", 1);
        let glued = reopen(crate::writer::with_part(&package, &part, xml.into_bytes()).unwrap());
        assert!(freeform_segments(&glued, 0, "7").is_err());
        let editor = GroupEditor::open(&glued, 0).unwrap();
        assert!(describe_vertices(editor.roots.iter().find(|n| n.id == "7").unwrap()).is_null());
    }

    /// A Visio oval (DrawOval): ONE `Ellipse` row; shape 5 at pin (4, 3), 4 x 2, optionally turned / mirrored.
    fn oval_drawing(angle: f64, flip_x: bool) -> Package {
        let blank = crate::fresh::write_blank_document(&crate::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".to_string() }).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(blank)).unwrap();
        let row = "<Row T='Ellipse' IX='1'><Cell N='X' V='2' F='Width*0.5'/><Cell N='Y' V='1' F='Height*0.5'/><Cell N='A' V='4' F='Width*1'/><Cell N='B' V='1' F='Height*0.5'/><Cell N='C' V='2' F='Width*0.5'/><Cell N='D' V='2' F='Height*1'/></Row>";
        let shape = format!("<Shape ID='5' Type='Shape'><Cell N='PinX' V='4'/><Cell N='PinY' V='3'/><Cell N='Width' V='4'/><Cell N='Height' V='2'/><Cell N='LocPinX' V='2'/><Cell N='LocPinY' V='1'/><Cell N='Angle' V='{angle}'/><Cell N='FlipX' V='{}'/><Section N='Geometry' IX='0'><Cell N='NoFill' V='0'/><Cell N='NoLine' V='0'/>{row}</Section></Shape>", if flip_x { 1 } else { 0 });
        let part = package.pages().unwrap()[0].part.clone();
        let mut xml = package.text(&part).unwrap().to_string();
        let at = xml.rfind("</Shapes>").unwrap();
        xml.insert_str(at, &shape);
        reopen(crate::writer::with_part(&package, &part, xml.into_bytes()).unwrap())
    }

    fn oval_points(package: &Package) -> (Vec<(f64, f64)>, serde_json::Value) {
        let editor = GroupEditor::open(package, 0).unwrap();
        let json = describe_vertices(editor.roots.iter().find(|n| n.id == "5").unwrap());
        let pts = json["points"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
        (pts, json)
    }

    #[test]
    fn an_oval_reports_its_four_cardinal_points_and_an_outline() {
        let package = oval_drawing(0.0, false);
        let (pts, json) = oval_points(&package);
        assert!(json["oval"] == true && json["closed"] == true && pts.len() == 5, "{json}");
        // pin (4, 3), 4 x 2: left (2, 3), top (4, 4), right (6, 3), bottom (4, 2)
        assert!(near(pts[0], (2.0, 3.0)) && near(pts[1], (4.0, 4.0)) && near(pts[2], (6.0, 3.0)) && near(pts[3], (4.0, 2.0)) && near(pts[4], pts[0]), "{pts:?}");
        let outline = json["outline"].as_array().unwrap();
        assert!(outline.len() > 40 && outline.iter().all(|p| { let (x, y) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap()); ((x - 4.0) / 2.0).powi(2) + (y - 3.0).powi(2) <= 1.0 + 1e-9 && ((x - 4.0) / 2.0).powi(2) + (y - 3.0).powi(2) >= 1.0 - 1e-9 }), "the unedited outline is the ellipse");
    }

    #[test]
    fn moving_an_oval_vertex_writes_four_quarter_ellipse_arcs_and_keeps_the_others() {
        for (angle, flip) in [(0.0_f64, false), (0.6, false), (0.6, true), (-1.1, true)] {
            let package = oval_drawing(angle, flip);
            let (before, _) = oval_points(&package);
            let target = (before[1].0 + 0.5, before[1].1 + 0.4);
            let moved = reopen(edit_vertices(&package, 0, "5", VertexEdit::Move { index: 1, to: target }).unwrap());
            let editor = GroupEditor::open(&moved, 0).unwrap();
            let node = editor.roots.iter().find(|n| n.id == "5").unwrap();
            let section = node.sections.iter().find(|s| s.name == "Geometry").unwrap();
            let kinds: Vec<_> = section.rows.iter().filter_map(|r| r.row_type.as_deref()).collect();
            assert_eq!(kinds, vec!["MoveTo", "EllipticalArcTo", "EllipticalArcTo", "EllipticalArcTo", "EllipticalArcTo"], "angle {angle} flip {flip}");
            let (after, json) = oval_points(&moved);
            assert!(json["oval"] == true && near(after[1], target) && [0, 2, 3].iter().all(|i| near(after[*i], before[*i])), "angle {angle} flip {flip}: {after:?} vs {before:?}");
            assert!((num(node, "Angle").unwrap() - angle).abs() < 1e-9 && (num(node, "FlipX").unwrap_or(0.0) != 0.0) == flip, "the turn and the mirror are kept");
            // the box is the box of the four vertices (every arc is monotone), in the shape's own frame
            let frame = frame_of(node).unwrap();
            let own: Vec<(f64, f64)> = after.iter().map(|p| frame.from_parent(*p)).collect();
            let (min_x, max_x) = own.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
            let (min_y, max_y) = own.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
            assert!(min_x.abs() < 1e-9 && min_y.abs() < 1e-9 && (max_x - num(node, "Width").unwrap()).abs() < 1e-9 && (max_y - num(node, "Height").unwrap()).abs() < 1e-9);
            // a second move works on the four-arc form (it reads back as an oval) and moving the vertex home gives the first ellipse again
            let home = reopen(edit_vertices(&moved, 0, "5", VertexEdit::Move { index: 1, to: before[1] }).unwrap());
            let (back, _) = oval_points(&home);
            assert!((0..5).all(|i| near(back[i], before[i])), "angle {angle} flip {flip}: {back:?}");
        }
    }

    #[test]
    fn an_oval_refuses_inserting_deleting_bowing_and_collapsing() {
        let package = oval_drawing(0.0, false);
        assert!(edit_vertices(&package, 0, "5", VertexEdit::Insert { after: 0, at: (3.0, 3.5) }).is_err());
        assert!(edit_vertices(&package, 0, "5", VertexEdit::Delete { index: 1 }).is_err());
        assert!(edit_vertices(&package, 0, "5", VertexEdit::Bulge { segment: 0, bow: 0.3 }).is_err());
        // the top vertex above the left one's x leaves no room for the arc (rx = 0)
        assert!(edit_vertices(&package, 0, "5", VertexEdit::Move { index: 1, to: (2.0, 4.0) }).is_err());
        assert!(edit_vertices(&package, 0, "5", VertexEdit::Move { index: 9, to: (2.0, 4.0) }).is_err());
    }

    fn blank_page() -> Package {
        let blank = crate::fresh::write_blank_document(&crate::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".to_string() }).unwrap();
        Package::from_reader(std::io::Cursor::new(blank)).unwrap()
    }

    #[test]
    fn a_drawn_polyline_and_polygon_are_freeforms_with_edit_points() {
        let package = blank_page();
        let (bytes, id) = crate::draw::draw_freeform(&package, 0, &[(1.0, 1.0), (2.0, 3.0), (3.5, 1.5), (5.0, 3.0)], false).unwrap();
        let open = reopen(bytes);
        let (v, bows, closed) = freeform_segments(&open, 0, &id).unwrap();
        assert!(!closed && bows == vec![0.0; 3] && v.len() == 4, "{v:?}");
        assert!(near(v[0], (1.0, 1.0)) && near(v[1], (2.0, 3.0)) && near(v[2], (3.5, 1.5)) && near(v[3], (5.0, 3.0)), "{v:?}");
        // an open polyline has no fill; the polygon below has
        let editor = GroupEditor::open(&open, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == id).unwrap();
        let section = node.sections.iter().find(|s| s.name == "Geometry").unwrap();
        assert_eq!(section.cells.get("NoFill").and_then(|c| c.value.as_deref()), Some("1"));
        // the polyline is editable like any freeform
        let moved = reopen(edit_vertices(&open, 0, &id, VertexEdit::Move { index: 2, to: (4.0, 0.5) }).unwrap());
        let (v2, _, _) = freeform_segments(&moved, 0, &id).unwrap();
        assert!(near(v2[2], (4.0, 0.5)) && near(v2[0], v[0]) && near(v2[3], v[3]), "{v2:?}");
        // a closed polygon: the first point comes back at the end
        let (bytes, pid) = crate::draw::draw_freeform(&open, 0, &[(7.0, 1.0), (9.0, 1.0), (8.0, 3.0)], true).unwrap();
        let polygon = reopen(bytes);
        let (pv, _, pclosed) = freeform_segments(&polygon, 0, &pid).unwrap();
        assert!(pclosed && pv.len() == 4 && near(pv[0], (7.0, 1.0)) && near(pv[3], pv[0]) && near(pv[2], (8.0, 3.0)), "{pv:?}");
        let editor = GroupEditor::open(&polygon, 0).unwrap();
        let section = editor.roots.iter().find(|n| n.id == pid).unwrap().sections.iter().find(|s| s.name == "Geometry").unwrap().clone();
        assert_eq!(section.cells.get("NoFill").and_then(|c| c.value.as_deref()), Some("0"));
    }

    #[test]
    fn drawn_freeforms_refuse_unusable_points_and_accept_flat_ones() {
        let package = blank_page();
        assert!(crate::draw::draw_freeform(&package, 0, &[(1.0, 1.0)], false).is_err());
        assert!(crate::draw::draw_freeform(&package, 0, &[(1.0, 1.0), (1.0, 1.0)], false).is_err());
        assert!(crate::draw::draw_freeform(&package, 0, &[(1.0, 1.0), (2.0, 2.0)], true).is_err());
        assert!(crate::draw::draw_freeform(&package, 0, &[(1.0, f64::NAN), (2.0, 2.0)], false).is_err());
        // a horizontal polyline (no height) is a legal shape and keeps its points
        let (bytes, id) = crate::draw::draw_freeform(&package, 0, &[(1.0, 2.0), (3.0, 2.0), (5.0, 2.0)], false).unwrap();
        let flat = reopen(bytes);
        let (v, _) = freeform_vertices(&flat, 0, &id).unwrap();
        assert!(v.len() == 3 && near(v[0], (1.0, 2.0)) && near(v[2], (5.0, 2.0)), "{v:?}");
    }

    #[test]
    fn a_drawn_arc_is_a_two_vertex_freeform_with_a_bow_and_edit_points() {
        let package = blank_page();
        for bow in [0.6_f64, -0.6] {
            let (bytes, id) = crate::draw::draw_arc(&package, 0, (2.0, 2.0), (5.0, 3.0), bow).unwrap();
            let arc = reopen(bytes);
            let (v, bows, closed) = freeform_segments(&arc, 0, &id).unwrap();
            assert!(!closed && v.len() == 2 && near(v[0], (2.0, 2.0)) && near(v[1], (5.0, 3.0)), "{v:?}");
            assert!((bows[0] - bow).abs() < 1e-9, "the bow reads back: {bows:?}");
            // the box encloses the bulge: the box of the shape is taller than the chord alone
            let editor = GroupEditor::open(&arc, 0).unwrap();
            let node = editor.roots.iter().find(|n| n.id == id).unwrap();
            assert!(num(node, "Height").unwrap() > 1.0 + 0.1 || num(node, "Width").unwrap() > 3.0 + 0.1, "the box holds the arc");
            // it is editable: the bow can be changed
            let more = reopen(edit_vertices(&arc, 0, &id, VertexEdit::Bulge { segment: 0, bow: bow / 2.0 }).unwrap());
            let (_, bows2, _) = freeform_segments(&more, 0, &id).unwrap();
            assert!((bows2[0] - bow / 2.0).abs() < 1e-9);
        }
        // a zero bow is a straight line freeform; too short and absurd bows are refused
        let (bytes, id) = crate::draw::draw_arc(&package, 0, (1.0, 1.0), (3.0, 1.0), 0.0).unwrap();
        let (_, b, _) = freeform_segments(&reopen(bytes), 0, &id).unwrap();
        assert_eq!(b, vec![0.0]);
        assert!(crate::draw::draw_arc(&package, 0, (1.0, 1.0), (1.01, 1.0), 0.2).is_err());
        assert!(crate::draw::draw_arc(&package, 0, (1.0, 1.0), (3.0, 1.0), f64::NAN).is_err());
    }

    /// Visio's DrawSpline(5,1, 6,3, 7,1.5, 8,3): a cubic through four points; Visio's own box is 3 x 2.2349576 (a coarse flattening of the curve), the true extent is 2.2442.
    fn spline_drawing() -> Package {
        let blank = crate::fresh::write_blank_document(&crate::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".to_string() }).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(blank)).unwrap();
        let e = "NURBS(5.8416192529638, 3, 0, 0, 0.22494173013211,2.8094997123591,0,1, 0.65918205900835,-1.4168181999182,0,1)";
        let rows = format!("<Row T='MoveTo' IX='1'><Cell N='X' V='0' F='Width*0'/><Cell N='Y' V='0' F='Height*0'/></Row><Row T='NURBSTo' IX='2'><Cell N='X' V='3' F='Width*1'/><Cell N='Y' V='2' F='Height*0.89487156556902'/><Cell N='A' V='0'/><Cell N='B' V='1'/><Cell N='C' V='0'/><Cell N='D' V='1'/><Cell N='E' V='{e}' F='{e}'/></Row>");
        let shape = format!("<Shape ID='9' Type='Shape'><Cell N='PinX' V='6.5'/><Cell N='PinY' V='2.117478796372449'/><Cell N='Width' V='3'/><Cell N='Height' V='2.234957592744898'/><Cell N='LocPinX' V='1.5'/><Cell N='LocPinY' V='1.117478796372449'/><Cell N='Angle' V='0'/><Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/><Cell N='NoLine' V='0'/>{rows}</Section></Shape>");
        let part = package.pages().unwrap()[0].part.clone();
        let mut xml = package.text(&part).unwrap().to_string();
        let at = xml.rfind("</Shapes>").unwrap();
        xml.insert_str(at, &shape);
        reopen(crate::writer::with_part(&package, &part, xml.into_bytes()).unwrap())
    }

    #[test]
    fn the_box_of_a_curve_is_the_true_extent_of_the_curve() {
        let package = spline_drawing();
        let editor = GroupEditor::open(&package, 0).unwrap();
        let before = describe_vertices(editor.roots.iter().find(|n| n.id == "9").unwrap());
        let c1 = (before["points"][1][0].as_f64().unwrap(), before["points"][1][1].as_f64().unwrap());
        // moving a control point onto itself refits the box: 3 wide, 2.2442 high (the curve's real top), the curve unchanged on the page
        let again = reopen(edit_vertices(&package, 0, "9", VertexEdit::Move { index: 1, to: c1 }).unwrap());
        let editor = GroupEditor::open(&again, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == "9").unwrap();
        assert!((num(node, "Width").unwrap() - 3.0).abs() < 1e-6, "{:?}", num(node, "Width"));
        assert!((num(node, "Height").unwrap() - 2.2441993).abs() < 1e-3, "height {:?}", num(node, "Height"));
        let after = describe_vertices(node);
        for i in 0..4 {
            for k in 0..2 {
                assert!((after["points"][i][k].as_f64().unwrap() - before["points"][i][k].as_f64().unwrap()).abs() < 1e-6, "point {i} moved");
            }
        }
        // the box holds the curve: evaluate it from the stored numbers and compare with the shape's own box
        let curve = read_curve(node).unwrap();
        let (sx, sy) = curve.scale();
        let mut local = vec![curve.start];
        for i in 0..curve.controls() {
            local.push((curve.nums[4 + 4 * i] * sx, curve.nums[5 + 4 * i] * sy));
        }
        local.push(curve.end);
        let samples = curve.samples(&local, 500).unwrap();
        let (h, w) = (num(node, "Height").unwrap(), num(node, "Width").unwrap());
        assert!(samples.iter().all(|p| p.0 >= -1e-6 && p.0 <= w + 1e-6 && p.1 >= -1e-6 && p.1 <= h + 1e-6), "the curve stays inside its box");
        assert!(samples.iter().any(|p| (p.1 - h).abs() < 1e-3) && samples.iter().any(|p| p.1.abs() < 1e-3), "and touches its top and bottom");
    }

    fn dist_to_outline(outline: &[serde_json::Value], p: (f64, f64)) -> f64 {
        outline.iter().map(|q| ((q[0].as_f64().unwrap() - p.0).powi(2) + (q[1].as_f64().unwrap() - p.1).powi(2)).sqrt()).fold(f64::MAX, f64::min)
    }

    #[test]
    fn a_drawn_curve_is_visios_spline_through_the_points() {
        let package = blank_page();
        let data = [(1.0, 1.0), (2.0, 3.0), (3.0, 1.2), (4.0, 3.4), (5.0, 1.1), (6.0, 3.0)];
        let (bytes, id) = crate::draw::draw_curve(&package, 0, &data).unwrap();
        let curve = reopen(bytes);
        let editor = GroupEditor::open(&curve, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == id).unwrap();
        let json = describe_vertices(node);
        assert!(json["curve"] == true && json["points"].as_array().unwrap().len() == 6, "{json}");
        // the control points are the ones Visio wrote for the same data (page inches; Visio's box is 5 x 2.6606 at (1, 0.768))
        let visio = [(0.10750522230459, 2.1528102400315), (0.36035192306465, -1.0763151392742), (0.65986385524773, 2.3149486169582), (0.83865313383354, -1.2086272626759)];
        for (i, (fx, fy)) in visio.iter().enumerate() {
            let (px, py) = (1.0 + fx * 4.999999999999998, 0.7680083191620808 + fy * 2.660603392887615);
            let got = (json["points"][i + 1][0].as_f64().unwrap(), json["points"][i + 1][1].as_f64().unwrap());
            assert!((got.0 - px).abs() < 1e-6 && (got.1 - py).abs() < 1e-6, "control {}: {got:?} vs Visio ({px}, {py})", i + 1);
        }
        // the curve passes through every data point
        let outline = json["outline"].as_array().unwrap();
        for d in data {
            assert!(dist_to_outline(outline, d) < 0.07, "the curve misses {d:?}");
        }
        // a NURBSTo row (no other geometry), no fill, editable like any Visio curve
        let section = node.sections.iter().find(|s| s.name == "Geometry").unwrap();
        assert_eq!(section.cells.get("NoFill").and_then(|c| c.value.as_deref()), Some("1"));
        let moved = reopen(edit_vertices(&curve, 0, &id, VertexEdit::Move { index: 2, to: (3.0, 0.5) }).unwrap());
        let editor = GroupEditor::open(&moved, 0).unwrap();
        let after = describe_vertices(editor.roots.iter().find(|n| n.id == id).unwrap());
        assert!((after["points"][2][1].as_f64().unwrap() - 0.5).abs() < 1e-6 && (after["points"][1][0].as_f64().unwrap() - json["points"][1][0].as_f64().unwrap()).abs() < 1e-6);
        // refusals
        assert!(crate::draw::draw_curve(&package, 0, &data[..3]).is_err());
        assert!(crate::draw::draw_curve(&package, 0, &[(1.0, 1.0), (1.0, 1.0), (2.0, 2.0), (3.0, 1.0), (4.0, 2.0)]).is_err());
        // a straight horizontal stroke is a legal curve (flat axis in inches)
        let (flat, fid) = crate::draw::draw_curve(&package, 0, &[(1.0, 2.0), (2.0, 2.0), (3.5, 2.0), (5.0, 2.0)]).unwrap();
        let editor = GroupEditor::open(&reopen(flat), 0).unwrap();
        let fj = describe_vertices(editor.roots.iter().find(|n| n.id == fid).unwrap());
        assert!((fj["points"][3][0].as_f64().unwrap() - 5.0).abs() < 1e-6 && (fj["points"][3][1].as_f64().unwrap() - 2.0).abs() < 1e-6, "{fj}");
    }

    #[test]
    fn fit_curve_turns_an_open_polyline_into_a_spline_through_its_vertices() {
        let package = blank_page();
        let pts = [(1.0, 1.0), (2.0, 2.5), (3.5, 1.5), (5.0, 3.0), (6.0, 1.2)];
        let (bytes, id) = crate::draw::draw_freeform(&package, 0, &pts, false).unwrap();
        let line = reopen(bytes);
        let turned_pkg = turned(&line, 0.5, false);
        for (name, pkg) in [("upright", &line), ("turned", &turned_pkg)] {
            let (before, _, _) = freeform_segments(pkg, 0, &id).unwrap();
            let fitted = reopen(fit_curve(pkg, 0, &id).unwrap());
            let editor = GroupEditor::open(&fitted, 0).unwrap();
            let node = editor.roots.iter().find(|n| n.id == id).unwrap();
            let json = describe_vertices(node);
            assert!(json["curve"] == true, "{name}: {json}");
            let outline = json["outline"].as_array().unwrap();
            for v in &before {
                assert!(dist_to_outline(outline, *v) < 0.07, "{name}: the curve misses {v:?}");
            }
            assert!((num(node, "Angle").unwrap() - if name == "turned" { 0.5 } else { 0.0 }).abs() < 1e-9, "{name}: the turn is kept");
        }
        // refusals: a closed polygon, an arc, an oval, a curve, a short polyline
        let (poly, pid) = crate::draw::draw_freeform(&package, 0, &[(1.0, 1.0), (4.0, 1.0), (4.0, 3.0), (1.0, 3.0), (2.0, 2.0)], true).unwrap();
        assert!(fit_curve(&reopen(poly), 0, &pid).is_err());
        let (arc, aid) = crate::draw::draw_arc(&package, 0, (1.0, 1.0), (5.0, 2.0), 0.5).unwrap();
        assert!(fit_curve(&reopen(arc), 0, &aid).is_err());
        assert!(fit_curve(&oval_drawing(0.0, false), 0, "5").is_err());
        let (short, sid) = crate::draw::draw_freeform(&package, 0, &[(1.0, 1.0), (2.0, 2.0), (3.0, 1.0)], false).unwrap();
        assert!(fit_curve(&reopen(short), 0, &sid).is_err());
        let again = reopen(fit_curve(&line, 0, &id).unwrap());
        assert!(fit_curve(&again, 0, &id).is_err(), "a curve cannot be fitted again");
    }

    fn draw_line(package: &Package, a: (f64, f64), b: (f64, f64)) -> (Package, String) {
        let (bytes, id) = crate::draw::draw_shape(package, 0, crate::draw::DrawKind::Line, a.0, a.1, b.0, b.1).unwrap();
        (reopen(bytes), id)
    }

    fn join(package: &Package, ids: &[&str]) -> Result<(Package, Vec<String>)> {
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        let (bytes, new_ids) = crate::groupedit::shapeops::shape_operation(package, 0, crate::groupedit::shapeops::ShapeOp::Join, &ids)?;
        Ok((reopen(bytes), new_ids))
    }

    fn page_shape_count(package: &Package) -> usize {
        GroupEditor::open(package, 0).unwrap().roots.len()
    }

    #[test]
    fn join_chains_lines_whose_ends_touch_into_one_shape() {
        let package = blank_page();
        let (package, a) = draw_line(&package, (1.0, 1.0), (3.0, 2.0));
        let (package, b) = draw_line(&package, (5.0, 1.2), (3.0, 2.0)); // drawn the other way round
        let (package, c) = draw_line(&package, (5.0, 1.2), (6.0, 3.0));
        let (joined, ids) = join(&package, &[&a, &b, &c]).unwrap();
        assert!(ids.len() == 1 && page_shape_count(&joined) == 1, "{ids:?}");
        let (v, bows, closed) = freeform_segments(&joined, 0, &ids[0]).unwrap();
        assert!(!closed && bows == vec![0.0; 3], "{v:?}");
        // one chain from one end to the other (either direction): the four points of the three lines
        let want = [(1.0, 1.0), (3.0, 2.0), (5.0, 1.2), (6.0, 3.0)];
        let forward = want.iter().zip(&v).all(|(a, b)| near(*a, *b));
        let backward = want.iter().rev().zip(&v).all(|(a, b)| near(*a, *b));
        assert!(v.len() == 4 && (forward || backward), "{v:?}");
        // the result is an ordinary 2-D shape without fill; its points can be edited (Edit Points reads the relative rows)
        let editor = GroupEditor::open(&joined, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == ids[0]).unwrap();
        assert!(node.cells.get("BeginX").is_none(), "a 2-D shape, no Begin / End cells");
        let section = node.sections.iter().find(|s| s.name == "Geometry").unwrap();
        assert_eq!(section.cells.get("NoFill").and_then(|c| c.value.as_deref()), Some("1"));
        let moved = reopen(edit_vertices(&joined, 0, &ids[0], VertexEdit::Move { index: 1, to: (3.0, 3.0) }).unwrap());
        let (v2, _, _) = freeform_segments(&moved, 0, &ids[0]).unwrap();
        assert!(near(v2[1], (3.0, 3.0)) && near(v2[0], v[0]) && near(v2[3], v[3]), "{v2:?}");
    }

    #[test]
    fn join_closes_a_chain_into_a_filled_polygon_and_keeps_what_does_not_touch() {
        let package = blank_page();
        let (package, a) = draw_line(&package, (1.0, 1.0), (3.0, 1.0));
        let (package, b) = draw_line(&package, (3.0, 1.0), (2.0, 3.0));
        let (package, c) = draw_line(&package, (2.0, 3.0), (1.0, 1.0));
        let (triangle, ids) = join(&package, &[&a, &b, &c]).unwrap();
        let (v, _, closed) = freeform_segments(&triangle, 0, &ids[0]).unwrap();
        assert!(closed && v.len() == 4 && near(v[0], v[3]), "{v:?}");
        let editor = GroupEditor::open(&triangle, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == ids[0]).unwrap();
        let section = node.sections.iter().find(|s| s.name == "Geometry").unwrap();
        assert_eq!(section.cells.get("NoFill").and_then(|c| c.value.as_deref()), Some("0"), "a closed chain is filled");
        // a gap of 0.001 in does not join (Visio: 1e-6 joins, 1e-3 does not): ONE shape with TWO geometry sections
        let (p2, d) = draw_line(&blank_page(), (1.0, 1.0), (3.0, 2.0));
        let (p2, e) = draw_line(&p2, (3.001, 2.0), (5.0, 1.2));
        let (apart, ids2) = join(&p2, &[&d, &e]).unwrap();
        assert!(ids2.len() == 1 && page_shape_count(&apart) == 1);
        let editor = GroupEditor::open(&apart, 0).unwrap();
        let node = editor.roots.iter().find(|n| n.id == ids2[0]).unwrap();
        assert_eq!(node.sections.iter().filter(|s| s.name == "Geometry").count(), 2, "two paths, two sections");
        // a single line becomes a 2-D shape; a group and an unselectable id are refused
        let (p3, f) = draw_line(&blank_page(), (1.0, 1.0), (3.0, 2.0));
        let (single, ids3) = join(&p3, &[&f]).unwrap();
        let editor = GroupEditor::open(&single, 0).unwrap();
        assert!(editor.roots.iter().find(|n| n.id == ids3[0]).unwrap().cells.get("BeginX").is_none());
        assert!(join(&p3, &["99"]).is_err());
        assert!(join(&p3, &[]).is_err());
        assert!(join(&p3, &[&f, &f]).is_err());
    }

    fn trim(package: &Package, ids: &[&str]) -> Result<(Package, Vec<String>)> {
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        let (bytes, new_ids) = crate::groupedit::shapeops::shape_operation(package, 0, crate::groupedit::shapeops::ShapeOp::Trim, &ids)?;
        Ok((reopen(bytes), new_ids))
    }

    /// Every shape of the page as a text: its vertices (an open path in the smaller direction) so two pages can be compared as sets.
    fn page_paths(package: &Package) -> Vec<String> {
        let editor = GroupEditor::open(package, 0).unwrap();
        let mut out: Vec<String> = editor
            .roots
            .iter()
            .map(|n| {
                let (v, _, closed) = freeform_segments(package, 0, &n.id).unwrap();
                let f: Vec<String> = v.iter().map(|p| format!("({:.2},{:.2})", p.0, p.1)).collect();
                let mut b = f.clone();
                b.reverse();
                if closed {
                    let mut sorted = f.clone();
                    sorted.sort();
                    format!("closed {}", sorted.join(""))
                } else {
                    format!("open {}", if f.concat() <= b.concat() { f.concat() } else { b.concat() })
                }
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn trim_cuts_crossing_lines_and_t_junctions_like_visio() {
        // an X of two lines: four pieces, each of the lines cut in two
        let (p, a) = draw_line(&blank_page(), (1.0, 1.0), (5.0, 3.0));
        let (p, b) = draw_line(&p, (1.0, 3.0), (5.0, 1.0));
        let (x, ids) = trim(&p, &[&a, &b]).unwrap();
        assert_eq!(ids.len(), 4);
        assert_eq!(page_paths(&x), vec!["open (1.00,1.00)(3.00,2.00)", "open (1.00,3.00)(3.00,2.00)", "open (3.00,2.00)(5.00,1.00)", "open (3.00,2.00)(5.00,3.00)"]);
        // every piece is a one-dimensional line
        let editor = GroupEditor::open(&x, 0).unwrap();
        assert!(editor.roots.iter().all(|n| n.cells.contains_key("BeginX")));
        // a T junction: the line that ends on the other one cuts it, itself stays (3 shapes in all)
        let (p, h) = draw_line(&blank_page(), (1.0, 3.0), (5.0, 3.0));
        let (p, v) = draw_line(&p, (3.0, 1.0), (3.0, 3.0));
        let (t, ids) = trim(&p, &[&h, &v]).unwrap();
        assert_eq!(ids.len(), 2, "only the horizontal line is cut");
        assert_eq!(page_paths(&t), vec!["open (1.00,3.00)(3.00,3.00)", "open (3.00,1.00)(3.00,3.00)", "open (3.00,3.00)(5.00,3.00)"]);
        // lines that do not touch: nothing to trim
        let (p, c) = draw_line(&blank_page(), (1.0, 1.0), (3.0, 1.0));
        let (p, d) = draw_line(&p, (5.0, 5.0), (7.0, 5.0));
        assert!(trim(&p, &[&c, &d]).is_err());
        assert!(trim(&p, &[&c]).is_err());
    }

    #[test]
    fn trim_cuts_a_closed_outline_at_its_seam_and_keeps_the_look() {
        // the rectangle (2,2)-(4,4) crossed by the line y = 3: the line in 3 pieces, the outline in 3 (two 2-D paths and a one-segment line)
        let (p, r) = {
            let (bytes, id) = crate::draw::draw_shape(&blank_page(), 0, crate::draw::DrawKind::Rectangle, 2.0, 2.0, 4.0, 4.0).unwrap();
            (reopen(bytes), id)
        };
        let (p, l) = draw_line(&p, (1.0, 3.0), (5.0, 3.0));
        let (t, ids) = trim(&p, &[&r, &l]).unwrap();
        assert_eq!(ids.len(), 6);
        assert_eq!(
            page_paths(&t),
            vec![
                "open (1.00,3.00)(2.00,3.00)",
                "open (2.00,2.00)(2.00,3.00)",
                "open (2.00,2.00)(4.00,2.00)(4.00,3.00)",
                "open (2.00,3.00)(2.00,4.00)(4.00,4.00)(4.00,3.00)",
                "open (2.00,3.00)(4.00,3.00)",
                "open (4.00,3.00)(5.00,3.00)"
            ]
        );
        // the line starts inside the rectangle: it meets the outline once, the outline is cut once (2 pieces) and the line in 2
        let (p, r) = {
            let (bytes, id) = crate::draw::draw_shape(&blank_page(), 0, crate::draw::DrawKind::Rectangle, 2.0, 2.0, 4.0, 4.0).unwrap();
            (reopen(bytes), id)
        };
        let (p, l) = draw_line(&p, (3.0, 3.0), (6.0, 3.0));
        let (t, ids) = trim(&p, &[&r, &l]).unwrap();
        assert_eq!(ids.len(), 4);
        let _ = t;
    }

    #[test]
    fn the_pieces_of_a_trimmed_line_keep_its_look() {
        let (p, a) = draw_line(&blank_page(), (1.0, 1.0), (5.0, 3.0));
        let (p, b) = draw_line(&p, (1.0, 3.0), (5.0, 1.0));
        // the first line (the first shape of the page) is red
        let part = p.pages().unwrap()[0].part.clone();
        let xml = p.text(&part).unwrap().replacen("<Cell N='ObjType' V='2'/>", "<Cell N='LineColor' V='#ff0000'/><Cell N='ObjType' V='2'/>", 1);
        let p = reopen(crate::writer::with_part(&p, &part, xml.into_bytes()).unwrap());
        let (t, _) = trim(&p, &[&a, &b]).unwrap();
        let part = t.pages().unwrap()[0].part.clone();
        let xml = t.text(&part).unwrap().to_string();
        assert_eq!(xml.matches("N='LineColor' V='#ff0000'").count(), 2, "the two pieces of the red line are red");
        assert_eq!(xml.matches("N='LineColor'").count(), 2, "only the pieces of the red line carry a colour of their own");
    }
}
