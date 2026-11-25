//! One minimal input for each crash the hostile-input sweeps found in this crate.
use layoutcraft_vsdx::router::{route_between_shapes, route_right_angle, Rect, Site};

#[test]
fn a_route_between_shapes_a_million_inches_away_does_not_overflow() {
    let begin = Site { point: (0.0, 0.0), out: (1, 0) };
    let end = Site { point: (1e308, -1e308), out: (-1, 0) };
    let boxes = [Rect { x0: -1.0, y0: -1.0, x1: 0.0, y1: 1.0 }, Rect { x0: 1e308, y0: -1e308, x1: f64::INFINITY, y1: f64::NAN }];
    let route = route_right_angle(begin, end, &boxes, (f64::NAN, 0.0));
    assert!(route.len() >= 2);
    let _ = route_between_shapes(&[begin], boxes[0], &[end], boxes[1]);
}
