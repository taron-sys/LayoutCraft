#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

fn svg_of(name: &str) -> String {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus").join(name);
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    to_svg(&scene_from_document(&document, 0).unwrap())
}

/// `group-displaymode-*.vsdx` (visio-test-harness/make-displaymode.mjs): a group of two red rectangles with its own blue rectangle. Visio's export
/// (probe in step 219): with no `DisplayMode` cell, or 2, the group's own geometry is drawn IN FRONT of the members (the middle and the members
/// are blue); 1 = BEHIND them (blue between, red members); 0 = not drawn (white between, red members).
fn order(svg: &str) -> (Option<usize>, Option<usize>) {
    (svg.find("fill=\"#0000ff\""), svg.rfind("fill=\"#ff0000\""))
}

#[test]
fn a_groups_own_geometry_is_drawn_in_front_of_behind_or_not_by_display_mode() {
    for name in ["group-displaymode-none.vsdx", "group-displaymode-2.vsdx"] {
        let (blue, red) = order(&svg_of(name));
        assert!(blue.is_some() && red.is_some() && blue > red, "{name}: the group's blue rectangle must come after the red members");
    }
    let (blue, red) = order(&svg_of("group-displaymode-1.vsdx"));
    assert!(blue.is_some() && red.is_some() && blue < red, "DisplayMode 1: blue behind the red members");
    let svg = svg_of("group-displaymode-0.vsdx");
    assert!(!svg.contains("fill=\"#0000ff\""), "DisplayMode 0: the group's own geometry is not drawn");
    assert!(svg.contains("fill=\"#ff0000\""));
}
