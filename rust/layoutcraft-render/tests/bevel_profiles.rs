#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `bevel-sizes.vsdx` was made in Visio (`probe-bevel-sizes.ps1`): five pages of twelve 1.4 x 1.1 in rectangles (fill 68,114,196), one
/// bevel type each, with bevel sizes 0.1 x 0.05, 0.2 x 0.05, 0.1 x 0.1, 0.2 x 0.2 and 1/12 x 1/12 in. Visio's export (288 dpi) of the
/// last page lights the interior to 104 % of the fill: (71, 118, 204); every type of that page is drawn from the measured profile.
#[test]
fn bevels_on_rectangles_are_drawn_from_visios_measured_profiles() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/bevel-sizes.vsdx");
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    assert_eq!(document.pages.len(), 5);
    let svg = to_svg(&scene_from_document(&document, 4).unwrap());
    assert_eq!(svg.matches("id=\"bevelP").count(), 48, "four gradients for each of the twelve shapes");
    assert!(svg.contains("fill=\"#4777cc\""), "the interior is lit to 104 % of the fill: {}", &svg[..svg.len().min(400)]);
    assert!(!svg.contains("bevelGrad"), "no shape fell back to the old ring");
}
