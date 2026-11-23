#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_vsdx::{import_document, Package};

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test/corpus")
        .join(name)
}

#[test]
fn line_gradient_and_outer_shadow_import() {
    let package = Package::open(corpus("line-gradient-shadow-2.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let shape = &document.pages[0].shapes[0];

    let gradient = shape
        .parts
        .iter()
        .find_map(|part| part.stroke_gradient.as_ref())
        .expect("line gradient should be attached to the stroke part");
    let stops = gradient["stops"].as_array().unwrap();
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0]["color"], "#ff0000");
    assert_eq!(stops[1]["color"], "#0000ff");

    let shadow = shape.shadow.as_ref().expect("outer shadow should import");
    assert!(shadow.dx > 0.0);
    assert!(shadow.dy > 0.0, "Visio's negative Y offset means down");
    assert!(shadow.blur > 0.0);
    assert_eq!(shadow.color, "#5a5a5a");
}

#[test]
fn quick_style_fill_and_line_matrices_resolve_theme_styles() {
    let package = Package::open(corpus("quickstyle-matrices.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[1];
    let fills: Vec<&str> = page
        .shapes
        .iter()
        .take(6)
        .map(|shape| shape.fill.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        fills,
        vec!["#ffffff", "#e9ece5", "#a5b592", "#a5b592", "#919f80", "#919f80"]
    );
    // Line matrix 2 and 3 select the theme's white line styles in this theme.
    let line2 = page
        .shapes
        .iter()
        .find(|shape| shape.label == "Line 2")
        .unwrap();
    assert_eq!(line2.stroke.as_deref(), Some("#ffffff"));
    // Quick-style effect matrix 4/6 resolve an outer shadow, 1/2/3/5 do not.
    let effects: Vec<bool> = page
        .shapes
        .iter()
        .filter(|shape| shape.label.starts_with("Effects "))
        .map(|shape| shape.shadow.is_some())
        .collect();
    assert_eq!(effects, vec![false, false, false, true, false, true]);
}

#[test]
fn variant_quick_style_matrices_follow_the_variation_scheme() {
    let package = Package::open(corpus("quickstyle-variants.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[1];
    for shape in page.shapes.iter().take(4) {
        assert_eq!(shape.fill.as_deref(), Some("#a5b592"));
    }
}

#[test]
fn explicit_glow_soft_edge_and_reflection_import() {
    let package = Package::open(corpus("effects-cells.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[1];
    let by_label = |label: &str| {
        page.shapes
            .iter()
            .find(|shape| shape.label == label)
            .unwrap()
    };
    let glow = by_label("Glow");
    let glow_effect = glow.glow.as_ref().expect("glow");
    assert!((glow_effect.radius - 8.0).abs() < 0.2);
    assert_eq!(glow_effect.color, "#ff0000");
    assert!((glow_effect.opacity - 0.7).abs() < 1e-6);
    assert!(by_label("Soft edge").soft_edge.is_some());
    let reflection = by_label("Reflection")
        .reflection
        .as_ref()
        .expect("reflection");
    assert!((reflection.size - 0.5).abs() < 1e-6);
    assert!(reflection.blur > 2.0);
    assert!((reflection.opacity - 0.7).abs() < 1e-6);
    assert!(by_label("Shadow").shadow.is_some());
}

#[test]
fn fill_patterns_oblique_shadows_and_bevels_import() {
    let package = Package::open(corpus("fill-patterns.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[1];
    let pattern = page.shapes[0].parts[0]
        .fill_pattern
        .as_ref()
        .expect("fill pattern");
    assert_eq!(pattern.kind, 2);
    assert_eq!(pattern.fore, "#4472c4");
    assert_eq!(pattern.back, "#ffffff");
    assert_eq!(
        page.shapes[18].parts[0].fill_pattern.as_ref().unwrap().kind,
        20
    );

    let package = Package::open(corpus("shadow-types.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[1];
    let oblique = page
        .shapes
        .iter()
        .find(|shape| shape.label == "Oblique")
        .and_then(|shape| shape.shadow.as_ref())
        .expect("oblique shadow");
    assert_eq!(oblique.kind, 2);
    assert!((oblique.scale - 2.0).abs() < 1e-6);
    assert!(oblique.oblique > 0.0);

    let package = Package::open(corpus("bevel-types.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[1];
    let bevel = page.shapes[0].bevel.as_ref().expect("bevel");
    assert_eq!(bevel.kind, 1);
    assert!((bevel.width - 9.6).abs() < 0.2);
    assert!((bevel.height - 4.8).abs() < 0.2);
}
