//! The Rust DXF export against the web's `svgToDxf` (`visio-test-harness/dump-dxf-oracle.mjs` wrote `oracle/dxf-oracle.json`): every page of every example and corpus drawing
//! gives the same counts, extents, layers and the same DXF text (compared by length and a 64 bit FNV-1a hash, and as whole text for the small ones).
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
fn the_rust_dxf_export_gives_the_dxf_of_the_web_export_on_every_page() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/dxf-oracle.json")).expect("run node visio-test-harness/dump-dxf-oracle.mjs");
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut engine = Engine::new();
    let mut open = String::new();
    let (mut pages, mut polylines, mut whole) = (0, 0, 0);
    for entry in oracle["pages"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let page = entry["page"].as_u64().unwrap() as usize;
        if open != path {
            engine = Engine::new();
            engine.open(std::fs::read(format!("{root}/{path}")).unwrap()).unwrap();
            open = path.to_string();
        }
        let made = engine.export_dxf(page).unwrap_or_else(|e| panic!("{path} page {page}: {e}"));
        let at = format!("{path} page {page}");
        assert_eq!(made.polylines as u64, entry["polylines"].as_u64().unwrap(), "{at}: polylines");
        assert_eq!(made.circles as u64, entry["circles"].as_u64().unwrap(), "{at}: circles");
        assert_eq!(made.texts as u64, entry["texts"].as_u64().unwrap(), "{at}: texts");
        let layers: Vec<&str> = entry["layers"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
        assert_eq!(made.layers.iter().map(String::as_str).collect::<Vec<_>>(), layers, "{at}: layers");
        let extents: Vec<f64> = entry["extents"].as_array().unwrap().iter().map(|e| e.as_f64().unwrap_or(f64::NAN)).collect();
        for (k, (got, want)) in made.extents.iter().zip(&extents).enumerate() {
            assert!(got == want || (got.is_nan() && want.is_nan()) || (got - want).abs() < 1e-9, "{at}: extent {k}: {got} against {want}");
        }
        if let Some(want) = entry["dxf"].as_str() {
            assert_eq!(made.dxf, want, "{at}: the DXF text");
            whole += 1;
        }
        // the length counts UTF-16 units in JavaScript; the DXF is ASCII, so it is the byte length
        assert_eq!(made.dxf.len() as u64, entry["length"].as_u64().unwrap(), "{at}: length");
        assert_eq!(fnv1a(&made.dxf), entry["hash"].as_str().unwrap(), "{at}: the DXF text (hash)");
        pages += 1;
        polylines += made.polylines;
    }
    assert!(pages >= 600 && polylines > 10_000 && whole >= 5, "{pages} pages, {polylines} polylines, {whole} compared as whole text");
}
