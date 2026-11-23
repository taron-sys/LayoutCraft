//! The Rust PDF page converter against the web's `svgToPdfPage` (`visio-test-harness/dump-pdfpage-oracle.mjs` wrote `oracle/pdfpage-oracle.json`): on every page of every example and
//! corpus drawing the same content stream (length, hash, whole text of small ones), fonts, graphics states and patterns, or the same `Unsupported` message.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::Engine;
use serde_json::{json, Value};

fn fnv1a(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[test]
fn the_rust_pdf_converter_gives_the_content_of_the_web_converter_on_every_page() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/pdfpage-oracle.json")).expect("run node visio-test-harness/dump-pdfpage-oracle.mjs");
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut engine = Engine::new();
    let mut open = String::new();
    let (mut pages, mut converted, mut unsupported, mut whole) = (0, 0, 0, 0);
    for entry in oracle["pages"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let page = entry["page"].as_u64().unwrap() as usize;
        if open != path {
            engine = Engine::new();
            engine.open(std::fs::read(format!("{root}/{path}")).unwrap()).unwrap();
            open = path.to_string();
        }
        let at = format!("{path} page {page}");
        let made = engine.pdf_page(page).unwrap_or_else(|e| panic!("{at}: {e}"));
        pages += 1;
        if let Some(want) = entry["unsupported"].as_str() {
            assert_eq!(made.unwrap_err().0, want, "{at}: the Unsupported message");
            unsupported += 1;
            continue;
        }
        let got = made.unwrap_or_else(|e| panic!("{at}: unsupported here ({}) but not in the web", e.0));
        assert_eq!(got.width_pt, entry["widthPt"].as_f64().unwrap(), "{at}: width");
        assert_eq!(got.height_pt, entry["heightPt"].as_f64().unwrap(), "{at}: height");
        assert_eq!(json!(got.fonts), entry["fonts"], "{at}: fonts");
        assert_eq!(json!(got.gstates), entry["gstates"], "{at}: graphics states");
        assert_eq!(json!(got.patterns), entry["patterns"], "{at}: patterns");
        if let Some(want) = entry["content"].as_str() {
            assert_eq!(got.content, want, "{at}: the content stream");
            whole += 1;
        }
        assert_eq!(got.content.encode_utf16().count() as u64, entry["length"].as_u64().unwrap(), "{at}: length");
        assert_eq!(fnv1a(&got.content), entry["hash"].as_str().unwrap(), "{at}: the content stream (hash)");
        converted += 1;
    }
    assert!(pages >= 600 && converted > 100 && whole >= 3, "{pages} pages, {converted} converted, {unsupported} unsupported on both sides, {whole} compared as whole text");
}
