#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, SceneItem};
use layoutcraft_vsdx::{import_document, Package};

/// `layer-colors.vsdx` was made in Visio (`probe-layer-color2.ps1`): five blue boxes with a green outline and black text over yellow
/// boxes, on layers with `Color` 255, 2, 3, 6 and `#ff0000`. Visio's export draws the coloured ones with outline and text in the
/// layer's colour (palette 2 red, 3 green, 6 magenta) and an opaque white fill.
#[test]
fn shapes_on_a_coloured_layer_draw_in_its_colour_with_a_white_fill() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/layer-colors.vsdx");
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    let colours: Vec<Option<&str>> = document.pages[0].layers.iter().map(|l| l.color.as_deref()).collect();
    assert_eq!(colours, [None, Some("#ff0000"), Some("#00ff00"), Some("#ff00ff"), Some("#ff0000")]);
    let scene = scene_from_document(&document, 0).unwrap();
    let boxes: Vec<_> = scene
        .items
        .iter()
        .filter_map(|item| match item {
            SceneItem::Shape(shape) if shape.text.is_some() => Some(shape),
            _ => None,
        })
        .collect();
    assert_eq!(boxes.len(), 5);
    let part = |i: usize| &boxes[i].parts[0];
    assert!(part(0).fill.eq_ignore_ascii_case("#4472c4") || part(0).fill != "#ffffff", "the box without a layer colour keeps its fill");
    for (i, colour) in [(1, "#ff0000"), (2, "#00ff00"), (3, "#ff00ff"), (4, "#ff0000")] {
        assert_eq!(part(i).fill, "#ffffff", "box {i}");
        assert_eq!(part(i).stroke, colour, "box {i}");
        assert_eq!(boxes[i].text.as_ref().unwrap().color, colour, "box {i}");
    }
}
