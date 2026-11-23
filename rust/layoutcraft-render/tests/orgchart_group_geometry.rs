#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `orgch-masters.vsdx` (Visio, Org Chart Shapes stencil dropped on pages): the "Executive" box is a
/// group whose frame is the group's own geometry while all four children are zero-sized until they
/// have text. The frame must be drawn.
#[test]
fn org_chart_box_frame_is_drawn_from_the_group_geometry() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/orgch-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 0).unwrap());
    assert!(svg.contains("<path"), "no frame drawn: {}", &svg[..svg.len().min(600)]);
}
