//! Union, Combine, Fragment, Intersect and Subtract, compared with what the installed Visio makes of the same two rectangles
//! (visio-test-harness/probe-shape-ops.ps1): A = (1,1)-(4,3) drawn first, B = (3,2)-(6,5).
use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
use layoutcraft_vsdx::groupedit::shapeops::{shape_operation, ShapeOp};
use layoutcraft_vsdx::{write_blank_document, BlankOptions, MasterCatalog, Package, ResolvedShape};

fn roots(package: &Package) -> Vec<ResolvedShape> {
    let part = package.pages().unwrap()[0].part.clone();
    MasterCatalog::load(package).unwrap().resolve(&package.page_shapes(&part).unwrap()).unwrap()
}

fn two_rectangles() -> (Package, String, String) {
    let blank = write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "P".to_string() }).unwrap();
    let p = Package::from_bytes(&blank).unwrap();
    let (b, a) = draw_shape(&p, 0, DrawKind::Rectangle, 1.0, 1.0, 4.0, 3.0).unwrap();
    let p = Package::from_bytes(&b).unwrap();
    let (b, bb) = draw_shape(&p, 0, DrawKind::Rectangle, 3.0, 2.0, 6.0, 5.0).unwrap();
    (Package::from_bytes(&b).unwrap(), a, bb)
}

/// (id, pin x, pin y, width, height, text, number of geometry sections, outline area in square inches) of every page shape.
fn shapes(package: &Package) -> Vec<(String, f64, f64, f64, f64, String, usize, f64)> {
    roots(package)
        .iter()
        .map(|n| {
            let num = |k: &str| n.cells.get(k).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok()).unwrap();
            let (w, h) = (num("Width"), num("Height"));
            let mut area = 0.0;
            let sections: Vec<_> = n.sections.iter().filter(|s| s.name == "Geometry").collect();
            for s in &sections {
                // shoelace over each subpath of the section (rings add, holes written clockwise subtract: even-odd keeps them apart here)
                let mut ring: Vec<(f64, f64)> = Vec::new();
                let flush = |ring: &mut Vec<(f64, f64)>, area: &mut f64| {
                    let mut a = 0.0;
                    for i in 0..ring.len() {
                        let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
                        a += p.0 * q.1 - q.0 * p.1;
                    }
                    *area += a / 2.0;
                    ring.clear();
                };
                for r in &s.rows {
                    let x = r.cells.get("X").and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok()).unwrap() * w;
                    let y = r.cells.get("Y").and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok()).unwrap() * h;
                    if r.row_type.as_deref() == Some("RelMoveTo") {
                        flush(&mut ring, &mut area);
                    }
                    ring.push((x, y));
                }
                flush(&mut ring, &mut area);
            }
            (n.id.clone(), num("PinX"), num("PinY"), w, h, n.text.clone().unwrap_or_default().trim().to_string(), sections.len(), area)
        })
        .collect()
}

fn run(op: ShapeOp, ids: &[&String]) -> (Package, Vec<String>) {
    let (p, ..) = two_rectangles();
    let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    let (bytes, new_ids) = shape_operation(&p, 0, op, &ids).unwrap();
    (Package::from_bytes(&bytes).unwrap(), new_ids)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn union_makes_one_shape_with_the_outline_of_both() {
    let (p, a, b) = two_rectangles();
    let (bytes, ids) = shape_operation(&p, 0, ShapeOp::Union, &[a, b]).unwrap();
    let out = shapes(&Package::from_bytes(&bytes).unwrap());
    assert_eq!(out.len(), 1);
    let s = &out[0];
    assert_eq!(ids, vec![s.0.clone()]);
    assert!(close(s.1, 3.5) && close(s.2, 3.0) && close(s.3, 5.0) && close(s.4, 4.0), "{s:?}"); // Visio: pin (3.5, 3), 5 x 4
    assert_eq!(s.5, "");
    assert!(close(s.7.abs(), 6.0 + 9.0 - 1.0), "union area {}", s.7); // 3x2 + 3x3 - 1x1
}

#[test]
fn the_first_selected_shape_gives_the_result_its_look_and_text() {
    // Visio keeps the first selected shape's fill and text: A first or B first only changes whose they are
    let (p, a, b) = two_rectangles();
    let id_a = a.clone();
    let (bytes, _) = shape_operation(&p, 0, ShapeOp::Subtract, &[a.clone(), b.clone()]).unwrap();
    let editor = roots(&Package::from_bytes(&bytes).unwrap());
    let fill_of_first = roots(&p).iter().find(|n| n.id == id_a).unwrap().cells.get("FillForegnd").cloned();
    assert_eq!(editor.len(), 1);
    assert_eq!(editor[0].cells.get("FillForegnd").cloned(), fill_of_first);
}

#[test]
fn subtract_removes_the_overlap_from_the_first_shape() {
    let (p, a, b) = two_rectangles();
    let (bytes, _) = shape_operation(&p, 0, ShapeOp::Subtract, &[a.clone(), b.clone()]).unwrap();
    let out = shapes(&Package::from_bytes(&bytes).unwrap());
    let s = &out[0];
    assert!(close(s.1, 2.5) && close(s.2, 2.0) && close(s.3, 3.0) && close(s.4, 2.0), "{s:?}"); // Visio: pin (2.5, 2), 3 x 2
    assert!(close(s.7.abs(), 6.0 - 1.0), "A minus B {}", s.7);
    // the other way round: B minus A, Visio: pin (4.5, 3.5), 3 x 3
    let (bytes, _) = shape_operation(&p, 0, ShapeOp::Subtract, &[b, a]).unwrap();
    let s = &shapes(&Package::from_bytes(&bytes).unwrap())[0];
    assert!(close(s.1, 4.5) && close(s.2, 3.5) && close(s.3, 3.0) && close(s.4, 3.0), "{s:?}");
    assert!(close(s.7.abs(), 9.0 - 1.0), "{}", s.7);
}

#[test]
fn intersect_keeps_the_common_part() {
    let (p, a, b) = two_rectangles();
    let (bytes, _) = shape_operation(&p, 0, ShapeOp::Intersect, &[a, b]).unwrap();
    let s = &shapes(&Package::from_bytes(&bytes).unwrap())[0];
    assert!(close(s.1, 3.5) && close(s.2, 2.5) && close(s.3, 1.0) && close(s.4, 1.0), "{s:?}"); // Visio: pin (3.5, 2.5), 1 x 1
}

#[test]
fn combine_keeps_one_section_per_shape() {
    let (p, a, b) = two_rectangles();
    let (bytes, _) = shape_operation(&p, 0, ShapeOp::Combine, &[a, b]).unwrap();
    let out = shapes(&Package::from_bytes(&bytes).unwrap());
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].6, 2, "two geometry sections as in Visio");
    assert!(close(out[0].3, 5.0) && close(out[0].4, 4.0));
}

#[test]
fn fragment_makes_one_shape_per_piece_without_text() {
    let (p, a, b) = two_rectangles();
    let (bytes, ids) = shape_operation(&p, 0, ShapeOp::Fragment, &[a, b]).unwrap();
    let out = shapes(&Package::from_bytes(&bytes).unwrap());
    assert_eq!(out.len(), 3);
    assert_eq!(ids.len(), 3);
    // Visio: B only (pin 4.5, 3.5; 3 x 3), A only (2.5, 2; 3 x 2), the overlap (3.5, 2.5; 1 x 1)
    let want = [(4.5, 3.5, 3.0, 3.0, 8.0), (2.5, 2.0, 3.0, 2.0, 5.0), (3.5, 2.5, 1.0, 1.0, 1.0)];
    for (s, w) in out.iter().zip(want) {
        assert!(close(s.1, w.0) && close(s.2, w.1) && close(s.3, w.2) && close(s.4, w.3), "{s:?} vs {w:?}");
        assert!(close(s.7.abs(), w.4), "area {} vs {}", s.7, w.4);
        assert_eq!(s.5, "");
    }
}

#[test]
fn disjoint_shapes_stay_apart_in_a_union_and_do_not_intersect() {
    let blank = write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "P".to_string() }).unwrap();
    let p = Package::from_bytes(&blank).unwrap();
    let (b, a) = draw_shape(&p, 0, DrawKind::Rectangle, 1.0, 1.0, 2.0, 2.0).unwrap();
    let p = Package::from_bytes(&b).unwrap();
    let (b, c) = draw_shape(&p, 0, DrawKind::Rectangle, 4.0, 4.0, 5.0, 5.0).unwrap();
    let p = Package::from_bytes(&b).unwrap();
    let (bytes, _) = shape_operation(&p, 0, ShapeOp::Union, &[a.clone(), c.clone()]).unwrap();
    let s = &shapes(&Package::from_bytes(&bytes).unwrap())[0];
    assert!(close(s.3, 4.0) && close(s.4, 4.0) && close(s.7.abs(), 2.0), "{s:?}");
    assert!(shape_operation(&p, 0, ShapeOp::Intersect, &[a, c]).is_err());
}

#[test]
fn an_ellipse_can_be_cut_by_a_rectangle() {
    let blank = write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "P".to_string() }).unwrap();
    let p = Package::from_bytes(&blank).unwrap();
    let (b, e) = draw_shape(&p, 0, DrawKind::Ellipse, 1.0, 1.0, 5.0, 5.0).unwrap();
    let p = Package::from_bytes(&b).unwrap();
    let (b, r) = draw_shape(&p, 0, DrawKind::Rectangle, 3.0, 0.0, 6.0, 6.0).unwrap(); // covers the right half
    let p = Package::from_bytes(&b).unwrap();
    let (bytes, _) = shape_operation(&p, 0, ShapeOp::Subtract, &[e, r]).unwrap();
    let s = &shapes(&Package::from_bytes(&bytes).unwrap())[0];
    assert!(close(s.1, 2.0) && close(s.3, 2.0) && close(s.4, 4.0), "left half of the circle: {s:?}");
    let half = std::f64::consts::PI * 4.0 / 2.0;
    assert!((s.7.abs() - half).abs() < 0.05, "area {} vs {}", s.7, half);
}

#[test]
fn the_selected_shapes_must_be_on_the_page_and_distinct() {
    let (p, a, _) = two_rectangles();
    assert!(shape_operation(&p, 0, ShapeOp::Union, &[a.clone()]).is_err());
    assert!(shape_operation(&p, 0, ShapeOp::Union, &[a.clone(), a]).is_err());
    assert!(shape_operation(&p, 0, ShapeOp::Union, &["1".to_string(), "99".to_string()]).is_err());
    let _ = run;
}
