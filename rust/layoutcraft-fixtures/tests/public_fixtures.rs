//! The committed public fixture set must be exactly what the generator produces, and nothing in it may be third-party material.
use layoutcraft_fixtures::{repository_root, specs, OUTPUT_DIR};
use serde_json::Value;
use std::fs;

#[test]
fn committed_public_fixtures_match_the_generator() {
    let root = repository_root().expect("repository root");
    layoutcraft_fixtures::verify(&root).expect("regenerate with `make-public-fixtures`");
}

#[test]
fn the_public_set_contains_only_its_own_sources() {
    let root = repository_root().expect("repository root");
    let out = root.join(OUTPUT_DIR);
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    let expected: Vec<&str> = specs().iter().map(|spec| spec.id).collect();
    let listed: Vec<&str> = manifest["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|fixture| fixture["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, expected);
    for fixture in manifest["fixtures"].as_array().unwrap() {
        let source = fixture["source"].as_str().unwrap();
        assert!(
            source.starts_with(&format!("{OUTPUT_DIR}/sources/")),
            "public source outside its own folder: {source}"
        );
        assert_eq!(fixture["kind"], "self-authored");
    }
}

#[test]
fn the_public_provenance_is_cleared_and_redistributable() {
    let root = repository_root().expect("repository root");
    let provenance: Value = serde_json::from_str(
        &fs::read_to_string(root.join(OUTPUT_DIR).join("provenance.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(provenance["reviewStatus"], "cleared");
    for fixture in provenance["fixtures"].as_array().unwrap() {
        assert_eq!(fixture["origin"], "self-authored");
        assert_eq!(fixture["status"], "cleared");
        assert_eq!(fixture["redistributable"], true);
    }
}
