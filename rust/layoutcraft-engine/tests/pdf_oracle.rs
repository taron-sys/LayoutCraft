//! The Rust PDF writer against the web's `buildPdf` (`visio-test-harness/dump-pdf-oracle.mjs` wrote `oracle/pdf-oracle.json`): the PDFs of the drawings whose pages are all vector pages
//! are byte for byte the PDFs the web writes (length, hash, whole bytes of small files). Drawings with a picture page are checked for structure only.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::Engine;
use serde_json::Value;

fn fnv1a(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[test]
fn the_rust_pdf_is_the_pdf_of_the_web_for_every_drawing_with_vector_pages_only() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/pdf-oracle.json")).expect("run node visio-test-harness/dump-pdf-oracle.mjs");
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let (mut same, mut whole, mut pictures) = (0, 0, 0);
    for entry in oracle["files"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let mut engine = Engine::new();
        engine.open(std::fs::read(format!("{root}/{path}")).unwrap()).unwrap();
        let made = engine.export_pdf(2.0).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(made.pages as u64, entry["pages"].as_u64().unwrap(), "{path}: pages");
        if let Some(raster) = entry["raster"].as_u64() {
            assert_eq!((made.pages - made.vector_pages) as u64, raster, "{path}: the pages that become pictures");
            assert!(made.bytes.starts_with(b"%PDF-1.4\n") && made.bytes.ends_with(b"%%EOF\n"), "{path}: structure");
            assert!(made.bytes.windows(11).any(|w| w == b"FlateDecode"), "{path}: a picture page has a Flate image");
            pictures += 1;
            continue;
        }
        assert_eq!(made.vector_pages, made.pages, "{path}: all pages are vector pages in the web");
        assert_eq!(made.bytes.len() as u64, entry["length"].as_u64().unwrap(), "{path}: length");
        if let Some(latin1) = entry["latin1"].as_str() {
            let want: Vec<u8> = latin1.chars().map(|c| c as u32 as u8).collect();
            assert_eq!(made.bytes, want, "{path}: the bytes");
            whole += 1;
        }
        assert_eq!(fnv1a(&made.bytes), entry["hash"].as_str().unwrap(), "{path}: the bytes (hash)");
        same += 1;
    }
    assert!(same >= 30 && whole >= 3, "{same} PDFs equal, {whole} compared byte for byte, {pictures} with picture pages");
}
