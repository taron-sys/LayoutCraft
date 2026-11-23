//! draw.io edge fidelity: a Rust port of `web/drawio-edges.js` (docs/34, step N4.8b): pure helpers for the draw.io importer. Arrowheads use Visio's own numbering.
use crate::dxf::hypot;

/// The Visio arrowhead number of a draw.io arrow name (`filled` = endFill / startFill is not 0): `Ok(Some(0))` for none, `Ok(None)`-like `Err(())` for an unknown name,
/// and `Ok(None)` when no name is given at all.
pub fn arrow_index(name: Option<&str>, filled: bool) -> Result<Option<i64>, ()> {
    let Some(name) = name else { return Ok(None) };
    if name == "none" || name.is_empty() {
        return Ok(Some(0));
    }
    let hollow = match name {
        "block" | "blockThin" => Some(14),
        "classic" | "classicThin" => Some(19),
        "oval" => Some(20),
        "diamond" | "diamondThin" => Some(22),
        "box" => Some(21),
        "halfCircle" => Some(20),
        _ => None,
    };
    let full = match name {
        "block" | "blockThin" | "doubleBlock" => Some(4),
        "classic" | "classicThin" => Some(5),
        "open" | "openThin" => Some(1),
        "oval" | "halfCircle" => Some(10),
        "diamond" | "diamondThin" => Some(22),
        "box" => Some(11),
        "dash" | "ERone" => Some(24),
        "cross" => Some(23),
        "ERmandOne" => Some(25),
        "ERmany" => Some(27),
        "ERoneToMany" => Some(28),
        "ERzeroToOne" => Some(31),
        "ERzeroToMany" => Some(29),
        _ => None,
    };
    let found = if filled { full } else { hollow.or(full) };
    found.map(Some).ok_or(())
}

/// draw.io's arrow size in px (default 6) as Visio's size index 0..6 (2 = medium).
pub fn arrow_size(px: f64) -> i64 {
    if !px.is_finite() {
        return 2;
    }
    if px <= 4.0 {
        1
    } else if px <= 8.0 {
        2
    } else if px <= 11.0 {
        3
    } else if px <= 15.0 {
        4
    } else if px <= 20.0 {
        5
    } else {
        6
    }
}

/// The connection point (1 top, 2 right, 3 bottom, 4 left) nearest to a position given as fractions (y down).
pub fn side_of(fx: f64, fy: f64) -> usize {
    let (dx, dy) = (fx - 0.5, fy - 0.5);
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 { 2 } else { 4 }
    } else if dy >= 0.0 {
        3
    } else {
        1
    }
}

/// Make a polyline axis-parallel: between two points that differ in both x and y a corner is added, alternating the direction.
pub fn orthogonalize(points: &[[f64; 2]], first_horizontal: Option<bool>) -> Vec<[f64; 2]> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    let mut horiz = first_horizontal.unwrap_or_else(|| (points[1][0] - points[0][0]).abs() >= (points[1][1] - points[0][1]).abs());
    for b in &points[1..] {
        let a = out[out.len() - 1];
        if (a[0] - b[0]).abs() < 1e-6 || (a[1] - b[1]).abs() < 1e-6 {
            out.push(*b);
            horiz = (a[1] - b[1]).abs() < 1e-6;
            continue;
        }
        out.push(if horiz { [b[0], a[1]] } else { [a[0], b[1]] });
        out.push(*b);
    }
    out
}

/// The leg direction an exit side gives: true horizontal, false vertical, None unknown.
pub fn exit_horizontal(fx: Option<f64>, fy: Option<f64>) -> Option<bool> {
    let (fx, fy) = (fx?, fy?);
    Some((fx - 0.5).abs() >= (fy - 0.5).abs())
}

/// Points through a curve: the polyline `pts` as at least four points on a smooth Catmull-Rom curve through them.
pub fn curve_through(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    if pts.len() == 2 {
        let (a, b) = (pts[0], pts[1]);
        return vec![a, [a[0] + (b[0] - a[0]) / 3.0, a[1] + (b[1] - a[1]) / 3.0], [a[0] + ((b[0] - a[0]) * 2.0) / 3.0, a[1] + ((b[1] - a[1]) * 2.0) / 3.0], b];
    }
    let mut out = vec![pts[0]];
    let p = |i: i64| pts[i.clamp(0, pts.len() as i64 - 1) as usize];
    for i in 0..pts.len() as i64 - 1 {
        let (p0, p1, p2, p3) = (p(i - 1), p(i), p(i + 1), p(i + 2));
        for t in [1.0 / 3.0, 2.0 / 3.0] {
            let t2 = t * t;
            let t3 = t2 * t;
            let coord = |k: usize| 0.5 * (2.0 * p1[k] + (-p0[k] + p2[k]) * t + (2.0 * p0[k] - 5.0 * p1[k] + 4.0 * p2[k] - p3[k]) * t2 + (-p0[k] + 3.0 * p1[k] - 3.0 * p2[k] + p3[k]) * t3);
            out.push([coord(0), coord(1)]);
        }
        out.push(p2);
    }
    out
}

/// Where the label of an edge goes: the point at `geo_x` (-1..1 -> 0..1) along the path, `geo_y` px to the side, moved by the offset point. y down.
pub fn label_position(pts: &[[f64; 2]], geo_x: f64, geo_y: f64, offset: [f64; 2]) -> [f64; 2] {
    let mut lens = vec![0.0];
    for i in 1..pts.len() {
        let last = lens[i - 1];
        lens.push(last + hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]));
    }
    let total = {
        let t = lens[lens.len() - 1];
        if t == 0.0 { 1.0 } else { t }
    };
    let want = ((geo_x + 1.0) / 2.0).max(0.0).min(1.0) * total;
    let mut k = 1;
    while k < pts.len() - 1 && lens[k] < want {
        k += 1;
    }
    let (a, b) = (pts[k - 1], pts[k]);
    let seg = {
        let s = lens[k] - lens[k - 1];
        if s == 0.0 { 1.0 } else { s }
    };
    let f = (want - lens[k - 1]) / seg;
    let (px, py) = (a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f);
    let (tx, ty) = ((b[0] - a[0]) / seg, (b[1] - a[1]) / seg);
    [px - ty * geo_y + offset[0], py + tx * geo_y + offset[1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_names_become_visio_numbers() {
        assert_eq!(arrow_index(None, true), Ok(None));
        assert_eq!(arrow_index(Some("none"), true), Ok(Some(0)));
        assert_eq!(arrow_index(Some("classic"), true), Ok(Some(5)));
        assert_eq!(arrow_index(Some("classic"), false), Ok(Some(19)));
        assert_eq!(arrow_index(Some("ERmany"), false), Ok(Some(27)));
        assert_eq!(arrow_index(Some("wobble"), true), Err(()));
        assert_eq!(arrow_size(6.0), 2);
        assert_eq!(arrow_size(30.0), 6);
        assert_eq!(arrow_size(f64::NAN), 2);
    }

    #[test]
    fn sides_orthogonal_routes_and_label_places() {
        assert_eq!(side_of(1.0, 0.5), 2);
        assert_eq!(side_of(0.5, 0.0), 1);
        let o = orthogonalize(&[[0.0, 0.0], [10.0, 20.0]], None);
        assert_eq!(o, vec![[0.0, 0.0], [0.0, 20.0], [10.0, 20.0]]);
        assert_eq!(curve_through(&[[0.0, 0.0], [3.0, 3.0]]).len(), 4);
        assert_eq!(label_position(&[[0.0, 0.0], [10.0, 0.0]], 0.0, 0.0, [0.0, 0.0]), [5.0, 0.0]);
        assert_eq!(exit_horizontal(Some(1.0), Some(0.5)), Some(true));
        assert_eq!(exit_horizontal(None, Some(0.5)), None);
    }
}
