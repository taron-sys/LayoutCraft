#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

fn svg_of(name: &str) -> String {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus").join(name);
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    to_svg(&scene_from_document(&document, 0).unwrap())
}

/// `shadow-oblique.vsdx` was made in Visio (`probe-oblique.ps1`): eight 1 x 1 in squares with an oblique shadow. Visio's export:
/// scale 2 at 0 degrees is a copy 2 in tall standing on the bottom edge (moved by the offset); 45 degrees tilts the vertical axis
/// (height scale x cos, lean scale x sin); an oblique shadow with offset 0 still shows.
#[test]
fn oblique_shadows_tilt_and_scale_the_vertical_axis_about_the_bottom_edge() {
    let svg = svg_of("shadow-oblique.vsdx");
    // scale 2, angle 0: x' = x, y' = 2 y
    assert!(svg.contains("matrix(1 0 0 2 "), "scale 2 / 0 degrees missing");
    // scale 1, angle 45 degrees: c = -sin, d = cos
    assert!(svg.contains("matrix(1 0 -0.707 0.707 "), "45 degrees missing");
    // eight shapes, eight shadows (the one with offset 0 included): every oblique shadow has a matrix with a 1 0 start
    assert_eq!(svg.matches("transform=\"matrix(1 0 ").count(), 8);
}

/// `shadow-inner.vsdx` (`probe-inner.ps1`): type 3 shadows. Visio paints the part of the shape that its copy moved against the offset
/// does not cover; there is no shadow behind the shape.
#[test]
fn inner_shadows_are_drawn_over_the_shape_with_a_mask() {
    let svg = svg_of("shadow-inner.vsdx");
    assert_eq!(svg.matches("<mask id=\"innerMask").count(), 6, "one mask per shape");
    assert!(svg.contains("innerBlur"), "the blurred case");
}
