#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{render_png, scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `rtl-text.vsdx` (engine-written, make-rtl.mjs): Hebrew, Arabic and mixed text boxes. Visio (PNG and SVG export of the same file):
/// the `Bidi` cell is not used (the same left alignment with it set), Hebrew and Arabic words run right to left like the Unicode
/// algorithm, and a number that follows Hebrew text stays at the right end ("abc <Hebrew> 123"), which is what an isolated
/// left-to-right run does in the browser.
#[test]
fn numbers_in_right_to_left_lines_are_isolated_and_plain_lines_are_not() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/rtl-text.vsdx");
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    let scene = scene_from_document(&document, 0).unwrap();
    let svg = to_svg(&scene);
    assert!(svg.contains("unicode-bidi:isolate\">123</tspan>"), "the number in the mixed line is isolated");
    assert_eq!(svg.matches("unicode-bidi:isolate").count(), 2, "only the two mixed boxes have a number: {}", svg.matches("unicode-bidi:isolate").count());
    assert!(svg.contains("<tspan>Hello</tspan>"), "a Latin line keeps plain runs");
    // the rasteriser reads the markup
    assert!(render_png(&scene, 1.0).is_ok());
}
