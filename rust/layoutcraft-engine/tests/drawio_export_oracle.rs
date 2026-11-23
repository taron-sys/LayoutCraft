//! The Rust draw.io export against the web's `exportDrawio` (`visio-test-harness/dump-drawio-export-oracle.mjs` wrote `oracle/drawio-export-oracle.json`): the same text
//! (compared by length and a 64 bit FNV-1a hash, and as whole text for small files) on every example and corpus drawing.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::Engine;
use serde_json::Value;

fn fnv1a(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[test]
fn the_rust_drawio_export_gives_the_file_of_the_web_export_on_every_drawing() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/drawio-export-oracle.json")).expect("run node visio-test-harness/dump-drawio-export-oracle.mjs");
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let (mut files, mut cells, mut whole, mut errors) = (0, 0, 0, 0);
    for entry in oracle["files"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let mut engine = Engine::new();
        engine.open(std::fs::read(format!("{root}/{path}")).unwrap()).unwrap();
        let made = engine.export_drawio();
        if entry["error"].is_string() {
            assert!(made.is_err(), "{path}: the web export fails ({}), the Rust export does not", entry["error"]);
            errors += 1;
            continue;
        }
        let xml = made.unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(xml.matches("<mxCell ").count() as u64, entry["cells"].as_u64().unwrap(), "{path}: cell count");
        if let Some(want) = entry["xml"].as_str() {
            assert_eq!(xml, want, "{path}: the XML");
            whole += 1;
        }
        assert_eq!(xml.encode_utf16().count() as u64, entry["length"].as_u64().unwrap(), "{path}: length");
        assert_eq!(fnv1a(&xml), entry["hash"].as_str().unwrap(), "{path}: the XML (hash)");
        files += 1;
        cells += xml.matches("<mxCell ").count();
    }
    assert!(files >= 100 && cells > 3000 && whole >= 3, "{files} files, {cells} cells, {whole} compared as whole text, {errors} drawings where both exports fail");
}
