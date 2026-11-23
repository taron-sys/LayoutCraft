//! Compares the right-angle router with the routes Visio produced for 384 dynamic connectors
//! glued point to point between two 1 x 0.7 in rectangles (`route-sweep-sites.vsdx`, made by
//! `visio-test-harness/make-route-sweep-sites.ps1`; routes read back into the JSON next to it).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_vsdx::router::{route_cost, route_right_angle, Rect, Site};

/// Connection points of the Rectangle master, relative to the shape centre: 1 bottom, 2 right,
/// 3 top, 4 left (indexes are Visio's 1-based `Connections.X<n>`).
fn site(n: u64, centre: (f64, f64)) -> Site {
    let (offset, out) = match n {
        1 => ((0.0, -0.35), (0, -1)),
        2 => ((0.5, 0.0), (1, 0)),
        3 => ((0.0, 0.35), (0, 1)),
        _ => ((-0.5, 0.0), (-1, 0)),
    };
    Site { point: (centre.0 + offset.0, centre.1 + offset.1), out }
}

fn rect(centre: (f64, f64)) -> Rect {
    Rect { x0: centre.0 - 0.5, y0: centre.1 - 0.35, x1: centre.0 + 0.5, y1: centre.1 + 0.35 }
}

#[test]
fn routes_match_visio_cost_and_mostly_shape() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/route-sweep-sites.json");
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(cases.len(), 384);
    let (mut exact, mut worse) = (0, 0);
    for case in &cases {
        let (ia, ib) = (case["ia"].as_u64().unwrap(), case["ib"].as_u64().unwrap());
        let (dx, dy) = (case["dx"].as_f64().unwrap(), case["dy"].as_f64().unwrap());
        let visio: Vec<(f64, f64)> = case["route"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
            .collect();
        let a = (0.0, 0.0);
        let b = (dx, dy);
        let ours = route_right_angle(site(ia, a), site(ib, b), &[rect(a), rect(b)], a);
        if route_cost(&ours) > route_cost(&visio) + 1e-3 {
            worse += 1;
        }
        let same = ours.len() == visio.len()
            && ours.iter().zip(&visio).all(|(p, q)| (p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3);
        if same {
            exact += 1;
        }
    }
    println!("exact {exact}/384, costlier {worse}");
    assert_eq!(worse, 0, "a route costs more than Visio's");
    assert!(exact >= 310, "only {exact}/384 routes equal Visio's");
}

/// Glue to the whole shape: Visio picks the sites. `route-sweep-shape.json` holds 112 routes
/// between two 1 x 0.7 in rectangles (made by `make-route-sweep-shape.ps1`).
#[test]
fn whole_shape_glue_picks_visio_sides() {
    use layoutcraft_vsdx::router::route_between_shapes;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/route-sweep-shape.json");
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(cases.len(), 112);
    let sites = |c: (f64, f64)| (1..=4).map(|n| site(n, c)).collect::<Vec<_>>();
    let (mut exact, mut costlier) = (0, 0);
    for case in &cases {
        let (dx, dy) = (case["dx"].as_f64().unwrap(), case["dy"].as_f64().unwrap());
        let visio: Vec<(f64, f64)> = case["route"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
            .collect();
        let (a, b) = ((0.0, 0.0), (dx, dy));
        let (_, _, ours) = route_between_shapes(&sites(a), rect(a), &sites(b), rect(b)).expect("a route");
        if route_cost(&ours) > route_cost(&visio) + 1e-3 {
            costlier += 1;
        }
        if ours.len() == visio.len()
            && ours.iter().zip(&visio).all(|(p, q)| (p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3)
        {
            exact += 1;
        }
    }
    println!("whole-shape: exact {exact}/112, costlier {costlier}");
    assert_eq!(costlier, 0);
    assert!(exact >= 100, "only {exact}/112 equal Visio's");
}

/// Held-out check for glue to the whole shape: a second sweep with other box sizes (A 1.6 x 0.5,
/// B 0.8 x 1.0) and other offsets, run after the side-choice thresholds were fixed on the first.
#[test]
fn whole_shape_glue_holds_for_other_sizes() {
    use layoutcraft_vsdx::router::route_between_shapes;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/route-sweep-shape2.json");
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let boxed = |c: (f64, f64), w: f64, h: f64| Rect { x0: c.0 - w / 2.0, y0: c.1 - h / 2.0, x1: c.0 + w / 2.0, y1: c.1 + h / 2.0 };
    let sites = |c: (f64, f64), w: f64, h: f64| {
        vec![
            Site { point: (c.0, c.1 - h / 2.0), out: (0, -1) },
            Site { point: (c.0 + w / 2.0, c.1), out: (1, 0) },
            Site { point: (c.0, c.1 + h / 2.0), out: (0, 1) },
            Site { point: (c.0 - w / 2.0, c.1), out: (-1, 0) },
        ]
    };
    let (mut exact, mut costlier) = (0, 0);
    let mut misses = Vec::new();
    for case in &cases {
        let (dx, dy) = (case["dx"].as_f64().unwrap(), case["dy"].as_f64().unwrap());
        let visio: Vec<(f64, f64)> = case["route"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
            .collect();
        let (a, b) = ((0.0, 0.0), (dx, dy));
        let (_, _, ours) = route_between_shapes(&sites(a, 1.6, 0.5), boxed(a, 1.6, 0.5), &sites(b, 0.8, 1.0), boxed(b, 0.8, 1.0)).expect("a route");
        if route_cost(&ours) > route_cost(&visio) + 1e-3 {
            costlier += 1;
        }
        if ours.len() == visio.len()
            && ours.iter().zip(&visio).all(|(p, q)| (p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3)
        {
            exact += 1;
        } else {
            misses.push((dx, dy));
        }
    }
    println!("held-out: exact {exact}/{}, costlier {costlier}; misses {misses:?}", cases.len());
    // Offsets of 5 in run into the neighbouring sweep cell, where Visio routes around shapes this
    // router does not know about (8 cases); two more are tight-gap jogs.
    assert!(exact >= 105, "only {exact}/{} equal Visio's", cases.len());
    assert!(costlier * 20 <= cases.len(), "{costlier} routes cost more than Visio's");
}
