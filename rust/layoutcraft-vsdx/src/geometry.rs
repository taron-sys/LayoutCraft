//! Visio Geometry row → SVG path conversion.
//!
//! Evidence: the row types (`MoveTo`, `LineTo`, `ArcTo`, `EllipticalArcTo`, `NURBSTo`, `PolylineTo`, `SplineStart`, ...) and their cells are documented in [MS-VSDX];
//! how Visio samples arcs and splines was checked by page renders (visual conformance checks) and, for NURBS, probe-nurbs.ps1 / check-outline-in-png.py (the NURBS rules are in `groupedit/nurbs.rs`).
//!
//! The browser importer evaluates the cached geometry cells and samples the
//! same arcs/splines that Visio draws. This module ports that path builder so
//! imported part paths, not just their paint, match the canonical models.
use crate::shapes::{Cell, Section, SectionRow};
use std::f64::consts::PI;

const DPI: f64 = 96.0;

#[derive(Debug, Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy)]
struct Control {
    x: f64,
    y: f64,
    w: f64,
}

impl Control {
    fn point(x: f64, y: f64) -> Self {
        Self { x, y, w: 1.0 }
    }
}

#[derive(Debug, Clone)]
struct SplineState {
    controls: Vec<Control>,
    degree: usize,
    knots: Vec<f64>,
    last_knot: f64,
}

struct NurbsParsed {
    knot_last: f64,
    degree: f64,
    x_type: bool,
    y_type: bool,
    points: Vec<NurbsPoint>,
}

struct NurbsPoint {
    x: f64,
    y: f64,
    knot: f64,
    weight: f64,
}

struct NurbsCurve {
    controls: Vec<Control>,
    degree: usize,
    knots: Option<Vec<f64>>,
}

struct PathBuilder {
    d: String,
    started: bool,
    cur: Option<Point>,
}

impl PathBuilder {
    fn new() -> Self {
        Self {
            d: String::new(),
            started: false,
            cur: None,
        }
    }

    fn point(&mut self, command: &str, x: f64, y: f64) {
        // Visio stencils often use `MoveTo` rows that land on the current pen
        // point to split a continuous contour. Emitting `M` there would make
        // independent subpaths, and with the even-odd fill rule overlapping
        // subpaths cancel (the computer-network monitor screens collapsed to
        // a thin strip). A zero-length move is a continuation, not a new
        // contour.
        if command == "M" {
            if let Some(cur) = self.cur {
                if (x - cur.x).hypot(y - cur.y) < 0.01 {
                    return;
                }
            }
        }
        let command = if self.started { command } else { "M" };
        self.d.push_str(command);
        self.d.push_str(&num_path(x));
        self.d.push(' ');
        self.d.push_str(&num_path(y));
        self.started = true;
        self.cur = Some(Point { x, y });
    }

    fn append_curve<F>(
        &mut self,
        controls: &[Control],
        degree: usize,
        knots: Option<&[f64]>,
        map: &F,
    ) where
        F: Fn(f64, f64) -> (f64, f64),
    {
        let per_span = ((96.0 / (controls.len().max(1) - 1) as f64).round() as usize).clamp(8, 24);
        let points = sample_spline(controls, degree, knots, per_span);
        for point in points.iter().skip(1) {
            let (x, y) = map(point.x, point.y);
            self.point("L", x, y);
        }
    }

    fn append_periodic<F>(
        &mut self,
        controls: &[Control],
        degree: usize,
        base: f64,
        step: f64,
        map: &F,
    ) where
        F: Fn(f64, f64) -> (f64, f64),
    {
        let n = controls.len();
        let p = degree.clamp(1, n.saturating_sub(1));
        if n <= p || !step.is_finite() || step <= 0.0 {
            self.append_curve(controls, degree, None, map);
            return;
        }
        let mut extended = Vec::with_capacity(n * 3);
        for _ in 0..3 {
            extended.extend_from_slice(controls);
        }
        let mut full = Vec::with_capacity(extended.len() + p + 1);
        for index in 0..extended.len() + p + 1 {
            full.push(base + index as f64 * step);
        }
        let per = ((240.0 / n.max(1) as f64).round() as usize).clamp(8, 24);
        let mut points = Vec::new();
        for span in 0..n {
            let start = base + (n + span) as f64 * step;
            for index in (if span == 0 { 0 } else { 1 })..=per {
                points.push(nurbs_point(
                    &extended,
                    p,
                    Some(&full),
                    start + step * index as f64 / per as f64,
                ));
            }
        }
        let Some(first) = points.first() else {
            return;
        };
        let (x, y) = map(first.x, first.y);
        self.point("M", x, y);
        for point in points.iter().skip(1) {
            let (x, y) = map(point.x, point.y);
            self.point("L", x, y);
        }
    }
}

pub fn path_from_section(
    section: &Section,
    w_in: f64,
    h_in: f64,
    close: bool,
    page_space: bool,
) -> String {
    // A page shape's geometry has y running up; a master's own coordinate
    // space is stored top-down (Visio's master-map branch).
    let map = |x: f64, y: f64| {
        if page_space {
            (x * DPI, (h_in - y) * DPI)
        } else {
            (x * DPI, y * DPI)
        }
    };
    let rel = |x: f64, y: f64| map(x * w_in, y * h_in);
    path_from_section_with(section, close, map, rel, (w_in, h_in))
}

pub fn path_from_section_with<F, G>(
    section: &Section,
    close: bool,
    map: F,
    rel: G,
    rel_size: (f64, f64),
) -> String
where
    F: Fn(f64, f64) -> (f64, f64),
    G: Fn(f64, f64) -> (f64, f64),
{
    let mut rows: Vec<&SectionRow> = section.rows.iter().collect();
    rows.sort_by(|a, b| {
        let key = |row: &SectionRow| {
            row.index
                .as_deref()
                .and_then(|index| index.parse::<f64>().ok())
                .unwrap_or(f64::INFINITY)
        };
        key(a)
            .partial_cmp(&key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut builder = PathBuilder::new();
    let mut cur_local: Option<Point> = None;
    let mut spline: Option<SplineState> = None;
    let mut has_infinite = false;

    for row in rows {
        let kind = row.row_type.as_deref().unwrap_or("");
        if spline.is_some() && kind != "SplineKnot" && kind != "SplineEnd" {
            flush_spline(&mut spline, &mut builder, &map);
        }
        let gx = row_num(row, "X").unwrap_or(0.0);
        let gy = row_num(row, "Y").unwrap_or(0.0);
        match kind {
            "RelMoveTo" | "RelLineTo" => {
                let (mx, my) = rel(gx, gy);
                cur_local = Some(Point {
                    x: gx * rel_size.0,
                    y: gy * rel_size.1,
                });
                builder.point(if kind == "RelMoveTo" { "M" } else { "L" }, mx, my);
            }
            "MoveTo" | "LineTo" => {
                let (mx, my) = map(gx, gy);
                cur_local = Some(Point { x: gx, y: gy });
                builder.point(if kind == "MoveTo" { "M" } else { "L" }, mx, my);
            }
            "CubBezTo" | "RelCubBezTo" => {
                // X/Y is the end point; A/B and C/D are the two Bézier
                // control points. The Rel* rows store every coordinate as a
                // fraction of the shape's width/height.
                let relative = kind == "RelCubBezTo";
                let to = |x: f64, y: f64| {
                    if relative {
                        rel(x, y)
                    } else {
                        map(x, y)
                    }
                };
                let (x1, y1) = to(
                    row_num(row, "A").unwrap_or(gx),
                    row_num(row, "B").unwrap_or(gy),
                );
                let (x2, y2) = to(
                    row_num(row, "C").unwrap_or(gx),
                    row_num(row, "D").unwrap_or(gy),
                );
                let (ex, ey) = to(gx, gy);
                cur_local = Some(Point {
                    x: if relative { gx * rel_size.0 } else { gx },
                    y: if relative { gy * rel_size.1 } else { gy },
                });
                if builder.cur.is_some() {
                    builder.d.push_str(&format!(
                        "C{} {} {} {} {} {}",
                        num_path(x1),
                        num_path(y1),
                        num_path(x2),
                        num_path(y2),
                        num_path(ex),
                        num_path(ey)
                    ));
                    builder.cur = Some(Point { x: ex, y: ey });
                } else {
                    builder.point("M", ex, ey);
                }
            }
            "QuadBezTo" | "RelQuadBezTo" => {
                let relative = kind == "RelQuadBezTo";
                let to = |x: f64, y: f64| {
                    if relative {
                        rel(x, y)
                    } else {
                        map(x, y)
                    }
                };
                let (cx, cy) = to(
                    row_num(row, "A").unwrap_or(gx),
                    row_num(row, "B").unwrap_or(gy),
                );
                let (ex, ey) = to(gx, gy);
                cur_local = Some(Point {
                    x: if relative { gx * rel_size.0 } else { gx },
                    y: if relative { gy * rel_size.1 } else { gy },
                });
                if builder.cur.is_some() {
                    builder.d.push_str(&format!(
                        "Q{} {} {} {}",
                        num_path(cx),
                        num_path(cy),
                        num_path(ex),
                        num_path(ey)
                    ));
                    builder.cur = Some(Point { x: ex, y: ey });
                } else {
                    builder.point("M", ex, ey);
                }
            }
            "ArcTo" => {
                let end = Point { x: gx, y: gy };
                let start = cur_local.unwrap_or(end);
                let bow = row_num(row, "A").unwrap_or(0.0);
                if let Some(points) = circular_arc_points(start, end, bow) {
                    for point in points.iter().skip(1) {
                        let (x, y) = map(point.x, point.y);
                        builder.point("L", x, y);
                    }
                } else {
                    let (x, y) = map(gx, gy);
                    builder.point("L", x, y);
                }
                cur_local = Some(end);
            }
            "Ellipse" => {
                let (cx, cy) = map(gx, gy);
                // (A, B) and (C, D) are points on the two axes; either may be the horizontal one.
                let p1 = map(row_num(row, "A").unwrap_or(gx), row_num(row, "B").unwrap_or(gy));
                let p2 = map(row_num(row, "C").unwrap_or(gx), row_num(row, "D").unwrap_or(gy));
                let rx = (p1.0 - cx).abs().max((p2.0 - cx).abs());
                let ry = (p1.1 - cy).abs().max((p2.1 - cy).abs());
                cur_local = Some(Point { x: gx, y: gy });
                // Neither point on an axis: the two are the ends of conjugate diameters of a
                // tilted ellipse, which is the affine image of a circle (four Beziers are exact).
                let (v1, v2) = ((p1.0 - cx, p1.1 - cy), (p2.0 - cx, p2.1 - cy));
                let tol = 1e-6 * (1.0 + rx + ry);
                let on_axes = (v1.1.abs() < tol && v2.0.abs() < tol) || (v1.0.abs() < tol && v2.1.abs() < tol);
                if !on_axes {
                    const K: f64 = 0.552_284_749_830_793_4;
                    let at = |a: f64, b: f64| (cx + v1.0 * a + v2.0 * b, cy + v1.1 * a + v2.1 * b);
                    let start = at(1.0, 0.0);
                    builder.point("M", start.0, start.1);
                    let quads = [((1.0, K), (K, 1.0), (0.0, 1.0)), ((-K, 1.0), (-1.0, K), (-1.0, 0.0)), ((-1.0, -K), (-K, -1.0), (0.0, -1.0)), ((K, -1.0), (1.0, -K), (1.0, 0.0))];
                    for (c1, c2, e) in quads {
                        let (c1, c2, e) = (at(c1.0, c1.1), at(c2.0, c2.1), at(e.0, e.1));
                        builder.d.push_str(&format!(
                            "C{} {} {} {} {} {}",
                            num_path(c1.0), num_path(c1.1), num_path(c2.0), num_path(c2.1), num_path(e.0), num_path(e.1)
                        ));
                    }
                    builder.d.push('Z');
                    continue;
                }
                builder.point("M", cx - rx, cy);
                builder.d.push_str(&format!(
                    "A{} {} 0 1 0 {} {}A{} {} 0 1 0 {} {}Z",
                    num_path(rx),
                    num_path(ry),
                    num_path(cx + rx),
                    num_path(cy),
                    num_path(rx),
                    num_path(ry),
                    num_path(cx - rx),
                    num_path(cy)
                ));
            }
            "EllipticalArcTo" | "RelEllipticalArcTo" => {
                // Rel* rows store X/Y and the arc's A/B point as fractions of
                // the shape's width/height; C (angle) and D (axis ratio) stay
                // dimensionless. Azure/stencil glyphs ("SQL") use relative
                // arcs heavily, so dropping them shreds the letters.
                let relative = kind == "RelEllipticalArcTo";
                let (sx, sy) = if relative {
                    (rel_size.0, rel_size.1)
                } else {
                    (1.0, 1.0)
                };
                let end = Point {
                    x: gx * sx,
                    y: gy * sy,
                };
                let start = cur_local.unwrap_or(Point { x: 0.0, y: 0.0 });
                let mid = Point {
                    x: row_num(row, "A")
                        .unwrap_or(if relative { gx } else { start.x })
                        * sx,
                    y: row_num(row, "B")
                        .unwrap_or(if relative { gy } else { start.y })
                        * sy,
                };
                let theta = row_num(row, "C").unwrap_or(0.0);
                let ratio = row_num(row, "D").unwrap_or(1.0);
                if let Some(points) = elliptical_arc_points(start, end, mid, theta, ratio) {
                    for point in points.iter().skip(1) {
                        let (x, y) = map(point.x, point.y);
                        builder.point("L", x, y);
                    }
                } else {
                    let (ex, ey) = map(end.x, end.y);
                    if let Some(cur) = builder.cur {
                        let (mx, my) = map(mid.x, mid.y);
                        let qx = 2.0 * mx - 0.5 * (cur.x + ex);
                        let qy = 2.0 * my - 0.5 * (cur.y + ey);
                        builder.d.push_str(&format!(
                            "Q{} {} {} {}",
                            num_path(qx),
                            num_path(qy),
                            num_path(ex),
                            num_path(ey)
                        ));
                        builder.cur = Some(Point { x: ex, y: ey });
                    } else {
                        builder.point("L", ex, ey);
                    }
                }
                cur_local = Some(end);
            }
            "NURBSTo" => {
                let end = Point { x: gx, y: gy };
                let parsed = parse_nurbs_formula(cell_text(row, "E").as_deref());
                let start = cur_local.unwrap_or(Point { x: 0.0, y: 0.0 });
                let curve = nurbs_curve(
                    parsed.as_ref(),
                    start,
                    end,
                    row_num(row, "C").unwrap_or(0.0),
                    row_num(row, "A").unwrap_or(0.0),
                    row_num(row, "D").unwrap_or(1.0),
                    row_num(row, "B").unwrap_or(1.0),
                    rel_size.0,
                    rel_size.1,
                );
                if curve.controls.len() > 2 {
                    builder.append_curve(
                        &curve.controls,
                        curve.degree,
                        curve.knots.as_deref(),
                        &map,
                    );
                } else {
                    let (x, y) = map(gx, gy);
                    builder.point("L", x, y);
                }
                cur_local = Some(end);
            }
            "SplineStart" => {
                let start = cur_local.unwrap_or(Point { x: gx, y: gy });
                let degree = {
                    let rounded = row_num(row, "D").unwrap_or(3.0).round();
                    if rounded == 0.0 || !rounded.is_finite() {
                        3
                    } else {
                        rounded as usize
                    }
                };
                spline = Some(SplineState {
                    controls: vec![Control::point(start.x, start.y), Control::point(gx, gy)],
                    degree: degree.max(1),
                    knots: vec![
                        row_num(row, "B").unwrap_or(0.0),
                        row_num(row, "A").unwrap_or(0.0),
                    ],
                    last_knot: row_num(row, "C").unwrap_or(0.0),
                });
                cur_local = Some(Point { x: gx, y: gy });
            }
            "SplineKnot" => {
                if let Some(state) = &mut spline {
                    state.controls.push(Control::point(gx, gy));
                    state.knots.push(row_num(row, "A").unwrap_or(0.0));
                    cur_local = Some(Point { x: gx, y: gy });
                } else {
                    let (x, y) = map(gx, gy);
                    cur_local = Some(Point { x: gx, y: gy });
                    builder.point("L", x, y);
                }
            }
            "SplineEnd" => {
                if let Some(state) = &mut spline {
                    state.knots.push(state.last_knot);
                    flush_spline(&mut spline, &mut builder, &map);
                }
            }
            "InfiniteLine" => {
                let p1 = Point { x: gx, y: gy };
                let p2 = Point {
                    x: row_num(row, "A").unwrap_or(gx + 1.0),
                    y: row_num(row, "B").unwrap_or(gy),
                };
                let dx = p2.x - p1.x;
                let dy = p2.y - p1.y;
                let length = dx.hypot(dy);
                if length > 1e-9 {
                    let ext = 240.0;
                    let (x1, y1) = map(p1.x - dx / length * ext, p1.y - dy / length * ext);
                    let (x2, y2) = map(p2.x + dx / length * ext, p2.y + dy / length * ext);
                    builder.point("M", x1, y1);
                    builder.point("L", x2, y2);
                }
                cur_local = Some(p2);
                has_infinite = true;
            }
            "PolylineTo" => {
                let end = Point { x: gx, y: gy };
                // The formula may hold unevaluated terms (`Width*0.24`); then the cached value, in
                // inches, is the evaluated polyline.
                let parsed = parse_polyline_formula(cell_text(row, "A").as_deref())
                    .or_else(|| parse_polyline_formula(row.cells.get("A").and_then(|c| c.value.as_deref())));
                let mut last = cur_local;
                if let Some(parsed) = parsed {
                    for point in parsed.points {
                        let lx = if parsed.x_type {
                            point.x
                        } else {
                            point.x * rel_size.0
                        };
                        let ly = if parsed.y_type {
                            point.y
                        } else {
                            point.y * rel_size.1
                        };
                        if let Some(previous) = last {
                            if (lx - previous.x).abs() < 1e-9 && (ly - previous.y).abs() < 1e-9 {
                                last = Some(Point { x: lx, y: ly });
                                continue;
                            }
                        }
                        let (x, y) = map(lx, ly);
                        builder.point("L", x, y);
                        last = Some(Point { x: lx, y: ly });
                    }
                }
                if last.is_none_or(|previous| {
                    (end.x - previous.x).abs() > 1e-9 || (end.y - previous.y).abs() > 1e-9
                }) {
                    let (x, y) = map(gx, gy);
                    builder.point("L", x, y);
                }
                cur_local = Some(end);
            }
            _ => {}
        }
    }
    flush_spline(&mut spline, &mut builder, &map);

    if has_infinite {
        return builder.d;
    }
    if close && !builder.d.is_empty() && !builder.d.ends_with('Z') {
        builder.d.push('Z');
    }
    builder.d
}

fn flush_spline<F>(spline: &mut Option<SplineState>, builder: &mut PathBuilder, map: &F)
where
    F: Fn(f64, f64) -> (f64, f64),
{
    let Some(state) = spline.take() else {
        return;
    };
    if state.controls.len() < 2 {
        return;
    }
    let knots = &state.knots;
    let uniform = knots.len() == state.controls.len() + 1
        && knots.windows(2).enumerate().all(|(index, window)| {
            index == 0 || ((window[1] - window[0]) - (knots[1] - knots[0])).abs() < 1e-9
        })
        && (knots[1] - knots[0]).abs() >= 1e-9;
    if uniform {
        builder.append_periodic(
            &state.controls,
            state.degree,
            knots[0],
            knots[1] - knots[0],
            map,
        );
    } else {
        builder.append_curve(&state.controls, state.degree, Some(knots), map);
    }
}

fn row_num(row: &SectionRow, name: &str) -> Option<f64> {
    let cell = row.cells.get(name).or_else(|| {
        row.cells
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, cell)| cell)
    })?;
    cell.value
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn cell_text(row: &SectionRow, name: &str) -> Option<String> {
    let cell: &Cell = row.cells.get(name).or_else(|| {
        row.cells
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, cell)| cell)
    })?;
    if let Some(formula) = cell.formula.as_deref() {
        if formula != "Inh" && formula != "No Formula" {
            return Some(formula.to_string());
        }
    }
    cell.value.clone()
}

fn clamped_knots(n: usize, degree: usize) -> Vec<f64> {
    let p = degree.clamp(1, n.saturating_sub(1));
    let mut knots = Vec::with_capacity(n + p + 1);
    for index in 0..n + p + 1 {
        if index <= p {
            knots.push(0.0);
        } else if index >= n {
            knots.push(1.0);
        } else {
            knots.push((index - p) as f64 / (n - p) as f64);
        }
    }
    knots
}

fn nurbs_point(controls: &[Control], degree: usize, knots: Option<&[f64]>, t: f64) -> Point {
    let n = controls.len();
    let p = degree.clamp(1, n.saturating_sub(1));
    let owned;
    let kv = match knots {
        Some(knots) if knots.len() == n + p + 1 => knots,
        _ => {
            owned = clamped_knots(n, p);
            &owned
        }
    };
    let mut k = p;
    if t >= kv[n] {
        k = n - 1;
    } else {
        while k < n - 1 && t >= kv[k + 1] {
            k += 1;
        }
    }
    let mut d: Vec<Control> = (0..=p)
        .map(|j| {
            let control = controls[j + k - p];
            let w = if control.w != 0.0 { control.w } else { 1.0 };
            Control {
                x: control.x * w,
                y: control.y * w,
                w,
            }
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let lo = kv[i];
            let hi = kv[i + p - r + 1];
            let den = hi - lo;
            let a = if den > 0.0 { (t - lo) / den } else { 0.0 };
            d[j] = Control {
                x: (1.0 - a) * d[j - 1].x + a * d[j].x,
                y: (1.0 - a) * d[j - 1].y + a * d[j].y,
                w: (1.0 - a) * d[j - 1].w + a * d[j].w,
            };
        }
    }
    let result = d[p];
    let w = if result.w == 0.0 { 1.0 } else { result.w };
    Point {
        x: result.x / w,
        y: result.y / w,
    }
}

fn sample_spline(
    controls: &[Control],
    degree: usize,
    knots: Option<&[f64]>,
    per_span: usize,
) -> Vec<Point> {
    if controls.is_empty() {
        return Vec::new();
    }
    if controls.len() == 1 {
        return vec![Point {
            x: controls[0].x,
            y: controls[0].y,
        }];
    }
    let n = controls.len();
    let p = degree.clamp(1, n.saturating_sub(1));
    let owned;
    let kv = match knots {
        Some(knots) if knots.len() == n + p + 1 => knots,
        _ => {
            owned = clamped_knots(n, p);
            &owned
        }
    };
    let mut out = Vec::new();
    for span in 0..n - p {
        let a = kv[p + span];
        let b = kv[p + span + 1];
        if b <= a {
            continue;
        }
        for index in (if span == 0 { 0 } else { 1 })..=per_span {
            out.push(nurbs_point(
                controls,
                p,
                Some(kv),
                a + (b - a) * index as f64 / per_span as f64,
            ));
        }
    }
    if out.is_empty() {
        out.push(nurbs_point(controls, p, Some(kv), kv[n]));
    }
    out
}

fn circular_arc_points(start: Point, end: Point, bow: f64) -> Option<Vec<Point>> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let chord = dx.hypot(dy);
    if chord <= 1e-9 || !bow.is_finite() {
        return None;
    }
    if bow.abs() < 1e-9 {
        return Some(vec![start, end]);
    }
    let mx = (start.x + end.x) / 2.0;
    let my = (start.y + end.y) / 2.0;
    let nx = dy / chord;
    let ny = -dx / chord;
    let mid = Point {
        x: mx + nx * bow,
        y: my + ny * bow,
    };
    let h = (bow * bow - chord * chord / 4.0) / (2.0 * bow);
    let cx = mx + nx * h;
    let cy = my + ny * h;
    let radius = (chord / 2.0).hypot(h);
    if radius <= 1e-9 {
        return None;
    }
    let ps = (start.y - cy).atan2(start.x - cx);
    let pe = (end.y - cy).atan2(end.x - cx);
    let pm = (mid.y - cy).atan2(mid.x - cx);
    let tau = PI * 2.0;
    let norm = |value: f64| ((value % tau) + tau) % tau;
    let ccw_span = norm(pe - ps);
    let ccw_mid = norm(pm - ps);
    let cw_span = norm(ps - pe);
    let cw_mid = norm(ps - pm);
    let ccw = if ccw_mid <= ccw_span + 1e-6 && cw_mid > cw_span + 1e-6 {
        true
    } else if cw_mid <= cw_span + 1e-6 && ccw_mid > ccw_span + 1e-6 {
        false
    } else {
        ccw_span <= cw_span
    };
    let span = if ccw { ccw_span } else { -cw_span };
    let count = 32usize;
    let mut points = Vec::with_capacity(count + 1);
    points.push(start);
    for index in 1..count {
        let phi = ps + span * index as f64 / count as f64;
        points.push(Point {
            x: cx + radius * phi.cos(),
            y: cy + radius * phi.sin(),
        });
    }
    points.push(end);
    Some(points)
}

fn elliptical_arc_points(
    start: Point,
    end: Point,
    mid: Point,
    theta: f64,
    ratio: f64,
) -> Option<Vec<Point>> {
    let d = if ratio.is_finite() && ratio > 0.0 && ratio <= 1000.0 {
        ratio
    } else {
        1.0
    };
    let k = d * d;
    let cos = theta.cos();
    let sin = theta.sin();
    let u = |point: Point| point.x * cos + point.y * sin;
    let v = |point: Point| -point.x * sin + point.y * cos;
    let us = u(start);
    let vs = v(start);
    let ue = u(end);
    let ve = v(end);
    let um = u(mid);
    let vm = v(mid);
    let a11 = 2.0 * (us - ue);
    let a12 = 2.0 * k * (vs - ve);
    let b1 = us * us - ue * ue + k * (vs * vs - ve * ve);
    let a21 = 2.0 * (ue - um);
    let a22 = 2.0 * k * (ve - vm);
    let b2 = ue * ue - um * um + k * (ve * ve - vm * vm);
    let det = a11 * a22 - a12 * a21;
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let cu = (b1 * a22 - a12 * b2) / det;
    let cv = (a11 * b2 - b1 * a21) / det;
    let a2 = (us - cu).powi(2) + k * (vs - cv).powi(2);
    if a2 <= 1e-12 {
        return None;
    }
    let a = a2.sqrt();
    let b = a / d;
    let phi_of = |u_value: f64, v_value: f64| ((v_value - cv) / b).atan2((u_value - cu) / a);
    let ps = phi_of(us, vs);
    let pe = phi_of(ue, ve);
    let pm = phi_of(um, vm);
    let tau = PI * 2.0;
    let norm = |value: f64| ((value % tau) + tau) % tau;
    let ccw_span = norm(pe - ps);
    let ccw_mid = norm(pm - ps);
    let cw_span = norm(ps - pe);
    let cw_mid = norm(ps - pm);
    let ccw = if ccw_mid <= ccw_span + 1e-6 && cw_mid > cw_span + 1e-6 {
        true
    } else if cw_mid <= cw_span + 1e-6 && ccw_mid > ccw_span + 1e-6 {
        false
    } else {
        ccw_span <= cw_span
    };
    let span = if ccw { ccw_span } else { -cw_span };
    if span.abs() < 1e-9 {
        return None;
    }
    let count = 32usize;
    let mut points = Vec::with_capacity(count + 1);
    points.push(start);
    for index in 1..count {
        let phi = ps + span * index as f64 / count as f64;
        let u_value = cu + a * phi.cos();
        let v_value = cv + b * phi.sin();
        points.push(Point {
            x: u_value * cos - v_value * sin,
            y: u_value * sin + v_value * cos,
        });
    }
    points.push(end);
    Some(points)
}

fn parse_polyline_formula(text: Option<&str>) -> Option<PolylineParsed> {
    let text = text?;
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    let inner = &text[start + 1..end];
    let nums: Vec<f64> = inner
        .split(',')
        .map(|item| item.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    // Two type flags and at least one point.
    if nums.len() < 4 {
        return None;
    }
    let mut points = Vec::new();
    let mut index = 2;
    while index + 1 < nums.len() {
        points.push(Point {
            x: nums[index],
            y: nums[index + 1],
        });
        index += 2;
    }
    Some(PolylineParsed {
        x_type: nums[0] != 0.0,
        y_type: nums[1] != 0.0,
        points,
    })
}

struct PolylineParsed {
    x_type: bool,
    y_type: bool,
    points: Vec<Point>,
}

fn parse_nurbs_formula(text: Option<&str>) -> Option<NurbsParsed> {
    let text = text?;
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    let inner = &text[start + 1..end];
    let nums: Vec<f64> = inner
        .split(',')
        .map(|item| item.trim().parse::<f64>().unwrap_or(0.0))
        .collect();
    if nums.len() < 8 {
        return None;
    }
    let mut points = Vec::new();
    let mut index = 4;
    while index + 3 < nums.len() {
        points.push(NurbsPoint {
            x: nums[index],
            y: nums[index + 1],
            knot: nums[index + 2],
            weight: nums[index + 3],
        });
        index += 4;
    }
    Some(NurbsParsed {
        knot_last: nums[0],
        degree: nums[1],
        x_type: nums[2] != 0.0,
        y_type: nums[3] != 0.0,
        points,
    })
}

#[allow(clippy::too_many_arguments)]
fn nurbs_curve(
    parsed: Option<&NurbsParsed>,
    start: Point,
    end: Point,
    first_knot: f64,
    second_last_knot: f64,
    first_weight: f64,
    last_weight: f64,
    w: f64,
    h: f64,
) -> NurbsCurve {
    let weight = |value: f64| if value != 0.0 { value } else { 1.0 };
    let mut controls = vec![Control {
        x: start.x,
        y: start.y,
        w: weight(first_weight),
    }];
    if let Some(parsed) = parsed {
        for point in &parsed.points {
            controls.push(Control {
                x: if parsed.x_type { point.x } else { point.x * w },
                y: if parsed.y_type { point.y } else { point.y * h },
                w: weight(point.weight),
            });
        }
    }
    controls.push(Control {
        x: end.x,
        y: end.y,
        w: weight(last_weight),
    });
    let n = controls.len();
    let degree = ((parsed
        .map(|parsed| parsed.degree.round())
        .unwrap_or(3.0)
        .max(1.0)) as usize)
        .min(n.saturating_sub(1));
    let degree = degree.max(1);
    let Some(parsed) = parsed else {
        return NurbsCurve {
            controls,
            degree,
            knots: None,
        };
    };
    if controls.len() <= 2 || !parsed.knot_last.is_finite() {
        return NurbsCurve {
            controls,
            degree,
            knots: None,
        };
    }
    let interior_count = n.saturating_sub(degree + 1);
    let mut sequence: Vec<f64> = parsed.points.iter().map(|point| point.knot).collect();
    sequence.push(second_last_knot);
    let interior = if sequence.len() > interior_count {
        sequence[sequence.len() - interior_count..].to_vec()
    } else {
        sequence
    };
    let first = if first_knot.is_finite() {
        first_knot
    } else {
        0.0
    };
    let mut knots = vec![first; degree + 1];
    knots.extend(interior);
    knots.extend(std::iter::repeat_n(parsed.knot_last, degree + 1));
    if knots.windows(2).any(|window| window[1] < window[0]) {
        return NurbsCurve {
            controls,
            degree,
            knots: None,
        };
    }
    if knots.last().copied().unwrap_or(0.0) <= knots[0] {
        return NurbsCurve {
            controls,
            degree,
            knots: None,
        };
    }
    NurbsCurve {
        controls,
        degree,
        knots: Some(knots),
    }
}

fn num_path(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 {
        return "0".to_string();
    }
    if rounded.fract() == 0.0 {
        return format!("{}", rounded as i64);
    }
    let mut text = format!("{rounded:.2}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}


/// Mirror a path (absolute `M L C Q A Z` commands, in a box of `w_px` x `h_px`) about the box
/// centre: `FlipX` turns `x` into `w - x`, `FlipY` turns `y` into `h - y`. An arc keeps its radii
/// and takes the opposite sweep when exactly one axis is mirrored.
pub(crate) fn flip_path(d: &str, w_px: f64, h_px: f64, flip_x: bool, flip_y: bool) -> String {
    if !flip_x && !flip_y {
        return d.to_string();
    }
    let mut out = String::with_capacity(d.len() + 16);
    let mut command = ' ';
    let mut args: Vec<f64> = Vec::new();
    let mut number = String::new();
    let mirrored_once = flip_x ^ flip_y;
    let flush = |command: char, args: &mut Vec<f64>, out: &mut String| {
        if command == ' ' {
            return;
        }
        out.push(command);
        for (index, value) in args.iter().enumerate() {
            let shifted = if command == 'A' {
                match index % 7 {
                    4 if mirrored_once => 1.0 - value,
                    5 if flip_x => w_px - value,
                    6 if flip_y => h_px - value,
                    _ => *value,
                }
            } else if index % 2 == 0 {
                if flip_x { w_px - value } else { *value }
            } else if flip_y {
                h_px - value
            } else {
                *value
            };
            if index > 0 {
                out.push(' ');
            }
            out.push_str(&num_path(shifted));
        }
        args.clear();
    };
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() {
            if !number.is_empty() {
                args.push(number.parse().unwrap_or(0.0));
                number.clear();
            }
            flush(command, &mut args, &mut out);
            command = ch;
            if ch == 'Z' {
                out.push('Z');
                command = ' ';
            }
        } else if ch == ' ' || ch == ',' {
            if !number.is_empty() {
                args.push(number.parse().unwrap_or(0.0));
                number.clear();
            }
        } else if ch == '-' && !number.is_empty() && !number.ends_with('e') {
            args.push(number.parse().unwrap_or(0.0));
            number.clear();
            number.push(ch);
        } else {
            number.push(ch);
        }
    }
    if !number.is_empty() {
        args.push(number.parse().unwrap_or(0.0));
    }
    flush(command, &mut args, &mut out);
    out
}


/// Whether any subpath of `d` is left open: it has no `Z` and its last point is not its first.
/// Visio fills such a path (as if closed) but strokes it open, without the closing edge.
pub(crate) fn has_open_subpath(d: &str) -> bool {
    let mut subpaths: Vec<&str> = d.split('M').filter(|part| !part.trim().is_empty()).collect();
    if subpaths.is_empty() {
        return false;
    }
    subpaths.retain(|part| !part.trim_end().ends_with('Z'));
    subpaths.iter().any(|part| {
        let numbers: Vec<f64> = part
            .split(|ch: char| ch.is_ascii_alphabetic() || ch == ' ' || ch == ',')
            .filter(|token| !token.is_empty())
            .flat_map(|token| {
                // A minus sign starts a new number when it directly follows digits.
                let mut out = Vec::new();
                let mut current = String::new();
                for ch in token.chars() {
                    if ch == '-' && !current.is_empty() && !current.ends_with('e') {
                        out.push(std::mem::take(&mut current));
                    }
                    current.push(ch);
                }
                out.push(current);
                out
            })
            .filter_map(|token| token.parse::<f64>().ok())
            .collect();
        if numbers.len() < 4 {
            return false;
        }
        let (first, last) = ((numbers[0], numbers[1]), (numbers[numbers.len() - 2], numbers[numbers.len() - 1]));
        (first.0 - last.0).abs() > 0.01 || (first.1 - last.1).abs() > 0.01
    })
}
