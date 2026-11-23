#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use layoutcraft_model::Document;
use layoutcraft_vsdx::Package;

#[test]
fn every_golden_source_opens_and_exposes_its_pages() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let golden = root.join("test/golden");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(golden.join("manifest.json")).unwrap()).unwrap();
    let fixtures = manifest["fixtures"].as_array().unwrap();
    assert_eq!(fixtures.len(), 20);
    let mut total_master_derived = 0usize;

    for fixture in fixtures {
        let id = fixture["id"].as_str().unwrap();
        let source = root.join(fixture["source"].as_str().unwrap());
        let package = Package::open(&source).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(
            package.entry_count(),
            fixture["untouched"]["entryCount"].as_u64().unwrap() as usize,
            "{id}: entry count"
        );
        let pages = package.pages().unwrap_or_else(|e| panic!("{id}: {e}"));
        let model = Document::open(golden.join(fixture["model"].as_str().unwrap()))
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(pages.len(), model.pages.len(), "{id}: page count");
        for (page, expected) in pages.iter().zip(&model.pages) {
            assert_eq!(page.name, expected.name, "{id}: page name");
            assert!(
                package.contains(&page.part),
                "{id}: missing page part {}",
                page.part
            );
            let tree = package.page_shapes(&page.part).unwrap();
            let mut ids = Vec::new();
            for node in &tree {
                node.walk(&mut |n| ids.push(n.id.clone()));
            }
            assert!(!ids.is_empty(), "{id}/{}: no shapes", page.name);
            for shape in &expected.shapes {
                let visio_id = shape.visio_id.as_deref().expect("golden shape visioId");
                assert!(
                    ids.iter().any(|id| id == visio_id),
                    "{id}/{}: golden shape {visio_id} missing from XML tree",
                    page.name
                );
            }
            let resolved = package.resolve_page_shapes(&page.part).unwrap();
            let mut resolved_ids = Vec::new();
            for node in &resolved {
                node.walk(&mut |n| {
                    resolved_ids.push(n.id.clone());
                    if let Some(master) = &n.master_part {
                        assert!(
                            package.contains(master),
                            "{id}: missing master part {master}"
                        );
                        assert!(
                            n.master_shape_id.is_some(),
                            "{id}: master without selected shape"
                        );
                        assert!(
                            !n.cells.is_empty(),
                            "{id}: master-derived shape has no cells"
                        );
                        total_master_derived += 1;
                    }
                });
            }
            assert_eq!(ids, resolved_ids, "{id}/{}: resolved tree ids", page.name);
        }
    }
    assert!(
        total_master_derived > 0,
        "no master-derived shapes resolved"
    );
}
