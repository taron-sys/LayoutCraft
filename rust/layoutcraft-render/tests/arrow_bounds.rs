use layoutcraft_model::Document;
use layoutcraft_render::{scene_from_document, to_svg};

/// (top, bottom) of the picture of a page, from the SVG's viewBox.
fn picture_height(page: serde_json::Value) -> (f64, f64) {
    let document: Document = serde_json::from_value(serde_json::json!({ "pages": [page] })).unwrap();
    let svg = to_svg(&scene_from_document(&document, 0).unwrap());
    let view = svg.split("viewBox=\"").nth(1).and_then(|rest| rest.split('"').next()).expect("a viewBox");
    let numbers: Vec<f64> = view.split_whitespace().map(|number| number.parse().unwrap()).collect();
    (numbers[1], numbers[1] + numbers[3])
}

/// A level line at the top edge of the content, as a shape of its own (`turn` degrees).
fn line_shape(arrow: Option<u32>, turn: f64) -> serde_json::Value {
    let mut part = serde_json::json!({ "d": "M0 0L400 0", "fill": "none", "stroke": "#000000", "strokeWidth": 1.333 });
    if let Some(kind) = arrow {
        part["endArrow"] = serde_json::json!({ "kind": kind, "size": 2 });
    }
    serde_json::json!({ "name": "p", "connectors": [], "shapes": [{
        "id": "1", "type": "rect", "x": 100, "y": 100, "w": 400, "h": 0, "rot": turn, "stroke": "#000000", "strokeWidth": 1.333, "parts": [part]
    }] })
}

/// An arrowhead that is the outermost ink is inside the picture: it reaches about 4.5 px beside a 1 pt line, much further
/// than the line's half width.
#[test]
fn an_arrowhead_of_a_line_shape_is_inside_the_picture() {
    // 4 is drawn by its own rule, 13 from the gallery, 10 is a circle, 42 a dot
    for kind in [4, 13, 10, 42] {
        for turn in [0.0, 180.0] {
            let (bare_top, bare_bottom) = picture_height(line_shape(None, turn));
            let (top, bottom) = picture_height(line_shape(Some(kind), turn));
            assert!(top < bare_top - 2.0 && bottom > bare_bottom + 2.0, "kind {kind}, turned {turn}: {top}..{bottom} against {bare_top}..{bare_bottom}");
        }
    }
}

#[test]
fn an_arrowhead_of_a_connector_is_inside_the_picture() {
    let page = |arrow: &str| {
        serde_json::json!({ "name": "p", "shapes": [], "connectors": [{
            "id": "c", "from": { "point": { "x": 100, "y": 100 } }, "to": { "point": { "x": 500, "y": 100 } },
            "stroke": "#000000", "strokeWidth": 1.333, "arrowEnd": arrow
        }] })
    };
    let (bare_top, bare_bottom) = picture_height(page("none"));
    let (top, bottom) = picture_height(page("triangle"));
    assert!(top < bare_top - 1.0 && bottom > bare_bottom + 1.0, "{top}..{bottom} against {bare_top}..{bare_bottom}");
}
