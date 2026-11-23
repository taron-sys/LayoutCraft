#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_model::Document;
use layoutcraft_render::{render_png, scene_from_document, to_svg, SceneItem};

fn golden_model(name: &str) -> Document {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    Document::open(root.join("test/golden/models").join(name)).unwrap()
}

#[test]
fn scene_builds_shapes_parts_and_rich_text() {
    let document = golden_model("test-corpus-rich-text.json");
    let scene = scene_from_document(&document, 0).unwrap();
    assert_eq!(scene.items.len(), 5);
    let shape = scene
        .items
        .iter()
        .find_map(|item| match item {
            SceneItem::Shape(shape) if shape.visio_id.as_deref() == Some("1") => Some(shape),
            _ => None,
        })
        .expect("shape 1");
    assert_eq!(shape.parts.len(), 1);
    let text = shape.text.as_ref().expect("rich text");
    assert_eq!(text.paragraphs.len(), 1);
    assert_eq!(text.paragraphs[0].runs.len(), 7);
    assert_eq!(text.paragraphs[0].runs[0].text, "Bold red");
    assert_eq!(text.paragraphs[0].runs[0].bold, Some(true));
    assert_eq!(text.paragraphs[0].runs[0].color.as_deref(), Some("#ff0000"));
}

#[test]
fn svg_is_deterministic_and_contains_text_and_geometry() {
    let document = golden_model("test-corpus-rich-text.json");
    let scene = scene_from_document(&document, 0).unwrap();
    let first = to_svg(&scene);
    let second = to_svg(&scene);
    assert_eq!(first, second);
    assert!(first.starts_with("<svg "));
    assert!(first.ends_with("</svg>"));
    assert!(first.contains("Bold"));
    assert!(first.contains("red"));
    assert!(first.contains("italic"));
    assert!(first.contains("ff0000"));
    assert!(first.contains("<path "));
}

#[test]
fn connector_scene_resolves_glued_points_and_waypoints() {
    let document = golden_model("test-corpus-connectors-route.json");
    let scene = scene_from_document(&document, 0).unwrap();
    let connectors: Vec<_> = scene
        .items
        .iter()
        .filter_map(|item| match item {
            SceneItem::Connector(connector) => Some(connector),
            _ => None,
        })
        .collect();
    assert_eq!(connectors.len(), 8);
    let first = connectors
        .iter()
        .find(|connector| connector.label == "S1")
        .expect("S1 connector");
    assert!(first.from.x.is_finite() && first.from.y.is_finite());
    assert!(first.to.x.is_finite() && first.to.y.is_finite());
    assert_eq!(first.waypoints.len(), 1);
    assert!(to_svg(&scene).contains("S1"));

    let fixed = scene_from_document(&document, 2).unwrap();
    let auto = fixed
        .items
        .iter()
        .find_map(|item| match item {
            SceneItem::Connector(connector) if connector.label == "F1" => Some(connector),
            _ => None,
        })
        .expect("F1 connector");
    assert_eq!(auto.waypoints.len(), 2, "elbow route is generated");
}

#[test]
fn rasterizes_ms_example_at_golden_page_size() {
    let document = golden_model("examples-ms-example.json");
    let scene = scene_from_document(&document, 0).unwrap();
    assert_eq!(scene.raster_size(), (745, 640));
    let png = render_png(&scene, 1.0).unwrap();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    let image = image::load_from_memory(&png).unwrap();
    assert_eq!(image.width(), 745);
    assert_eq!(image.height(), 640);
    assert!(png.len() > 5_000);
}

#[test]
fn line_gradients_and_shadows_render_in_svg() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package =
        layoutcraft_vsdx::Package::open(root.join("test/corpus/line-gradient-shadow-2.vsdx")).unwrap();
    let document = layoutcraft_vsdx::import_document(&package).unwrap();
    let scene = scene_from_document(&document, 0).unwrap();
    let svg = to_svg(&scene);
    assert!(
        svg.contains(r#"stroke="url(#grad"#),
        "line gradient stroke missing"
    );
    assert!(
        svg.contains(r#"filter="url(#shdw"#),
        "shadow blur filter missing"
    );
    assert!(svg.contains(r#"feGaussianBlur stdDeviation="4""#));
    let shape = scene
        .items
        .iter()
        .find_map(|item| match item {
            SceneItem::Shape(shape) => Some(shape),
            SceneItem::Connector(_) => None,
        })
        .unwrap();
    assert!(shape.shadow.is_some());
    assert!(shape
        .parts
        .iter()
        .any(|part| part.stroke_gradient.is_some()));
}

#[test]
fn glow_soft_edge_and_reflection_render_in_svg() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package =
        layoutcraft_vsdx::Package::open(root.join("test/corpus/effects-cells.vsdx")).unwrap();
    let document = layoutcraft_vsdx::import_document(&package).unwrap();
    let scene = scene_from_document(&document, 1).unwrap();
    let svg = to_svg(&scene);
    assert!(svg.contains(r#"filter="url(#glow"#), "glow filter missing");
    assert!(
        svg.contains(r#"filter="url(#soft"#),
        "soft edge filter missing"
    );
    assert!(
        svg.contains(r#"mask="url(#reflMask"#),
        "reflection fade mask missing"
    );
    assert!(svg.contains("reflBlur"));
}

#[test]
fn patterns_oblique_shadows_and_bevels_render_in_svg() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package =
        layoutcraft_vsdx::Package::open(root.join("test/corpus/fill-patterns.vsdx")).unwrap();
    let document = layoutcraft_vsdx::import_document(&package).unwrap();
    let svg = to_svg(&scene_from_document(&document, 1).unwrap());
    assert!(svg.contains(r#"<pattern id="pat"#), "pattern def missing");
    assert!(svg.contains(r#"fill="url(#pat"#), "pattern fill missing");

    let package = layoutcraft_vsdx::Package::open(root.join("test/corpus/bevel-types.vsdx")).unwrap();
    let document = layoutcraft_vsdx::import_document(&package).unwrap();
    let svg = to_svg(&scene_from_document(&document, 1).unwrap());
    // plain rectangles are drawn from Visio's measured bevel profiles (see bevel_profiles.rs)
    assert!(svg.contains("bevelP"), "bevel profile gradient missing");

    let package = layoutcraft_vsdx::Package::open(root.join("test/corpus/shadow-types.vsdx")).unwrap();
    let document = layoutcraft_vsdx::import_document(&package).unwrap();
    let svg = to_svg(&scene_from_document(&document, 1).unwrap());
    assert!(svg.contains("matrix(1 0 "), "oblique shadow matrix missing");
}

#[test]
fn ecommerce_connectors_and_multiline_labels_render() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = layoutcraft_vsdx::Package::open(
        root.join("examples/E-Commerce.Shopping.Process.Flow.Diagram.Visio.1.1.vsdx"),
    )
    .unwrap();
    let document = layoutcraft_vsdx::import_document(&package).unwrap();
    let svg = to_svg(&scene_from_document(&document, 0).unwrap());
    assert!(
        svg.contains(r##"stroke="#1ba1e2""##),
        "connector lines missing"
    );
    assert!(svg.contains(">Yes</text>"), "connector label missing");
    assert!(svg.matches(r#"<text "#).count() > 30);

    let svg = to_svg(&scene_from_document(&document, 1).unwrap());
    assert!(svg.contains(">Validation</text>"));
    assert!(svg.contains(">Confirmation</text>"));
}

#[test]
fn hatch_tiles_follow_the_measured_visio_pixels() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = layoutcraft_vsdx::Package::open(root.join("test/corpus/fill-patterns.vsdx")).unwrap();
    let document = layoutcraft_vsdx::import_document(&package).unwrap();
    let svg = to_svg(&scene_from_document(&document, 1).unwrap());
    // pixel tiles are 4 x 4 (patterns 15-18, 24) or 8 x 8 (pattern 4); plain horizontal/vertical
    // lines are one pixel every 8 (patterns 6, 7)
    assert!(svg.contains(r#"width="4" height="4""#), "4 x 4 pixel tiles");
    assert!(svg.contains(r#"width="8" height="8""#), "8 x 8 tiles");
    assert!(!svg.contains(r#"width="16" height="16""#), "no 16 x 16 tiles any more");
}
