#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, SceneItem};
use layoutcraft_vsdx::{import_document, Package};

/// `layers-guides.vsdx` is Visio-authored: five layers (one hidden, one
/// locked, one non-printing), shapes on one/two/no layers and three guides.
/// Visio's `Page.Export` draws visible, visible+hidden, locked and no-layer
/// shapes and omits the shape only on the hidden layer.
fn document() -> layoutcraft_model::Document {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/layers-guides.vsdx");
    import_document(&Package::open(source).unwrap()).unwrap()
}

#[test]
fn imports_layers_membership_and_guides() {
    let doc = document();
    let page = &doc.pages[0];
    let names: Vec<&str> = page.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        ["Visible layer", "Hidden layer", "Both layers", "Locked layer", "No print layer"]
    );
    assert!(!page.layers[1].visible);
    assert!(page.layers[3].locked);
    assert!(!page.layers[4].print);
    assert_eq!(page.guides.len(), 3);
    // Guides are page furniture, not shapes.
    assert!(page.shapes.iter().all(|s| s.shape_type != "Guide"));
    assert_eq!(page.shapes.len(), 6);

    let by_label = |label: &str| page.shapes.iter().find(|s| s.label == label).unwrap();
    assert_eq!(by_label("visible+hidden").layers, vec![0, 1]);
    assert!(by_label("no layer").layers.is_empty());
    assert!(by_label("hidden").layer_hidden);
    for label in ["visible", "visible+hidden", "locked", "no print", "no layer"] {
        assert!(!by_label(label).layer_hidden, "{label} must stay drawn");
    }
}

#[test]
fn hidden_layer_shapes_are_not_rendered() {
    let doc = document();
    let scene = scene_from_document(&doc, 0).unwrap();
    let drawn = scene
        .items
        .iter()
        .filter(|item| matches!(item, SceneItem::Shape(_)))
        .count();
    // Six shapes on the page, one only on the hidden layer.
    assert_eq!(drawn, 5);
}

/// Visio's `Page.Export` of this page (checked with COM) draws "visible",
/// "visible+hidden", "locked" and "no layer" and omits both "hidden" and
/// "no print". The editor (screen mode) still draws the non-printing shape.
#[test]
fn print_mode_leaves_out_non_printing_layers_but_screen_mode_keeps_them() {
    use std::collections::BTreeMap;
    use layoutcraft_render::{scene_from_document_with_options, SceneOptions};
    let doc = document();
    let page = &doc.pages[0];
    let flagged: Vec<&str> = page
        .shapes
        .iter()
        .filter(|s| s.layer_nonprinting)
        .map(|s| s.label.as_str())
        .collect();
    assert_eq!(flagged, ["no print"]);
    let count = |print: bool| {
        scene_from_document_with_options(&doc, 0, &BTreeMap::new(), SceneOptions { print, ..Default::default() })
            .unwrap()
            .items
            .iter()
            .filter(|item| matches!(item, SceneItem::Shape(_)))
            .count()
    };
    assert_eq!(count(false), 5, "screen: everything except the hidden layer");
    assert_eq!(count(true), 4, "print: also without the non-printing layer");
}
