#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use layoutcraft_model::{Counts, Document};

#[test]
fn every_golden_model_parses_and_matches_counts() {
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/golden");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(golden.join("manifest.json")).unwrap()).unwrap();
    let fixtures = manifest["fixtures"].as_array().unwrap();
    assert_eq!(fixtures.len(), 20);

    for fixture in fixtures {
        let id = fixture["id"].as_str().unwrap();
        let model_path = golden.join(fixture["model"].as_str().unwrap());
        let doc = Document::open(&model_path).unwrap_or_else(|e| panic!("{id}: {e}"));
        doc.validate().unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(doc.schema_version, 1, "{id}");
        assert!(doc.pages.iter().all(|p| !p.name.is_empty()), "{id}");

        let expected = Counts {
            pages: fixture["counts"]["pages"].as_u64().unwrap() as usize,
            shapes: fixture["counts"]["shapes"].as_u64().unwrap() as usize,
            connectors: fixture["counts"]["connectors"].as_u64().unwrap() as usize,
            media: fixture["counts"]["media"].as_u64().unwrap() as usize,
        };
        assert_eq!(doc.counts(), expected, "{id}");
    }
}
