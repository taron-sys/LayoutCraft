#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use layoutcraft_model::Document;
use layoutcraft_vsdx::{import_document, Package};

#[test]
fn rich_text_matches_all_golden_models() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(golden.join("manifest.json")).unwrap()).unwrap();
    let mut checked = 0;
    let mut rich_expected = 0;
    let mut rich_mismatch = 0;
    let mut align_mismatch = 0;
    let mut box_mismatch = 0;
    let mut label_mismatch = 0;
    let mut fit_mismatch = 0;
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
                if expected_shape.rich.is_some() {
                    rich_expected += 1;
                }
                if !json_close(shape.rich.as_ref(), expected_shape.rich.as_ref()) {
                    rich_mismatch += 1;
                    if rich_mismatch <= 8 {
                        println!(
                            "{id}/{}/{} rich differs; ours={} expected={}",
                            page.name,
                            vid,
                            shape.rich.is_some(),
                            expected_shape.rich.is_some()
                        );
                    }
                }
                if shape.align != expected_shape.align || shape.v_align != expected_shape.v_align {
                    align_mismatch += 1;
                    if align_mismatch <= 12 {
                        println!(
                            "{id}/{}/{} align {:?} != {:?}, vAlign {:?} != {:?}",
                            page.name,
                            vid,
                            shape.align,
                            expected_shape.align,
                            shape.v_align,
                            expected_shape.v_align
                        );
                    }
                }
                if !json_close(shape.text_box.as_ref(), expected_shape.text_box.as_ref()) {
                    box_mismatch += 1;
                }
                if shape.label != expected_shape.label {
                    label_mismatch += 1;
                }
                if !json_close(shape.text_fit.as_ref(), expected_shape.text_fit.as_ref()) {
                    fit_mismatch += 1;
                }
            }
        }
    }
    println!(
        "checked {checked} shapes, {rich_expected} expected rich, {rich_mismatch} rich mismatches, {align_mismatch} align, {box_mismatch} text box, {label_mismatch} labels, {fit_mismatch} text fit"
    );
    assert!(checked > 250, "too few shapes checked");
    assert!(rich_expected > 100, "too few rich-text shapes");
    assert_eq!(
        (
            rich_mismatch,
            align_mismatch,
            box_mismatch,
            label_mismatch,
            fit_mismatch
        ),
        (0, 0, 0, 0, 0),
        "rich text, alignment, text-box, label or fit mismatches"
    );
}

fn json_close(actual: Option<&Value>, expected: Option<&Value>) -> bool {
    match (actual, expected) {
        (None, None) => true,
        (Some(a), Some(b)) => close(a, b),
        _ => false,
    }
}

fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let a = a.as_f64().unwrap();
            let b = b.as_f64().unwrap_or(f64::NAN);
            (a - b).abs() < 1e-9
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| close(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| close(value, other)))
        }
        _ => a == b,
    }
}
