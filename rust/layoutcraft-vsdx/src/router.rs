//! Right-angle connector routing between two glued connection points.
//!
//! Derived from Visio's own routes (`visio-test-harness/make-route-sweep-sites.ps1`, 384 dynamic
//! connectors glued point to point between two rectangles at many offsets):
//!
//! * every route leaves and enters a site along the site's outward direction;
//! * routes run on the grid made by the sites, their stub points (3.75 mm = 0.1476 in out of the
//!   site) and the shapes' boxes grown by that stub, and never cross a shape box; a turn may come
//!   before the stub point when a grid line (for example the other site's x) lies closer;
//! * Visio's route is always one of the cheapest, where cost is length in inches plus 1.0 per bend
//!   (384/384 in the sweep);
//! * among equally cheap routes the choice is a tie-break (here: smaller sum of |x| over the bends,
//!   then larger sum of |y|, measured from the begin shape's centre), which reproduces Visio's
//!   pick for about 84 % of the sweep (see `tests/route_sweep.rs`).
//!
//! Only two-shape routing between fixed sites is modelled: no obstacles other than the two glued
//! shapes, no line jumps, no auto-chosen sides (`glue to shape`).
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Space kept between a route and a shape, and the stub at each site: 3.75 mm in inches.
pub const STUB: f64 = 3.75 / 25.4;
/// Cost of one bend, in inches of route length.
const BEND_COST: f64 = 1.0;
/// Cost units per inch (integer arithmetic keeps ties exact).
const UNITS: f64 = 1_000_000.0;

/// An axis-aligned shape box in page inches (y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// A glue point and the axis direction in which a route leaves the shape there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    pub point: (f64, f64),
    /// One of (1,0), (-1,0), (0,1), (0,-1).
    pub out: (i8, i8),
}

const DIRS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Coordinates are clamped to this many inches: a hostile file can put a shape anywhere, and the integer grid must not overflow.
const MAX_COORD_IN: f64 = 100_000.0;

fn unit(value: f64) -> i64 {
    // NaN clamps to NaN and rounds to 0 in the cast.
    (value.clamp(-MAX_COORD_IN, MAX_COORD_IN) * UNITS).round() as i64
}

fn dir_index(dir: (i8, i8)) -> usize {
    DIRS.iter().position(|d| *d == dir).unwrap_or(0)
}

/// Route from `begin` to `end` around `obstacles` (normally the two glued shapes' boxes). The
/// result starts at `begin.point`, ends at `end.point` and holds only the corners between them.
/// `origin` is the begin shape's centre (used only to break ties).
pub fn route_right_angle(begin: Site, end: Site, obstacles: &[Rect], origin: (f64, f64)) -> Vec<(f64, f64)> {
    search(begin, end, obstacles, origin)
        .or_else(|| search(begin, end, &[], origin))
        .unwrap_or_else(|| vec![begin.point, end.point])
}

fn search(begin: Site, end: Site, obstacles: &[Rect], origin: (f64, f64)) -> Option<Vec<(f64, f64)>> {
    let boxes: Vec<[i64; 4]> = obstacles
        .iter()
        .map(|r| [unit(r.x0), unit(r.y0), unit(r.x1), unit(r.y1)])
        .collect();
    let stub = |site: &Site| {
        (
            unit(site.point.0 + f64::from(site.out.0) * STUB),
            unit(site.point.1 + f64::from(site.out.1) * STUB),
        )
    };
    let (start_point, goal_point) = ((unit(begin.point.0), unit(begin.point.1)), (unit(end.point.0), unit(end.point.1)));
    let inward = (-end.out.0, -end.out.1);

    // Grid: both sites, their stub points and the boxes grown by the stub.
    let mut xs: Vec<i64> = vec![start_point.0, goal_point.0, stub(&begin).0, stub(&end).0];
    let mut ys: Vec<i64> = vec![start_point.1, goal_point.1, stub(&begin).1, stub(&end).1];
    for b in &boxes {
        let grow = unit(STUB);
        xs.extend([b[0] - grow, b[2] + grow]);
        ys.extend([b[1] - grow, b[3] + grow]);
    }
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();
    let xi = |v: i64| xs.binary_search(&v).unwrap();
    let yi = |v: i64| ys.binary_search(&v).unwrap();

    // A segment is blocked when it passes through the open interior of a shape box.
    let blocked = |a: (i64, i64), b: (i64, i64)| {
        boxes.iter().any(|g| {
            if a.0 == b.0 {
                g[0] < a.0 && a.0 < g[2] && a.1.min(b.1) < g[3] && a.1.max(b.1) > g[1]
            } else {
                g[1] < a.1 && a.1 < g[3] && a.0.min(b.0) < g[2] && a.0.max(b.0) > g[0]
            }
        })
    };

    // State: grid cell and the direction of the last move. Cost: (route cost, sum |x| of bends,
    // minus sum |y| of bends), all additive, compared lexicographically.
    type Cost = (i64, i64, i64);
    let (w, h) = (xs.len(), ys.len());
    let index = |x: usize, y: usize, d: usize| (y * w + x) * 4 + d;
    let mut best: Vec<Option<Cost>> = vec![None; w * h * 4];
    let mut parent: Vec<usize> = vec![usize::MAX; w * h * 4];
    let origin_units = (unit(origin.0), unit(origin.1));
    let start = index(xi(start_point.0), yi(start_point.1), dir_index(begin.out));
    let goal = index(xi(goal_point.0), yi(goal_point.1), dir_index(inward));
    let bend_cost = unit(BEND_COST);
    // Tie-break on |y| of the bends: a jog between two sites facing each other sits near the begin
    // shape, any other detour near the target.
    let y_sign: i64 = if (begin.out.0, begin.out.1) == (-end.out.0, -end.out.1) { 1 } else { -1 };
    let mut heap = BinaryHeap::new();
    best[start] = Some((0, 0, 0));
    heap.push(Reverse(((0i64, 0i64, 0i64), start)));
    while let Some(Reverse((cost, node))) = heap.pop() {
        if best[node] != Some(cost) {
            continue;
        }
        if node == goal {
            break;
        }
        let (d, cell) = (node % 4, node / 4);
        let (x, y) = (cell % w, cell / w);
        let here = (xs[x], ys[y]);
        for (nd, dir) in DIRS.iter().enumerate() {
            if (dir.0, dir.1) == (-DIRS[d].0, -DIRS[d].1) {
                continue;
            }
            // The first move leaves the site along its outward direction.
            if node == start && nd != d {
                continue;
            }
            let next = match *dir {
                (1, 0) => (x + 1 < w).then(|| (x + 1, y)),
                (-1, 0) => (x > 0).then(|| (x - 1, y)),
                (0, 1) => (y + 1 < h).then(|| (x, y + 1)),
                _ => (y > 0).then(|| (x, y - 1)),
            };
            let Some((nx, ny)) = next else { continue };
            let there = (xs[nx], ys[ny]);
            if blocked(here, there) {
                continue;
            }
            let turn = nd != d;
            let length = (there.0 - here.0).abs() + (there.1 - here.1).abs();
            let mut next_cost = (cost.0 + length, cost.1, cost.2);
            if turn {
                next_cost.0 += bend_cost;
                next_cost.1 += (here.0 - origin_units.0).abs();
                next_cost.2 += y_sign * (here.1 - origin_units.1).abs();
            }
            let next_node = index(nx, ny, nd);
            if best[next_node].map_or(true, |b| next_cost < b) {
                best[next_node] = Some(next_cost);
                parent[next_node] = node;
                heap.push(Reverse((next_cost, next_node)));
            }
        }
    }
    best[goal]?;
    let mut node = goal;
    let mut cells = Vec::new();
    while node != usize::MAX {
        let cell = node / 4;
        cells.push((xs[cell % w] as f64 / UNITS, ys[cell / w] as f64 / UNITS));
        node = parent[node];
    }
    cells.reverse();

    // Drop points that are not corners.
    let mut out = vec![cells[0]];
    for i in 1..cells.len() - 1 {
        let (a, b, c) = (out[out.len() - 1], cells[i], cells[i + 1]);
        let same_x = (a.0 - b.0).abs() < 1e-9 && (b.0 - c.0).abs() < 1e-9;
        let same_y = (a.1 - b.1).abs() < 1e-9 && (b.1 - c.1).abs() < 1e-9;
        if !(same_x || same_y) {
            out.push(b);
        }
    }
    out.push(cells[cells.len() - 1]);
    Some(out)
}

/// The cost this module minimises for a route: length plus one inch per bend.
pub fn route_cost(points: &[(f64, f64)]) -> f64 {
    let length: f64 = points.windows(2).map(|w| (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs()).sum();
    length + BEND_COST * points.len().saturating_sub(2) as f64
}

/// Route between two shapes glued as a whole: Visio picks the connection points itself. Every
/// pair of sites with different outward directions is routed (Visio never joins two sites facing
/// the same way) and the cheapest route wins, except when the best is a one-bend L: there are
/// then two, vertical segment first or horizontal first, and Visio does not take the cheaper
/// one. Its choice, fitted on two sweeps (`route-sweep-shape`, 112 cases, boxes 1 x 0.7 and
/// 0.7 x 0.7; `route-sweep-shape2`, 128 cases, 1.6 x 0.5 and 0.8 x 1.0):
///
/// * boxes that overlap in x or y: vertical first only for a target up and to the left;
/// * otherwise vertical first when the boxes nearly share a column (gap under 0.25 in) or the
///   vertical run from the begin box to the target's centre height is at least 0.8 in.
///
/// Columns where a third shape of the sweep got in Visio's way (offset 5 in) do not follow it.
/// (cost, bends, begin site, end site, route)
type Candidate = (f64, usize, Site, Site, Vec<(f64, f64)>);

fn cheapest<'a>(set: &[&'a Candidate]) -> Option<&'a Candidate> {
    set.iter().copied().fold(None, |best: Option<&Candidate>, c| match best {
        Some(b) if b.0 <= c.0 + 1e-9 => Some(b),
        _ => Some(c),
    })
}

pub fn route_between_shapes(
    from: &[Site],
    from_box: Rect,
    to: &[Site],
    to_box: Rect,
) -> Option<(Site, Site, Vec<(f64, f64)>)> {
    let centre = |r: &Rect| ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    let (ca, cb) = (centre(&from_box), centre(&to_box));
    let mut candidates: Vec<Candidate> = Vec::new();
    for a in from {
        for b in to {
            if a.out == b.out {
                continue;
            }
            if let Some(route) = search(*a, *b, &[from_box, to_box], ca) {
                candidates.push((route_cost(&route), route.len().saturating_sub(2), *a, *b, route));
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let all: Vec<&Candidate> = candidates.iter().collect();
    let best = cheapest(&all)?;
    let mut pick = best;
    if best.1 == 1 {
        let vertical: Vec<&Candidate> = candidates.iter().filter(|c| c.1 == 1 && c.2.out.0 == 0).collect();
        let horizontal: Vec<&Candidate> = candidates.iter().filter(|c| c.1 == 1 && c.2.out.0 != 0).collect();
        if !vertical.is_empty() && !horizontal.is_empty() {
            let (dx, dy) = (cb.0 - ca.0, cb.1 - ca.1);
            let hgap = dx.abs() - ((from_box.x1 - from_box.x0) + (to_box.x1 - to_box.x0)) / 2.0;
            let vgap = dy.abs() - ((from_box.y1 - from_box.y0) + (to_box.y1 - to_box.y0)) / 2.0;
            let vertical_first = if vgap <= 0.0 || hgap <= 0.0 {
                dy > 0.0 && dx < 0.0
            } else {
                hgap < 0.25 || dy.abs() - (from_box.y1 - from_box.y0) / 2.0 >= 0.8
            };
            pick = cheapest(if vertical_first { &vertical } else { &horizontal })?;
        }
    }
    Some((pick.2, pick.3, pick.4.clone()))
}
