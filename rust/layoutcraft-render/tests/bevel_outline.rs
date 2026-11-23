#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `bevel-shapes.vsdx` (visio-test-harness/probe-bevel-shapes.ps1: oval, rounded rectangle, triangle and diamond with bevel types 1, 2 and 3,
/// 0.1 x 0.1 in): the bevel of a shape that is not a plain rectangle follows its outline (it used to be a frame around the bounding box, and a
/// rounded corner was lost to the square ring).
#[test]
fn bevels_of_non_rectangular_shapes_follow_the_outline() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/bevel-shapes.vsdx");
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&document, 0).unwrap());
    // twelve shapes: every one has its bevel bands laid along the outline, none the old bounding-box ring
    assert!(svg.matches("bevelOc").count() >= 24, "outline bevel clips: {}", svg.matches("bevelOc").count());
    assert!(!svg.contains("id=\"bevelGrad"), "the old bounding-box ring is still used");
    // an ellipse is cut into many short bands, a triangle into three
    let bands = |shape: usize| svg.matches(&format!("id=\"bevelO{shape}e")).count();
    let counts: Vec<usize> = (0..12).map(bands).collect();
    assert!(counts.iter().filter(|c| **c >= 48).count() >= 3, "ellipses: {counts:?}");
    assert!(counts.iter().filter(|c| **c == 3).count() >= 3, "triangles: {counts:?}");
    // the rounded rectangles keep their corners: the bevel clip is a path with arcs, not a box
    assert!(svg.contains("bevelOc") && svg.matches("A25.2 25.2 0 0 1").count() + svg.matches("A24 24 0 0 1").count() >= 4, "rounded corners missing");
}
