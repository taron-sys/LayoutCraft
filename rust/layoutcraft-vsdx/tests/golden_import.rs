#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use layoutcraft_model::Document;
use layoutcraft_vsdx::{import_document, Package};

#[test]
fn simple_corpora_import_geometry_labels_and_shape_data() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(golden.join("manifest.json")).unwrap()).unwrap();
    let fixtures = manifest["fixtures"].as_array().unwrap();
    let ids = [
        "test-corpus-theme-quickstyles",
        "test-corpus-rich-text",
        "test-corpus-datagraphics",
    ];
    for fixture_id in ids {
        let fixture = fixtures
            .iter()
            .find(|f| f["id"].as_str() == Some(fixture_id))
            .unwrap();
        let source = root.join(fixture["source"].as_str().unwrap());
        let package = Package::open(&source).unwrap();
        let imported = import_document(&package).unwrap();
        let expected = Document::open(golden.join(fixture["model"].as_str().unwrap())).unwrap();
        assert_eq!(imported.pages.len(), expected.pages.len(), "{fixture_id}");
        for (page, expected_page) in imported.pages.iter().zip(&expected.pages) {
            assert_eq!(page.name, expected_page.name, "{fixture_id}");
            assert_eq!(
                page.shapes.len(),
                expected_page.shapes.len(),
                "{fixture_id}"
            );
            for expected_shape in &expected_page.shapes {
                let id = expected_shape.visio_id.as_deref().unwrap();
                let shape = page
                    .shapes
                    .iter()
                    .find(|s| s.visio_id.as_deref() == Some(id))
                    .unwrap_or_else(|| panic!("{fixture_id}: shape {id} missing"));
                assert!(
                    (shape.x - expected_shape.x).abs() < 1e-4,
                    "{fixture_id}/{id}: x"
                );
                assert!(
                    (shape.y - expected_shape.y).abs() < 1e-4,
                    "{fixture_id}/{id}: y"
                );
                assert!(
                    (shape.w - expected_shape.w).abs() < 1e-4,
                    "{fixture_id}/{id}: w"
                );
                assert!(
                    (shape.h - expected_shape.h).abs() < 1e-4,
                    "{fixture_id}/{id}: h"
                );
                assert!(
                    (shape.rot - expected_shape.rot).abs() < 1e-6,
                    "{fixture_id}/{id}: rot"
                );
                assert_eq!(
                    shape.flip_x, expected_shape.flip_x,
                    "{fixture_id}/{id}: flipX"
                );
                assert_eq!(
                    shape.flip_y, expected_shape.flip_y,
                    "{fixture_id}/{id}: flipY"
                );
                assert_eq!(
                    shape.label, expected_shape.label,
                    "{fixture_id}/{id}: label"
                );
                assert_eq!(
                    shape.shape_type, expected_shape.shape_type,
                    "{fixture_id}/{id}: type"
                );
                assert_eq!(
                    shape.parts.len(),
                    expected_shape.parts.len(),
                    "{fixture_id}/{id}: parts"
                );
                for (part, expected_part) in shape.parts.iter().zip(&expected_shape.parts) {
                    let points = path_points(&part.d);
                    let expected_points = path_points(&expected_part.d);
                    assert_eq!(
                        points.len(),
                        expected_points.len(),
                        "{fixture_id}/{id}: path len"
                    );
                    for (a, b) in points.iter().zip(&expected_points) {
                        assert!(
                            (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4,
                            "{fixture_id}/{id}: path {a:?} != {b:?}"
                        );
                    }
                }
                assert_eq!(
                    shape.data.len(),
                    expected_shape.data.len(),
                    "{fixture_id}/{id}: data len"
                );
                for expected_row in &expected_shape.data {
                    let row = shape
                        .data
                        .iter()
                        .find(|r| r.name == expected_row.name)
                        .unwrap_or_else(|| panic!("{fixture_id}/{id}: data {}", expected_row.name));
                    assert_eq!(row.label, expected_row.label);
                    assert_eq!(row.value, expected_row.value);
                    assert_eq!(row.prompt, expected_row.prompt);
                    assert_eq!(row.format, expected_row.format);
                    assert_eq!(row.r#type, expected_row.r#type);
                    assert_eq!(row.invisible, expected_row.invisible);
                }
            }
        }
    }
}

#[test]
fn direct_style_cells_reach_the_editable_shape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let package = Package::open(root.join("examples/MS Example.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let expected = Document::open(golden.join("models/examples-ms-example.json")).unwrap();
    let shape = imported.pages[0]
        .shapes
        .iter()
        .find(|s| s.visio_id.as_deref() == Some("1"))
        .unwrap();
    let expected_shape = expected.pages[0]
        .shapes
        .iter()
        .find(|s| s.visio_id.as_deref() == Some("1"))
        .unwrap();
    assert_eq!(shape.fill, expected_shape.fill);
    assert_eq!(shape.stroke, expected_shape.stroke);
    assert!((shape.stroke_width.unwrap() - expected_shape.stroke_width.unwrap()).abs() < 1e-6);
    assert!((shape.font_size.unwrap() - expected_shape.font_size.unwrap()).abs() < 1e-6);
    assert_eq!(shape.text_color, expected_shape.text_color);
    assert_eq!(shape.bold, expected_shape.bold);
    assert_eq!(shape.italic, expected_shape.italic);
    assert_eq!(shape.underline, expected_shape.underline);
    assert_eq!(shape.parts.len(), expected_shape.parts.len());
    assert_eq!(shape.parts[0].fill, expected_shape.parts[0].fill);
    assert_eq!(shape.parts[0].stroke, expected_shape.parts[0].stroke);
    assert_eq!(
        shape.parts[0].stroke_width,
        expected_shape.parts[0].stroke_width
    );
}

#[test]
fn group_members_flatten_to_world_shapes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let package = Package::open(root.join("test/corpus/groups-nested.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let expected = Document::open(golden.join("models/test-corpus-groups-nested.json")).unwrap();
    let page = &imported.pages[0];
    let expected_page = &expected.pages[0];
    assert_eq!(page.shapes.len(), expected_page.shapes.len());
    for expected_shape in &expected_page.shapes {
        let id = expected_shape.visio_id.as_deref().unwrap();
        let shape = page
            .shapes
            .iter()
            .find(|s| s.visio_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("shape {id} missing"));
        assert!((shape.x - expected_shape.x).abs() < 1e-4, "{id}: x");
        assert!((shape.y - expected_shape.y).abs() < 1e-4, "{id}: y");
        assert!((shape.w - expected_shape.w).abs() < 1e-4, "{id}: w");
        assert!((shape.h - expected_shape.h).abs() < 1e-4, "{id}: h");
        assert_eq!(shape.label, expected_shape.label, "{id}: label");
        assert_eq!(shape.group, expected_shape.group, "{id}: group");
        assert_eq!(shape.parts.len(), expected_shape.parts.len(), "{id}: parts");
        for (part, expected_part) in shape.parts.iter().zip(&expected_shape.parts) {
            let points = path_points(&part.d);
            let expected_points = path_points(&expected_part.d);
            assert_eq!(points.len(), expected_points.len(), "{id}: path len");
            for (a, b) in points.iter().zip(&expected_points) {
                assert!(
                    (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4,
                    "{id}: path {a:?} != {b:?}"
                );
            }
        }
    }
    assert_eq!(page.shapes.len(), 3);
}

#[test]
fn connectors_import_glue_routes_and_waypoints() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let package = Package::open(root.join("test/corpus/connectors-route.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let expected = Document::open(golden.join("models/test-corpus-connectors-route.json")).unwrap();
    assert_eq!(imported.pages.len(), expected.pages.len());
    for (page, expected_page) in imported.pages.iter().zip(&expected.pages) {
        assert_eq!(
            page.shapes.len(),
            expected_page.shapes.len(),
            "{} shapes",
            page.name
        );
        assert_eq!(
            page.connectors.len(),
            expected_page.connectors.len(),
            "{} connectors",
            page.name
        );
        for expected_conn in &expected_page.connectors {
            let id = expected_conn.visio_id.as_deref().unwrap();
            let conn = page
                .connectors
                .iter()
                .find(|c| c.visio_id.as_deref() == Some(id))
                .unwrap_or_else(|| panic!("{}: connector {id} missing", page.name));
            assert_eq!(conn.route, expected_conn.route, "{id}: route");
            assert_eq!(conn.label, expected_conn.label, "{id}: label");
            assert_eq!(
                conn.route_style, expected_conn.route_style,
                "{id}: routeStyle"
            );
            assert_eq!(
                conn.con_fixed_code, expected_conn.con_fixed_code,
                "{id}: conFixedCode"
            );
            assert_eq!(
                conn.line_route_ext, expected_conn.line_route_ext,
                "{id}: lineRouteExt"
            );
            assert_eq!(
                conn.walk_preference, expected_conn.walk_preference,
                "{id}: walkPreference"
            );
            assert_eq!(conn.plow_code, expected_conn.plow_code, "{id}: plowCode");
            assert_eq!(conn.glue_type, expected_conn.glue_type, "{id}: glueType");
            assert_eq!(
                conn.dyn_feedback, expected_conn.dyn_feedback,
                "{id}: dynFeedback"
            );
            assert_eq!(conn.z, expected_conn.z, "{id}: z");
            assert_eq!(
                conn.from.shape_id, expected_conn.from.shape_id,
                "{id}: from id"
            );
            assert_eq!(conn.to.shape_id, expected_conn.to.shape_id, "{id}: to id");
            if !corner_local(&expected_conn.from.local) {
                assert_eq!(
                    conn.from.side, expected_conn.from.side,
                    "{}/{}: from side",
                    page.name, id
                );
            }
            if !corner_local(&expected_conn.to.local) {
                assert_eq!(
                    conn.to.side, expected_conn.to.side,
                    "{}/{}: to side",
                    page.name, id
                );
            }
            assert_endpoint(&conn.from, &expected_conn.from, id, "from");
            assert_endpoint(&conn.to, &expected_conn.to, id, "to");
            assert_eq!(
                conn.waypoints.len(),
                expected_conn.waypoints.len(),
                "{id}: waypoints"
            );
            for (a, b) in conn.waypoints.iter().zip(&expected_conn.waypoints) {
                assert!((a["x"].as_f64().unwrap() - b["x"].as_f64().unwrap()).abs() < 1e-4);
                assert!((a["y"].as_f64().unwrap() - b["y"].as_f64().unwrap()).abs() < 1e-4);
            }
        }
    }
}

fn corner_local(value: &Option<serde_json::Value>) -> bool {
    let Some(value) = value else { return true };
    let x = value["x"].as_f64().unwrap_or(0.0);
    let y = value["y"].as_f64().unwrap_or(0.0);
    let at = |v: f64| v < 1e-9 || (v - 1.0).abs() < 1e-9;
    at(x) && at(y)
}

fn assert_endpoint(
    actual: &layoutcraft_model::Endpoint,
    expected: &layoutcraft_model::Endpoint,
    id: &str,
    side: &str,
) {
    match (&actual.local, &expected.local) {
        (Some(a), Some(b)) => {
            assert!((a["x"].as_f64().unwrap() - b["x"].as_f64().unwrap()).abs() < 1e-4);
            assert!((a["y"].as_f64().unwrap() - b["y"].as_f64().unwrap()).abs() < 1e-4);
        }
        (None, None) => {}
        _ => panic!("{id}: {side} local mismatch"),
    }
    match (&actual.point, &expected.point) {
        (Some(a), Some(b)) => {
            assert!((a["x"].as_f64().unwrap() - b["x"].as_f64().unwrap()).abs() < 1e-4);
            assert!((a["y"].as_f64().unwrap() - b["y"].as_f64().unwrap()).abs() < 1e-4);
        }
        (None, None) => {}
        _ => panic!("{id}: {side} point mismatch"),
    }
}

#[test]
fn rotated_group_members_compose_world_transforms() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let package = Package::open(root.join("test/corpus/groups-rotated.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let expected = Document::open(golden.join("models/test-corpus-groups-rotated.json")).unwrap();
    let page = &imported.pages[0];
    let expected_page = &expected.pages[0];
    assert_eq!(page.shapes.len(), 3);
    for expected_shape in &expected_page.shapes {
        let id = expected_shape.visio_id.as_deref().unwrap();
        let shape = page
            .shapes
            .iter()
            .find(|s| s.visio_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("shape {id} missing"));
        assert!((shape.x - expected_shape.x).abs() < 1e-4, "{id}: x");
        assert!((shape.y - expected_shape.y).abs() < 1e-4, "{id}: y");
        assert!((shape.w - expected_shape.w).abs() < 1e-4, "{id}: w");
        assert!((shape.h - expected_shape.h).abs() < 1e-4, "{id}: h");
        assert!((shape.rot - expected_shape.rot).abs() < 1e-6, "{id}: rot");
        assert_eq!(shape.label, expected_shape.label, "{id}: label");
        assert_eq!(shape.group, expected_shape.group, "{id}: group");
        assert_eq!(shape.parts.len(), expected_shape.parts.len(), "{id}: parts");
        for (part, expected_part) in shape.parts.iter().zip(&expected_shape.parts) {
            let points = path_points(&part.d);
            let expected_points = path_points(&expected_part.d);
            assert_eq!(points.len(), expected_points.len(), "{id}: path len");
            for (a, b) in points.iter().zip(&expected_points) {
                assert!(
                    (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4,
                    "{id}: path {a:?} != {b:?}"
                );
            }
        }
    }
}

#[test]
fn pictures_import_media_and_image_cells() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let package = Package::open(root.join("test/corpus/images.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let expected = Document::open(golden.join("models/test-corpus-images.json")).unwrap();
    assert_eq!(imported.media.len(), expected.media.len());
    for (id, expected_media) in &expected.media {
        let media = imported
            .media
            .get(id)
            .unwrap_or_else(|| panic!("media {id} missing"));
        assert_eq!(media.media_type, expected_media.media_type);
        assert_eq!(media.ext, expected_media.ext);
        assert_eq!(media.name, expected_media.name);
        assert_eq!(media.part, expected_media.part);
        assert_eq!(media.width, expected_media.width);
        assert_eq!(media.height, expected_media.height);
        assert_eq!(media.adjust, expected_media.adjust);
        assert_eq!(media.unsupported, expected_media.unsupported);
        assert_eq!(media.sha256, expected_media.sha256);
        assert_eq!(media.bytes, expected_media.bytes);
    }
    assert_eq!(imported.pages.len(), expected.pages.len());
    for (page, expected_page) in imported.pages.iter().zip(&expected.pages) {
        assert_eq!(
            page.shapes.len(),
            expected_page.shapes.len(),
            "{} shapes",
            page.name
        );
        for expected_shape in &expected_page.shapes {
            let id = expected_shape.visio_id.as_deref().unwrap();
            let shape = page
                .shapes
                .iter()
                .find(|s| s.visio_id.as_deref() == Some(id))
                .unwrap_or_else(|| panic!("{}: shape {id} missing", page.name));
            assert_eq!(shape.label, expected_shape.label, "{id}: label");
            if expected_shape.image.is_none() {
                assert_eq!(
                    shape.shape_type, "custom",
                    "{}/{}: non-image type",
                    page.name, id
                );
                continue;
            }
            assert_eq!(shape.shape_type, "image", "{}/{}: type", page.name, id);
            assert_eq!(shape.fill.as_deref(), Some("none"), "{id}: fill");
            let image = shape
                .image
                .as_ref()
                .unwrap_or_else(|| panic!("{id}: image missing"));
            let expected_image = expected_shape.image.as_ref().unwrap();
            assert_eq!(image.media_id, expected_image.media_id, "{id}: media");
            assert_eq!(
                image.media_type, expected_image.media_type,
                "{id}: media type"
            );
            assert_eq!(image.name, expected_image.name, "{id}: media name");
            assert_eq!(
                image.foreign_type, expected_image.foreign_type,
                "{id}: foreign type"
            );
            assert!(
                (image.offset_x - expected_image.offset_x).abs() < 1e-9,
                "{id}: offsetX"
            );
            assert!(
                (image.offset_y - expected_image.offset_y).abs() < 1e-9,
                "{id}: offsetY"
            );
            assert!(
                (image.scale_x - expected_image.scale_x).abs() < 1e-9,
                "{id}: scaleX"
            );
            assert!(
                (image.scale_y - expected_image.scale_y).abs() < 1e-9,
                "{id}: scaleY"
            );
            assert!(
                (image.brightness - expected_image.brightness).abs() < 1e-9,
                "{id}: brightness"
            );
            assert!(
                (image.contrast - expected_image.contrast).abs() < 1e-9,
                "{id}: contrast"
            );
            assert!(
                (image.gamma - expected_image.gamma).abs() < 1e-9,
                "{id}: gamma"
            );
            assert!(
                (image.transparency - expected_image.transparency).abs() < 1e-9,
                "{id}: transparency"
            );
            assert_eq!(
                image.unsupported, expected_image.unsupported,
                "{id}: unsupported"
            );
        }
    }
}

#[test]
fn themes_and_quick_styles_resolve_expected_paints() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let package = Package::open(root.join("test/corpus/theme-quickstyles.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let expected =
        Document::open(golden.join("models/test-corpus-theme-quickstyles.json")).unwrap();
    assert_eq!(imported.theme, expected.theme, "theme palette");
    for expected_shape in &expected.pages[0].shapes {
        let id = expected_shape.visio_id.as_deref().unwrap();
        let shape = imported.pages[0]
            .shapes
            .iter()
            .find(|shape| shape.visio_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("shape {id} missing"));
        assert_eq!(shape.fill, expected_shape.fill, "{id}: fill");
        assert_eq!(shape.stroke, expected_shape.stroke, "{id}: stroke");
        assert!(
            (shape.stroke_width.unwrap() - expected_shape.stroke_width.unwrap()).abs() < 1e-9,
            "{id}: stroke width"
        );
        assert_eq!(shape.font_size, expected_shape.font_size, "{id}: font size");
        assert_eq!(
            shape.font_family, expected_shape.font_family,
            "{id}: font family"
        );
        assert_eq!(shape.text_color, expected_shape.text_color, "{id}: text");
        assert_eq!(shape.bold, expected_shape.bold, "{id}: bold");
        assert_eq!(shape.italic, expected_shape.italic, "{id}: italic");
        assert_eq!(shape.underline, expected_shape.underline, "{id}: underline");
    }
}

#[test]
fn master_derived_theme_palette_is_imported() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let package = Package::open(root.join("examples/MS Example.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let expected = Document::open(golden.join("models/examples-ms-example.json")).unwrap();
    assert_eq!(imported.theme, expected.theme);
    let shape = imported.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("2"))
        .unwrap();
    let expected_shape = expected.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("2"))
        .unwrap();
    assert_eq!(shape.fill, expected_shape.fill);
    assert_eq!(shape.stroke, expected_shape.stroke);
    assert_eq!(shape.font_family, expected_shape.font_family);
    assert_eq!(shape.text_color, expected_shape.text_color);
}

#[test]
fn rich_text_runs_paragraphs_fields_and_text_boxes_import() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let package = Package::open(root.join("test/corpus/rich-text.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let expected = Document::open(golden.join("models/test-corpus-rich-text.json")).unwrap();
    assert_eq!(
        imported.pages[0].shapes.len(),
        expected.pages[0].shapes.len()
    );
    for expected_shape in &expected.pages[0].shapes {
        let id = expected_shape.visio_id.as_deref().unwrap();
        let shape = imported.pages[0]
            .shapes
            .iter()
            .find(|shape| shape.visio_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("shape {id} missing"));
        assert_value_close(
            shape.rich.as_ref(),
            expected_shape.rich.as_ref(),
            id,
            "rich",
        );
        assert_eq!(shape.align, expected_shape.align, "{id}: align");
        assert_eq!(shape.v_align, expected_shape.v_align, "{id}: vAlign");
        assert_value_close(
            shape.text_box.as_ref(),
            expected_shape.text_box.as_ref(),
            id,
            "text box",
        );
        assert_value_close(
            shape.text_fit.as_ref(),
            expected_shape.text_fit.as_ref(),
            id,
            "text fit",
        );
        assert_eq!(shape.label, expected_shape.label, "{id}: label");
    }
}

#[test]
fn layoutcraft_data_colour_rule_imports_from_written_vsdx() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = Package::open(root.join("test/corpus/datacolor-vsdx.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    let shape = imported.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("1"))
        .unwrap();
    let color = shape.data_color.as_ref().expect("data colour");
    assert_eq!(color.field, "Row_2");
    assert_eq!(color.min, 0.0);
    assert_eq!(color.max, 2000.0);
    assert_eq!(color.low, "#f8696b");
    assert_eq!(color.mid, "#ffeb84");
    assert_eq!(color.high, "#63be7b");
    assert_eq!(color.base_fill.as_deref(), Some("#ffffff"));
    assert_eq!(shape.fill.as_deref(), Some("#ffeb84"));
}

#[test]
fn layoutcraft_callout_metadata_and_links_import_from_written_vsdx() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = Package::open(root.join("test/corpus/dataicon-vsdx.vsdx")).unwrap();
    let imported = import_document(&package).unwrap();
    for (id, set, position) in [
        ("4", "boxes", "left"),
        ("5", "diamonds", "right"),
        ("6", "circles", "top"),
    ] {
        let shape = imported.pages[0]
            .shapes
            .iter()
            .find(|shape| shape.visio_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("shape {id} missing"));
        assert_eq!(shape.shape_type, "dataIcon", "{id}: type");
        assert!(shape.parts.is_empty(), "{id}: parts");
        assert_eq!(
            shape.attached_to.as_deref(),
            Some("shape:visio:1"),
            "{id}: link"
        );
        let callout = shape
            .callout
            .as_ref()
            .unwrap_or_else(|| panic!("{id}: callout"));
        assert_eq!(callout.kind, "icon", "{id}: kind");
        assert_eq!(callout.icon_set.as_deref(), Some(set), "{id}: icon set");
        assert_eq!(callout.field, "Row_2", "{id}: field");
        assert_eq!(callout.position, position, "{id}: position");
        assert!(!callout.auto_size, "{id}: auto size");
        assert_eq!(callout.min, Some(0.0), "{id}: min");
        assert_eq!(callout.max, Some(2000.0), "{id}: max");
        assert_eq!(callout.show_value, Some(false), "{id}: show value");
        assert_eq!(
            callout.target_visio_id.as_deref(),
            Some("1"),
            "{id}: target"
        );
        assert!(
            shape.data.iter().all(|row| row.name != "msvCalloutField"),
            "{id}: transport row leaked into Shape Data"
        );
    }
}

fn assert_value_close(actual: Option<&Value>, expected: Option<&Value>, id: &str, field: &str) {
    match (actual, expected) {
        (Some(actual), Some(expected)) => {
            assert_value_close_inner(actual, expected, &format!("{id}/{field}"))
        }
        (None, None) => {}
        _ => panic!("{id}/{field}: presence mismatch"),
    }
}

fn assert_value_close_inner(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            let a = a.as_f64().unwrap();
            let b = b.as_f64().unwrap_or(f64::NAN);
            assert!((a - b).abs() < 1e-9, "{path}: {a} != {b}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}: length");
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                assert_value_close_inner(a, b, &format!("{path}/{index}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{path}: key count");
            for (key, a) in a {
                let b = b
                    .get(key)
                    .unwrap_or_else(|| panic!("{path}: missing {key}"));
                assert_value_close_inner(a, b, &format!("{path}/{key}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}

fn path_points(path: &str) -> Vec<(f64, f64)> {
    let mut nums = Vec::new();
    let mut cur = String::new();
    for c in path.chars() {
        if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                if let Ok(v) = cur.parse() {
                    nums.push(v);
                }
                cur.clear();
            }
        }
    }
    if !cur.is_empty() {
        if let Ok(v) = cur.parse() {
            nums.push(v);
        }
    }
    nums.as_chunks::<2>()
        .0
        .iter()
        .map(|p| (p[0], p[1]))
        .collect()
}
