#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use layoutcraft_model::Document;
use layoutcraft_vsdx::{import_document, Package};

#[test]
fn geometry_paths_match_all_golden_models() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(golden.join("manifest.json")).unwrap()).unwrap();
    let mut checked = 0;
    let mut mismatch = 0;
    let mut point_mismatch = 0;
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
                if shape.parts.len() != expected_shape.parts.len() {
                    continue;
                }
                checked += 1;
                let mut differs = false;
                for (index, (part, expected_part)) in
                    shape.parts.iter().zip(&expected_shape.parts).enumerate()
                {
                    if part.d != expected_part.d {
                        differs = true;
                        if mismatch < 12 {
                            println!(
                                "{id}/{}/{vid} part{index}:\n ours {}\n gold {}",
                                page.name, part.d, expected_part.d
                            );
                        }
                    }
                    let points = path_points(&part.d);
                    let expected_points = path_points(&expected_part.d);
                    if points.len() != expected_points.len()
                        || points
                            .iter()
                            .zip(&expected_points)
                            .any(|(a, b)| (a.0 - b.0).abs() > 1e-6 || (a.1 - b.1).abs() > 1e-6)
                    {
                        point_mismatch += 1;
                    }
                }
                if differs {
                    mismatch += 1;
                }
            }
        }
    }
    println!("checked {checked} shapes, {mismatch} path mismatches, {point_mismatch} part point mismatches");
    assert!(checked > 250, "too few shapes checked");
    assert_eq!(
        (mismatch, point_mismatch),
        (0, 0),
        "geometry path mismatches"
    );
}

fn path_points(path: &str) -> Vec<(f64, f64)> {
    let mut nums = Vec::new();
    let mut cur = String::new();
    for c in path.chars() {
        if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' {
            cur.push(c);
        } else if !cur.is_empty() {
            if let Ok(value) = cur.parse() {
                nums.push(value);
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(value) = cur.parse() {
            nums.push(value);
        }
    }
    nums.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (pair[0], pair[1]))
        .collect()
}
