//! Refresh the canonical model baseline from the Rust importer.
//!
//! usage: refresh-golden-models [root]
//!
//! Rewrites `test/golden/models/<id>.json` for every manifest fixture from the
//! current `layoutcraft-vsdx` output, then updates the manifest's model hashes and
//! counts. Source bytes, source hashes and untouched-patch entries are left
//! alone: this tool only refreshes the document-model baseline.
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use layoutcraft_vsdx::{import_document, Package};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    manifest_version: u32,
    document_schema_version: u32,
    generated_by: String,
    fixtures: Vec<Fixture>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    id: String,
    kind: String,
    source: String,
    source_bytes: u64,
    source_sha256: String,
    document_schema_version: u32,
    model: String,
    model_sha256: String,
    model_file_sha256: String,
    counts: Counts,
    untouched: Untouched,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    pages: usize,
    shapes: usize,
    connectors: usize,
    media: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Untouched {
    saved_name: String,
    entry_count: usize,
    entries: BTreeMap<String, String>,
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let golden = root.join("test/golden");
    let manifest_path = golden.join("manifest.json");
    let text = fs::read_to_string(&manifest_path).expect("read manifest");
    let mut manifest: Manifest = serde_json::from_str(&text).expect("parse manifest");
    let mut changed = 0usize;
    for fixture in &mut manifest.fixtures {
        let source = root.join(&fixture.source);
        let package = match Package::open(&source) {
            Ok(package) => package,
            Err(error) => {
                eprintln!("{}: {error}", fixture.source);
                continue;
            }
        };
        let document = match import_document(&package) {
            Ok(document) => document,
            Err(error) => {
                eprintln!("{}: {error}", fixture.source);
                continue;
            }
        };
        let model_text = document.to_json_pretty().expect("serialize model");
        let model_path = golden.join(&fixture.model);
        let previous = fs::read_to_string(&model_path).unwrap_or_default();
        if previous != model_text {
            changed += 1;
            fs::write(&model_path, &model_text).expect("write model");
            println!("updated {}", fixture.model);
        }
        let value: Value = serde_json::from_str(&model_text).expect("parse model");
        fixture.model_file_sha256 = sha256(model_text.as_bytes());
        fixture.model_sha256 = canonical_hash(&value);
        fixture.document_schema_version = document.schema_version;
        fixture.counts.pages = document.pages.len();
        fixture.counts.shapes = document.pages.iter().map(|page| page.shapes.len()).sum();
        fixture.counts.connectors = document
            .pages
            .iter()
            .map(|page| page.connectors.len())
            .sum();
        fixture.counts.media = document.media.len();
    }
    manifest.generated_by =
        "layoutcraft-vsdx refresh-golden-models (Rust baseline 2026-10-29)".to_string();
    let mut out = serde_json::to_string_pretty(&manifest).expect("serialize manifest");
    out.push('\n');
    fs::write(&manifest_path, out).expect("write manifest");
    println!("refreshed {changed} model file(s)");
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_hash(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    sha256(out.as_bytes())
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                out.push_str(&value.to_string());
            } else if let Some(value) = number.as_u64() {
                out.push_str(&value.to_string());
            } else if let Some(value) = number.as_f64() {
                out.push_str(&js_number(value));
            }
        }
        Value::String(value) => {
            out.push_str(&serde_json::to_string(value).expect("quote string"));
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("quote key"));
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
    }
}

/// Format an f64 the way ECMAScript `JSON.stringify` does.
fn js_number(x: f64) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    let negative = x < 0.0;
    let abs = x.abs();
    let raw = serde_json::to_string(&abs).unwrap_or_else(|_| abs.to_string());
    let (mantissa, exp) = match raw.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse::<i32>().unwrap_or(0)),
        None => (raw.as_str(), 0),
    };
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let frac = mantissa
        .split_once('.')
        .map(|(_, fraction)| fraction.len() as i32)
        .unwrap_or(0);
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let point = digits.len() as i32 + exp - frac;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    let n = digits.len() as i32;
    if (1e-6..1e21).contains(&abs) {
        if point <= 0 {
            out.push_str("0.");
            for _ in 0..(-point) {
                out.push('0');
            }
            out.push_str(&digits);
        } else if point >= n {
            out.push_str(&digits);
            for _ in 0..(point - n) {
                out.push('0');
            }
        } else {
            out.push_str(&digits[..point as usize]);
            out.push('.');
            out.push_str(&digits[point as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let exponent = point - 1;
        out.push('e');
        if exponent >= 0 {
            out.push('+');
        }
        out.push_str(&exponent.to_string());
    }
    out
}
