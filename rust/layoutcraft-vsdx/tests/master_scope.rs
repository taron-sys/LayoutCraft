//! Geometry formulas of a master-backed group's children resolve against the
//! master shape's own cells. The page child's ID lives in a different ID space
//! and can collide with another master shape (SuperMicro PWS 1K41: page shape 6
//! is master shape 9, while master shape 6 is the full-width box).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_vsdx::{import_document, Package};

#[test]
fn page_child_ids_do_not_alias_master_shape_cells() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/visioStencils-converted/Documents/IT Vendors/SuperMicro PWS 1K41 BR v2013.vsdx");
    if !path.exists() {
        eprintln!("skipped: converted example not present");
        return;
    }
    let document = import_document(&Package::open(&path).unwrap()).unwrap();
    let group = document.pages[0].shapes.iter().max_by(|a, b| a.w.total_cmp(&b.w)).unwrap();
    let part = group
        .parts
        .iter()
        .find(|part| part.source.as_ref().is_some_and(|s| s.master_shape_id == "9"))
        .expect("master shape 9 part");
    // A fan ring is about 2.9in wide (~280px); the box it sits in is ~671px.
    let widest = part.d.split(['M', 'L', 'A', 'Z']).filter_map(|seg| {
        let nums: Vec<f64> = seg.split([' ', ',']).filter_map(|n| n.parse().ok()).collect();
        (nums.len() >= 2).then(|| nums[0])
    }).fold(0.0f64, f64::max);
    assert!(widest < 400.0, "ring must not take the box width: {}", part.d);
}
