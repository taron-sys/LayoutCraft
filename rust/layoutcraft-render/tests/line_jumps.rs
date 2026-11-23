#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `linejump-styles.vsdx` is Visio-authored (`make-linejump-styles.ps1`): a horizontal connector
/// crossing a vertical one, one page per page-level `LineJumpStyle` 0..8. Visio stores the hop in
/// the connector's geometry (`ArcTo`, `PolylineTo`, or a `MoveTo` for the gap style).
fn page_path(page: usize) -> String {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/linejump-styles.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, page).unwrap());
    // The first connector path (the horizontal one, which hops).
    let start = svg.find("<path d=\"M19.2 96").expect("horizontal connector");
    let rest = &svg[start + 9..];
    rest[..rest.find('"').unwrap()].to_string()
}

#[test]
fn arc_hop_is_drawn_as_an_arc() {
    let d = page_path(0);
    assert!(d.contains("A3.15 3.15 0 0 1 113.323 110.173"), "{d}");
}

#[test]
fn gap_hop_lifts_the_pen() {
    let d = page_path(2);
    assert!(d.contains("L107.024 110.173M113.323 110.173"), "{d}");
}

#[test]
fn square_and_polygon_hops_follow_the_stored_points() {
    let square = page_path(3);
    assert!(square.contains("L107.024 107.024L113.323 107.024L113.323 110.173"), "{square}");
    let triangle = page_path(4);
    assert!(triangle.contains("L110.173 107.024L113.323 110.173"), "{triangle}");
    // 2/3/4/5/6-sided polygons: the number of bend points between the hop's ends grows.
    let bends = |d: &str| d.matches('L').count();
    assert!(bends(&page_path(8)) > bends(&page_path(5)));
}
