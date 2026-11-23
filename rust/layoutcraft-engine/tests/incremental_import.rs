//! An edit re-imports the changed page incrementally (shared parts and the flattened output of unchanged top-level shapes are taken over, see
//! `layoutcraft_vsdx::import_document_with`). The result must be exactly what a fresh import of the same file gives: this test edits real drawings
//! several times in a row and compares the engine's document with a fresh import after every edit.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_engine::Engine;
use layoutcraft_vsdx::{import_document, Package};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn drawings() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in ["test/corpus", "examples", "test"] {
        let Ok(entries) = std::fs::read_dir(root().join(dir)) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "vsdx") && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The path of the first difference of two JSON values (for the failure message).
fn first_diff(a: &serde_json::Value, b: &serde_json::Value, path: std::string::String) -> Option<std::string::String> {
    use serde_json::Value::*;
    match (a, b) {
        (Object(x), Object(y)) => {
            for key in x.keys().chain(y.keys()) {
                match (x.get(key), y.get(key)) {
                    (Some(p), Some(q)) => {
                        if let Some(found) = first_diff(p, q, format!("{path}/{key}")) {
                            return Some(found);
                        }
                    }
                    _ => return Some(format!("{path}/{key} (only on one side)")),
                }
            }
            None
        }
        (Array(x), Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path} (length {} vs {})", x.len(), y.len()));
            }
            x.iter().zip(y).enumerate().find_map(|(i, (p, q))| first_diff(p, q, format!("{path}[{i}]")))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: {} vs {}", a.to_string().chars().take(80).collect::<std::string::String>(), b.to_string().chars().take(80).collect::<std::string::String>())),
    }
}

fn same(engine: &Engine, what: &str) {
    let (a, b) = (current(engine), fresh(engine));
    if let Some(path) = first_diff(&a, &b, String::new()) {
        panic!("{what}: the incremental import differs from a fresh one at {path}");
    }
}

fn fresh(engine: &Engine) -> serde_json::Value {
    let bytes = engine.export().expect("export").to_vec();
    let package = Package::from_bytes(&bytes).expect("package");
    let document = import_document(&package).expect("fresh import");
    serde_json::to_value(&document).expect("json")
}

fn current(engine: &Engine) -> serde_json::Value {
    serde_json::to_value(engine.document().expect("document")).expect("json")
}

/// The ids of page-level shapes that are not connectors and have a size (candidates for an edit).
fn movable(engine: &Engine) -> Vec<String> {
    let document = engine.document().unwrap();
    let page = &document.pages[0];
    let connectors: Vec<&str> = page.connectors.iter().filter_map(|c| c.visio_id.as_deref()).collect();
    page.shapes
        .iter()
        .filter(|s| s.group.is_none() && s.w > 1.0 && s.h > 1.0)
        .filter_map(|s| s.visio_id.clone())
        .filter(|id| !connectors.contains(&id.as_str()))
        .collect()
}

#[test]
fn incremental_import_equals_a_fresh_import_after_edits() {
    let mut checked_edits = 0;
    let mut files = 0;
    for path in drawings().into_iter().take(45) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.len() > 3_000_000 {
            continue;
        }
        let mut engine = Engine::new();
        if engine.open(bytes).is_err() || engine.document().is_none_or(|d| d.pages.is_empty()) {
            continue;
        }
        files += 1;
        let ids = movable(&engine);
        if ids.is_empty() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        // three edits in a row: the second and third use the cache the previous import left
        let steps: Vec<(i32, &str, f64)> = vec![(2, &ids[0], 0.3), (2, &ids[ids.len() / 2], -0.2), (7, &ids[ids.len() - 1], 0.6)];
        for (op, id, amount) in steps {
            let result = match op {
                2 => engine.edit(0, 2, id, amount, amount, 0.0, 0.0, 0),
                _ => engine.edit(0, 7, id, amount, 0.0, 0.0, 0.0, 0),
            };
            if result.is_err() {
                continue; // an edit the engine refuses for this shape (locked, no frame): nothing to compare
            }
            same(&engine, &format!("{name}: after op {op} on shape {id}"));
            checked_edits += 1;
        }
        // a delete changes the shape count: the stacking counter shifts, the cache must not be taken over wrongly
        // (Pictures keep their media numbers across edits on purpose, a fresh import renumbers them: compare these only on drawings without pictures.)
        let no_pictures = engine.document().is_some_and(|d| d.media.is_empty());
        let victim = ids[0].clone();
        if no_pictures && engine.edit(0, 6, &victim, 0.0, 0.0, 0.0, 0.0, 0).is_ok() {
            same(&engine, &format!("{name}: after deleting {victim}"));
            checked_edits += 1;
        }
        // undo restores the earlier document; the next edit must still be right
        if no_pictures && engine.undo().unwrap_or(false) {
            same(&engine, &format!("{name}: after undo"));
            checked_edits += 1;
        }
    }
    assert!(files >= 10, "too few drawings found ({files})");
    assert!(checked_edits >= 30, "too few edits compared ({checked_edits})");
    eprintln!("incremental import: {files} drawings, {checked_edits} edits compared");
}
