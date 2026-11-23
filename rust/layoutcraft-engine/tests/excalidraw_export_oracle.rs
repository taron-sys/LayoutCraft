//! The Rust Excalidraw export against the web's `exportExcalidraw` (`visio-test-harness/dump-excalidraw-export-oracle.mjs` wrote `oracle/excalidraw-export-oracle.json`):
//! the scenes are compared in a canonical form (keys sorted, numbers as JavaScript writes them) by length and a 64 bit FNV-1a hash, and as whole text for small ones.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::svgutil::js_string;
use layoutcraft_engine::Engine;
use serde_json::Value;

fn canon(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        // 10 significant digits, as the oracle script writes them (the native and the wasm engine differ in the last bit of some results)
        Value::Number(n) => {
            let x = n.as_f64().unwrap();
            js_string(if x.is_finite() { format!("{x:.9e}").parse::<f64>().unwrap() } else { x })
        }
        Value::String(s) => serde_json::to_string(s).unwrap(),
        Value::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            format!("{{{}}}", keys.iter().map(|k| format!("{}:{}", serde_json::to_string(k).unwrap(), canon(&o[*k]))).collect::<Vec<_>>().join(","))
        }
    }
}

fn fnv1a(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[test]
fn the_rust_excalidraw_export_gives_the_scene_of_the_web_export_on_every_page() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/excalidraw-export-oracle.json")).expect("run node visio-test-harness/dump-excalidraw-export-oracle.mjs");
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut engine = Engine::new();
    let mut open = String::new();
    let (mut pages, mut elements, mut pictures, mut whole) = (0, 0, 0, 0);
    for entry in oracle["pages"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let page = entry["page"].as_u64().unwrap() as usize;
        if open != path {
            engine = Engine::new();
            engine.open(std::fs::read(format!("{root}/{path}")).unwrap()).unwrap();
            open = path.to_string();
        }
        let at = format!("{path} page {page}");
        let scene = engine.export_excalidraw(page).unwrap_or_else(|e| panic!("{at}: {e}"));
        assert!(entry["error"].is_null(), "{at}: the web export failed: {}", entry["error"]);
        let text = canon(&scene);
        assert_eq!(scene["elements"].as_array().unwrap().len() as u64, entry["elements"].as_u64().unwrap(), "{at}: element count");
        assert_eq!(scene["files"].as_object().unwrap().len() as u64, entry["files"].as_u64().unwrap(), "{at}: pictures");
        if let Some(want) = entry["canon"].as_str() {
            assert_eq!(text, want, "{at}: the scene");
            whole += 1;
        }
        assert_eq!(text.encode_utf16().count() as u64, entry["length"].as_u64().unwrap(), "{at}: length");
        assert_eq!(fnv1a(&text), entry["hash"].as_str().unwrap(), "{at}: the scene (hash)");
        pages += 1;
        elements += scene["elements"].as_array().unwrap().len();
        pictures += scene["files"].as_object().unwrap().len();
    }
    assert!(pages >= 600 && elements > 5000 && whole >= 3, "{pages} pages, {elements} elements, {pictures} pictures, {whole} compared as whole text");
}
