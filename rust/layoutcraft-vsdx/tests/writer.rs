#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use serde_json::json;
use std::io::Cursor;
use std::path::PathBuf;
use layoutcraft_model::Document;
use layoutcraft_vsdx::{
    import_document, patch_page, patch_page_cells, patch_page_with_connectors,
    patch_page_with_media, write_fresh_document, write_fresh_document_with_media, GeometryRow,
    NewConnector, NewPicture, NewShape, Package, ShapeCellPatch,
};

#[test]
fn patches_shape_cells_and_preserves_other_entries() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = root.join("test/corpus/theme-quickstyles.vsdx");
    let package = Package::open(&source).unwrap();
    let page_part = package.pages().unwrap()[0].part.clone();
    let original_page = package.text(&page_part).unwrap().to_string();
    let bytes =
        patch_page_cells(&package, 0, &[ShapeCellPatch::new("1").set("PinX", "3")]).unwrap();
    assert_ne!(bytes, std::fs::read(&source).unwrap());

    let patched = Package::from_reader(Cursor::new(bytes.clone())).unwrap();
    for name in package.names() {
        if name == page_part {
            continue;
        }
        assert_eq!(
            package.entry(name),
            patched.entry(name),
            "untouched entry changed: {name}"
        );
    }
    let page = patched.text(&page_part).unwrap();
    assert!(page.contains("<Cell N='PinX' V='3'/>"));
    assert_ne!(page, original_page);

    let original = import_document(&package).unwrap();
    let updated = import_document(&patched).unwrap();
    let before = original.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("1"))
        .unwrap();
    let after = updated.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("1"))
        .unwrap();
    assert!((after.x - before.x - 144.0).abs() < 1e-6);
    assert!((after.y - before.y).abs() < 1e-6);
}

#[test]
fn patches_text_shape_data_and_geometry_rows() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = Package::open(root.join("test/corpus/theme-quickstyles.vsdx")).unwrap();
    let page_part = package.pages().unwrap()[0].part.clone();
    let bytes = patch_page_cells(
        &package,
        0,
        &[ShapeCellPatch::new("1")
            .text("Hello & <World>")
            .row_cell("Geometry", "0", "2", "X", "2")],
    )
    .unwrap();
    let patched = Package::from_reader(Cursor::new(bytes)).unwrap();
    let page = patched.text(&page_part).unwrap();
    assert!(page.contains("<Text>Hello &amp; &lt;World&gt;</Text>"));
    assert!(page.contains("<Cell N='X' V='2'/>"));
    let document = import_document(&patched).unwrap();
    let shape = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("1"))
        .unwrap();
    assert_eq!(shape.label, "Hello & <World>");
    assert!(shape.parts[0].d.contains("L192"));

    let source = root.join("test/corpus/datagraphics.vsdx");
    let package = Package::open(&source).unwrap();
    let bytes = patch_page_cells(
        &package,
        0,
        &[ShapeCellPatch::new("1")
            .data("Row_1", "Eve")
            .data("Row_4", "New value")],
    )
    .unwrap();
    let patched = Package::from_reader(Cursor::new(bytes)).unwrap();
    let document = import_document(&patched).unwrap();
    let shape = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("1"))
        .unwrap();
    assert_eq!(
        shape
            .data
            .iter()
            .find(|row| row.name == "Row_1")
            .unwrap()
            .value,
        "Eve"
    );
    assert_eq!(
        shape
            .data
            .iter()
            .find(|row| row.name == "Row_4")
            .unwrap()
            .value,
        "New value"
    );
}

#[test]
fn adds_and_removes_inline_shapes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = Package::open(root.join("test/corpus/theme-quickstyles.vsdx")).unwrap();
    let bytes = patch_page(
        &package,
        0,
        &[],
        &[NewShape::new("900", 0.0, 0.0, 100.0, 50.0)
            .fill("#112233")
            .stroke("#445566")
            .text("Added shape")],
        &[],
    )
    .unwrap();
    let patched = Package::from_reader(Cursor::new(bytes)).unwrap();
    let document = import_document(&patched).unwrap();
    assert_eq!(document.pages[0].shapes.len(), 7);
    let added = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("900"))
        .unwrap();
    assert_eq!(added.label, "Added shape");
    assert!((added.w - 100.0).abs() < 1e-6);
    assert!((added.h - 50.0).abs() < 1e-6);
    assert_eq!(added.fill.as_deref(), Some("#112233"));
    assert_eq!(added.stroke.as_deref(), Some("#445566"));
    assert!((added.x - 0.0).abs() < 1e-6);
    assert!((added.y - 0.0).abs() < 1e-6);

    let bytes = patch_page(&package, 0, &[], &[], &["1".to_string()]).unwrap();
    let patched = Package::from_reader(Cursor::new(bytes)).unwrap();
    let document = import_document(&patched).unwrap();
    assert_eq!(document.pages[0].shapes.len(), 5);
    assert!(document.pages[0]
        .shapes
        .iter()
        .all(|shape| shape.visio_id.as_deref() != Some("1")));
}

#[test]
fn adds_a_connector_with_glue_and_label() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = Package::open(root.join("test/corpus/theme-quickstyles.vsdx")).unwrap();
    let page_part = package.pages().unwrap()[0].part.clone();
    let bytes = patch_page_with_connectors(
        &package,
        0,
        &[],
        &[],
        &[],
        &[NewConnector::new("901", (144.0, 144.0), (384.0, 192.0))
            .glue("1", "2")
            .label("new link")],
    )
    .unwrap();
    let patched = Package::from_reader(Cursor::new(bytes)).unwrap();
    let page = patched.text(&page_part).unwrap();
    assert!(page.contains("<Connect FromSheet='901' FromCell='BeginX'"));
    assert!(page.contains("<Connect FromSheet='901' FromCell='EndX'"));
    let document = import_document(&patched).unwrap();
    assert_eq!(document.pages[0].connectors.len(), 1);
    let connector = &document.pages[0].connectors[0];
    assert_eq!(connector.visio_id.as_deref(), Some("901"));
    assert_eq!(connector.label, "new link");
    assert_eq!(connector.from.shape_id.as_deref(), Some("shape:visio:1"));
    assert_eq!(connector.to.shape_id.as_deref(), Some("shape:visio:2"));
    assert!(connector.from.point.is_none());
    assert!(connector.to.point.is_none());
    assert!(connector.from.local.is_some());
    assert!(connector.to.local.is_some());
}

#[test]
fn adds_a_picture_with_media_part_and_relationship() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let image_package = Package::open(root.join("test/corpus/images.vsdx")).unwrap();
    let image_bytes = image_package
        .entry("visio/media/image1.png")
        .expect("source picture")
        .to_vec();
    let package = Package::open(root.join("test/corpus/theme-quickstyles.vsdx")).unwrap();
    let page_part = package.pages().unwrap()[0].part.clone();
    let rels_part = "visio/pages/_rels/page1.xml.rels";
    let bytes = patch_page_with_media(
        &package,
        0,
        &[],
        &[],
        &[],
        &[],
        &[NewPicture::new("902", 10.0, 20.0, 100.0, 80.0, image_bytes).text("Added photo")],
    )
    .unwrap();
    let patched = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert!(patched.contains("visio/media/layoutcraft1.png"));
    let rels = patched.text(rels_part).unwrap();
    assert!(rels.contains("rIdRust1"));
    assert!(rels.contains("../media/layoutcraft1.png"));
    assert!(patched
        .text("[Content_Types].xml")
        .unwrap()
        .contains("Extension='png'"));
    let document = import_document(&patched).unwrap();
    assert_eq!(document.media.len(), 1);
    let shape = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("902"))
        .unwrap();
    assert_eq!(shape.label, "Added photo");
    assert!((shape.w - 100.0).abs() < 1e-6);
    assert!((shape.h - 80.0).abs() < 1e-6);
    let image = shape.image.as_ref().unwrap();
    assert_eq!(image.media_type.as_deref(), Some("image/png"));
    assert!(image.media_id.is_some());
    assert!(!image.unsupported);
    assert!(!page_part.is_empty());
}

#[test]
fn patches_rich_text_runs_and_geometry_rows() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = Package::open(root.join("test/corpus/theme-quickstyles.vsdx")).unwrap();
    let rich = json!({
        "paragraphs": [{
            "pp": "0",
            "align": "center",
            "bullet": false,
            "bulletStr": "•",
            "indent": 0,
            "runs": [
                {
                    "cp": "0",
                    "text": "Hello ",
                    "style": {
                        "fontSize": 20,
                        "textColor": "#112233",
                        "bold": true,
                        "italic": false,
                        "underline": false
                    }
                },
                {
                    "cp": "1",
                    "text": "world",
                    "style": {
                        "fontSize": 16,
                        "textColor": "#ff0000",
                        "bold": false,
                        "italic": true,
                        "underline": true
                    }
                }
            ],
            "style": {
                "align": "center",
                "bullet": false,
                "bulletStr": "•",
                "indent": 0
            }
        }]
    });
    let rows = vec![
        GeometryRow::new("MoveTo", "1").set("X", "0").set("Y", "0"),
        GeometryRow::new("LineTo", "2")
            .set("X", "1.8")
            .set("Y", "0"),
        GeometryRow::new("LineTo", "3")
            .set("X", "0")
            .set("Y", "1.2"),
        GeometryRow::new("LineTo", "4").set("X", "0").set("Y", "0"),
    ];
    let bytes = patch_page_cells(
        &package,
        0,
        &[ShapeCellPatch::new("1").rich(rich).geometry_rows("0", rows)],
    )
    .unwrap();
    let patched = Package::from_reader(Cursor::new(bytes)).unwrap();
    let document = import_document(&patched).unwrap();
    let shape = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("1"))
        .unwrap();
    assert_eq!(shape.label, "Hello world");
    let rich = shape.rich.as_ref().unwrap();
    let runs = rich["paragraphs"][0]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["text"], "Hello ");
    assert_eq!(runs[0]["style"]["bold"], true);
    assert_eq!(runs[0]["style"]["fontSize"].as_f64(), Some(20.0));
    assert_eq!(runs[0]["style"]["textColor"], "#112233");
    assert_eq!(runs[1]["text"], "world");
    assert_eq!(runs[1]["style"]["italic"], true);
    assert_eq!(runs[1]["style"]["underline"], true);
    assert!(shape.parts[0].d.contains("172.8"));
    assert!(shape.parts[0].d.contains("115.2"));
}

#[test]
fn fresh_document_round_trips_shapes_text_and_connectors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in [
        "test-corpus-theme-quickstyles.json",
        "test-corpus-rich-text.json",
        "test-corpus-connectors-route.json",
    ] {
        let document = Document::open(root.join("test/golden/models").join(name)).unwrap();
        let bytes = write_fresh_document(&document).unwrap();
        let package = Package::from_reader(Cursor::new(bytes)).unwrap();
        let reimported = import_document(&package).unwrap();
        assert_eq!(
            reimported.pages.len(),
            document.pages.len(),
            "{name}: pages"
        );
        for (page, original_page) in reimported.pages.iter().zip(&document.pages) {
            assert_eq!(
                page.shapes.len(),
                original_page.shapes.len(),
                "{name}: shapes"
            );
            assert_eq!(
                page.connectors.len(),
                original_page.connectors.len(),
                "{name}: connectors"
            );
            for original in &original_page.shapes {
                let id = original.visio_id.as_deref().unwrap();
                let shape = page
                    .shapes
                    .iter()
                    .find(|shape| shape.visio_id.as_deref() == Some(id))
                    .unwrap_or_else(|| panic!("{name}: shape {id} missing"));
                assert_eq!(shape.label, original.label, "{name}/{id}: label");
                assert!((shape.x - original.x).abs() < 1e-4, "{name}/{id}: x");
                assert!((shape.y - original.y).abs() < 1e-4, "{name}/{id}: y");
                assert!((shape.w - original.w).abs() < 1e-4, "{name}/{id}: w");
                assert!((shape.h - original.h).abs() < 1e-4, "{name}/{id}: h");
                assert_eq!(shape.fill, original.fill, "{name}/{id}: fill");
                assert_eq!(shape.stroke, original.stroke, "{name}/{id}: stroke");
                assert_eq!(
                    shape.parts.len(),
                    original.parts.len(),
                    "{name}/{id}: parts"
                );
                assert_eq!(shape.data.len(), original.data.len(), "{name}/{id}: data");
            }
        }
    }
}

#[test]
fn fresh_document_emits_pictures_and_media_parts() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let document = Document::open(root.join("test/golden/models/test-corpus-images.json")).unwrap();
    let source = Package::open(root.join("test/corpus/images.vsdx")).unwrap();
    let mut media = std::collections::BTreeMap::new();
    for (id, record) in &document.media {
        let part = record.part.as_deref().unwrap();
        media.insert(id.clone(), source.entry(part).unwrap().to_vec());
    }
    let bytes = write_fresh_document_with_media(&document, &media).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let reimported = import_document(&package).unwrap();
    assert_eq!(reimported.media.len(), document.media.len());
    assert_eq!(reimported.pages.len(), document.pages.len());
    for (page, original) in reimported.pages.iter().zip(&document.pages) {
        assert_eq!(page.shapes.len(), original.shapes.len());
        for original_shape in &original.shapes {
            let id = original_shape.visio_id.as_deref().unwrap();
            let shape = page
                .shapes
                .iter()
                .find(|shape| shape.visio_id.as_deref() == Some(id))
                .unwrap();
            if original_shape.image.is_some() {
                assert!(shape.image.is_some(), "{id}: image");
                assert_eq!(shape.shape_type, "image", "{id}: type");
                assert!((shape.w - original_shape.w).abs() < 1e-4, "{id}: w");
                assert!((shape.h - original_shape.h).abs() < 1e-4, "{id}: h");
            }
        }
    }
}

#[test]
fn adding_a_missing_cell_keeps_the_shape_valid() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = Package::open(root.join("test/corpus/theme-quickstyles.vsdx")).unwrap();
    let page_part = package.pages().unwrap()[0].part.clone();
    let bytes = patch_page_cells(
        &package,
        0,
        &[ShapeCellPatch::new("1").set("LineColor", "#123456")],
    )
    .unwrap();
    let patched = Package::from_reader(Cursor::new(bytes)).unwrap();
    let page = patched.text(&page_part).unwrap();
    assert!(page.contains("<Cell N='LineColor' V='#123456'/>"));
    import_document(&patched).unwrap();
}
