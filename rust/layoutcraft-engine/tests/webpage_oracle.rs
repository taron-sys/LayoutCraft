//! The Rust web page export against the web's `buildWebPage` (`visio-test-harness/dump-webpage-oracle.mjs` wrote `oracle/webpage-oracle.json`).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::webpage::safe_href;
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

fn oracle() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/webpage-oracle.json")).expect("run node visio-test-harness/dump-webpage-oracle.mjs");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn safe_href_decides_like_the_web() {
    for h in oracle()["hrefs"].as_array().unwrap() {
        let address = h["address"].as_str().unwrap();
        assert_eq!(safe_href(address).as_deref(), h["href"].as_str(), "address {address:?}");
    }
}

#[test]
fn the_rust_web_page_is_the_web_page_of_the_web_export_on_every_drawing() {
    let oracle = oracle();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let (mut files, mut links, mut dropped, mut whole) = (0, 0, 0, 0);
    for entry in oracle["files"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let mut engine = Engine::new();
        engine.open(std::fs::read(format!("{root}/{path}")).unwrap()).unwrap();
        let made = engine.export_webpage().unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!((made.links as u64, made.dropped as u64), (entry["linkCount"].as_u64().unwrap(), entry["dropped"].as_u64().unwrap()), "{path}: links");
        if let Some(want) = entry["html"].as_str() {
            assert_eq!(made.html, want, "{path}: the HTML text");
            whole += 1;
        }
        assert_eq!(made.html.encode_utf16().count() as u64, entry["length"].as_u64().unwrap(), "{path}: length");
        assert_eq!(fnv1a(&made.html), entry["hash"].as_str().unwrap(), "{path}: the HTML text (hash)");
        files += 1;
        links += made.links;
        dropped += made.dropped;
    }
    assert!(files >= 100 && whole >= 3, "{files} files, {links} links, {dropped} dropped, {whole} compared as whole text");
}
