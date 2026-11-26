use layoutcraft_model::Document;
use layoutcraft_render::{scene_from_document, to_svg};

/// The SVG of one shape with rounded corners, a line colour and the given geometry in its 100 x 40 box.
fn rounded(path: &str) -> String {
    let document: Document = serde_json::from_value(serde_json::json!({ "pages": [{ "name": "p", "connectors": [], "shapes": [{
        "id": "1", "type": "rect", "x": 10, "y": 10, "w": 100, "h": 40, "rounding": 8, "stroke": "#000000", "strokeWidth": 1,
        "parts": [{ "d": path, "fill": "none", "stroke": "#000000", "strokeWidth": 1 }]
    }] }] }))
    .unwrap();
    to_svg(&scene_from_document(&document, 0).unwrap())
}

/// A rounded box gets its outline closed by a rounded rectangle; a line whose ends sit on the corners of its box (a slanted
/// line, the ticks of a dimension line) must not get one drawn around it.
#[test]
fn only_a_shape_that_encloses_an_area_gets_a_rounded_frame() {
    let frame = "rx=\"8\" fill=\"none\"";
    assert!(rounded("M0 0L100 0L100 40L0 40Z").contains(frame));
    for line in ["M0 40L100 0", "M0 0L0 40L0 0Z"] {
        assert!(!rounded(line).contains(frame), "{line}");
    }
}
