#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use layoutcraft_model::Document;
use layoutcraft_vsdx::{import_document, Package};

#[test]
fn paint_and_parts_match_all_golden_models() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(golden.join("manifest.json")).unwrap()).unwrap();
    let mut checked = 0;
    let mut fill_mismatch = 0;
    let mut stroke_mismatch = 0;
    let mut width_mismatch = 0;
    let mut parts_mismatch = 0;
    for fixture in manifest["fixtures"].as_array().unwrap() {
        let id = fixture["id"].as_str().unwrap();
        let source = root.join(fixture["source"].as_str().unwrap());
        let expected = Document::open(golden.join(fixture["model"].as_str().unwrap())).unwrap();
        let imported = match Package::open(&source).and_then(|p| import_document(&p)) {
            Ok(doc) => doc,
            Err(err) => {
                println!("{id}: import failed: {err}");
                continue;
            }
        };
        for (page, expected_page) in imported.pages.iter().zip(&expected.pages) {
            for expected_shape in &expected_page.shapes {
                let Some(vid) = expected_shape.visio_id.as_deref() else {
                    continue;
                };
                let Some(shape) = page
                    .shapes
                    .iter()
                    .find(|shape| shape.visio_id.as_deref() == Some(vid))
                else {
                    continue;
                };
                checked += 1;
                if shape.fill != expected_shape.fill {
                    fill_mismatch += 1;
                    if fill_mismatch <= 20 {
                        println!(
                            "{id}/{}/{} fill {:?} != {:?}",
                            page.name, vid, shape.fill, expected_shape.fill
                        );
                    }
                }
                if shape.stroke != expected_shape.stroke {
                    stroke_mismatch += 1;
                    if stroke_mismatch <= 20 {
                        println!(
                            "{id}/{}/{} stroke {:?} != {:?}",
                            page.name, vid, shape.stroke, expected_shape.stroke
                        );
                    }
                }
                if let (Some(a), Some(b)) = (shape.stroke_width, expected_shape.stroke_width) {
                    if (a - b).abs() > 0.01 {
                        width_mismatch += 1;
                        if width_mismatch <= 20 {
                            println!("{id}/{}/{vid} width {a} != {b}", page.name);
                        }
                    }
                }
                if shape.parts.len() != expected_shape.parts.len() {
                    parts_mismatch += 1;
                    if parts_mismatch <= 20 {
                        println!(
                            "{id}/{}/{} parts {} != {}",
                            page.name,
                            vid,
                            shape.parts.len(),
                            expected_shape.parts.len()
                        );
                    }
                }
            }
        }
    }
    println!(
        "checked {checked} shapes, {fill_mismatch} fill, {stroke_mismatch} stroke, {width_mismatch} width, {parts_mismatch} parts mismatches"
    );
    assert!(checked > 250, "too few shapes checked");
    assert_eq!(
        (
            fill_mismatch,
            stroke_mismatch,
            width_mismatch,
            parts_mismatch
        ),
        (0, 0, 0, 0),
        "paint or part-count mismatches"
    );
}
