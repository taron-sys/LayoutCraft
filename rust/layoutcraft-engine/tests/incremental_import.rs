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

/// How many drawings a normal run edits (about 25 s); `LAYOUTCRAFT_SWEEP_ALL=1` edits every drawing found (about 14 min for the
/// 1,389 drawings on the maintainer's machine: the full gate).
const SAMPLE: usize = 45;
/// Larger drawings are left out: each edit re-imports the page twice.
const MAX_BYTES: u64 = 3_000_000;

/// Every `.vsdx` of at most `MAX_BYTES` under `test/corpus`, `examples` and `test`, subfolders included, grouped by where it comes
/// from: `test/corpus`, the rest of `test`, the files at the top of `examples`, and each folder of `examples` on its own. Each group
/// is sorted.
fn drawing_groups() -> Vec<Vec<PathBuf>> {
    fn walk(dir: &std::path::Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|e| e == "vsdx") && entry.metadata().is_ok_and(|m| m.len() <= MAX_BYTES) {
                found.push(path);
            }
        }
    }
    let mut groups: std::collections::BTreeMap<String, Vec<PathBuf>> = std::collections::BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for dir in ["test/corpus", "examples", "test"] {
        let base = root().join(dir);
        let mut found = Vec::new();
        walk(&base, &mut found);
        for path in found {
            if !seen.insert(path.clone()) {
                continue;
            }
            let inside = path.strip_prefix(&base).ok().and_then(|rest| rest.parent()).and_then(|parent| parent.components().next());
            let group = match (dir, inside) {
                ("examples", Some(folder)) => format!("examples/{}", folder.as_os_str().to_string_lossy()),
                _ => dir.to_string(),
            };
            groups.entry(group).or_default().push(path);
        }
    }
    groups.into_values().map(|mut group| { group.sort(); group }).collect()
}

/// The drawings to edit: every one with `LAYOUTCRAFT_SWEEP_ALL=1`, else `SAMPLE` of them taken in turn from each group, so every
/// source of drawings is in a normal run (the same ones on every run).
fn drawings() -> Vec<PathBuf> {
    let groups = drawing_groups();
    let all = std::env::var_os("LAYOUTCRAFT_SWEEP_ALL").is_some_and(|v| v == "1");
    let limit = if all { usize::MAX } else { SAMPLE };
    let mut chosen = Vec::new();
    for round in 0.. {
        let before = chosen.len();
        for group in &groups {
            if chosen.len() < limit {
                chosen.extend(group.get(round).cloned());
            }
        }
        if chosen.len() == before || chosen.len() >= limit {
            break;
        }
    }
    chosen
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

/// `None` when the engine's document is what a fresh import of its file gives, else where they first differ.
fn same(engine: &Engine, what: &str) -> Option<String> {
    let (a, b) = (current(engine), fresh(engine));
    first_diff(&a, &b, String::new()).map(|path| format!("{what}: the incremental import differs from a fresh one at {path}"))
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
    // the first difference of each drawing (its later edits would only repeat it); all of them are reported at the end
    let mut failures: Vec<String> = Vec::new();
    'drawings: for path in drawings() {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let mut engine = Engine::new();
        if engine.open(bytes).is_err() || engine.document().is_none_or(|d| d.pages.is_empty()) {
            continue;
        }
        files += 1;
        let ids = movable(&engine);
        if ids.is_empty() {
            continue;
        }
        // the path below the checkout: many drawings share a file name (Drawing1.vsdx)
        let name = path.strip_prefix(root()).unwrap_or(&path).to_string_lossy().to_string();
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
            if let Some(failure) = same(&engine, &format!("{name}: after op {op} on shape {id}")) {
                failures.push(failure);
                continue 'drawings;
            }
            checked_edits += 1;
        }
        // a delete changes the shape count: the stacking counter shifts, the cache must not be taken over wrongly
        // (Pictures keep their media numbers across edits on purpose, a fresh import renumbers them: compare these only on drawings without pictures.)
        let no_pictures = engine.document().is_some_and(|d| d.media.is_empty());
        let victim = ids[0].clone();
        if no_pictures && engine.edit(0, 6, &victim, 0.0, 0.0, 0.0, 0.0, 0).is_ok() {
            if let Some(failure) = same(&engine, &format!("{name}: after deleting {victim}")) {
                failures.push(failure);
                continue 'drawings;
            }
            checked_edits += 1;
        }
        // undo restores the earlier document; the next edit must still be right
        if no_pictures && engine.undo().unwrap_or(false) {
            if let Some(failure) = same(&engine, &format!("{name}: after undo")) {
                failures.push(failure);
                continue 'drawings;
            }
            checked_edits += 1;
        }
    }
    assert!(failures.is_empty(), "{} of {files} drawings differ:\n{}", failures.len(), failures.join("\n"));
    assert!(files >= 10, "too few drawings found ({files})");
    assert!(checked_edits >= 30, "too few edits compared ({checked_edits})");
    eprintln!("incremental import: {files} drawings, {checked_edits} edits compared");
}
