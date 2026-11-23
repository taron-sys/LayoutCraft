//! The Rust draw.io import against the web's `importDrawio` (`visio-test-harness/dump-drawio-import-oracle.mjs` wrote `oracle/drawio-import-oracle.json`): the same files (hand made,
//! round trips of example drawings, a compressed copy) give the same summary and the same drawing (numbers compared to 10 significant digits).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::Engine;
use serde_json::{json, Map, Value};

fn r10(x: f64) -> f64 {
    if x.is_finite() { format!("{x:.9e}").parse().unwrap() } else { x }
}

fn round(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(r10(n.as_f64().unwrap())),
        Value::Array(a) => Value::Array(a.iter().map(round).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), round(v))).collect()),
        other => other.clone(),
    }
}

/// The drawing summary of the oracle script (keys the web's JSON.stringify would leave out because they are undefined are left out).
fn summary(engine: &Engine) -> Value {
    let model: Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
    let pick = |v: &Value, keys: &[&str]| -> Value {
        let mut m = Map::new();
        for k in keys {
            if let Some(x) = v.get(*k) {
                m.insert((*k).to_string(), x.clone());
            }
        }
        Value::Object(m)
    };
    let pages: Vec<Value> = model["pages"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let size: Value = serde_json::from_str(&engine.page_size_json(i).unwrap()).unwrap();
            let layers: Vec<Value> = p["layers"].as_array().map_or_else(Vec::new, |ls| ls.iter().map(|l| pick(l, &["name", "visible", "locked"])).collect());
            let shapes: Vec<Value> = p["shapes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    let mut v = pick(s, &["visioId", "type", "x", "y", "w", "h", "rot", "fill", "stroke", "strokeWidth", "fillOpacity", "label", "bold", "italic", "underline", "fontSize", "fontFamily", "textColor", "strokeStyle", "rounding", "layers"]);
                    if let Some(g) = s.get("group").filter(|g| !g.is_null()) {
                        v["group"] = pick(g, &["kind", "depth"]);
                    }
                    v
                })
                .collect();
            let connectors: Vec<Value> = p["connectors"]
                .as_array()
                .map_or_else(Vec::new, |cs| {
                    cs.iter()
                        .map(|c| {
                            let mut m = Map::new();
                            for k in ["visioId", "arrowStart", "arrowEnd", "stroke", "strokeWidth", "strokeStyle"] {
                                if let Some(x) = c.get(k) {
                                    m.insert(k.to_string(), x.clone());
                                }
                            }
                            for side in ["from", "to"] {
                                match c.get(side) {
                                    Some(Value::Null) => {
                                        m.insert(side.to_string(), Value::Null);
                                    }
                                    Some(end) => {
                                        if let Some(id) = end.get("shapeId") {
                                            m.insert(side.to_string(), id.clone());
                                        }
                                    }
                                    None => {}
                                }
                            }
                            Value::Object(m)
                        })
                        .collect()
                });
            json!({"name": p["name"], "size": size, "layers": layers, "shapes": shapes, "connectors": connectors})
        })
        .collect();
    json!({"pages": pages})
}

fn oracle() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/drawio-import-oracle.json")).expect("run node visio-test-harness/dump-drawio-import-oracle.mjs");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_rust_import_makes_the_drawings_of_the_web_import() {
    let oracle = oracle();
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/drawio");
    let (mut files, mut shapes, mut connectors) = (0, 0, 0);
    for file in oracle["files"].as_array().unwrap() {
        let name = file["name"].as_str().unwrap();
        let input = std::fs::read(format!("{dir}/{name}")).unwrap();
        let mut engine = Engine::new();
        let result = engine.import_drawio(&input);
        if let Some(want) = file["error"].as_str() {
            assert_eq!(result.unwrap_err(), want, "{name}: the error");
            continue;
        }
        let got = result.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(got.pages as u64, file["pages"].as_u64().unwrap(), "{name}: pages");
        assert_eq!(got.shapes as u64, file["shapes"].as_u64().unwrap(), "{name}: shapes");
        assert_eq!(got.connectors as u64, file["connectors"].as_u64().unwrap(), "{name}: connectors");
        assert_eq!(json!(got.approximated), file["approximated"], "{name}: approximated");
        assert_eq!(json!(got.skipped), file["skipped"], "{name}: skipped");
        assert_eq!(json!(got.map), file["map"], "{name}: map");
        assert_eq!(round(&summary(&engine)), round(&file["drawing"]), "{name}: the drawing");
        files += 1;
        shapes += got.shapes;
        connectors += got.connectors;
    }
    assert!(files >= 7 && shapes > 300 && connectors > 4, "{files} files, {shapes} shapes, {connectors} connectors were compared");
}

#[test]
fn bad_input_gives_the_messages_and_drawings_of_the_web_import() {
    for e in oracle()["errors"].as_array().unwrap() {
        let text = e["text"].as_str().unwrap();
        let mut engine = Engine::new();
        let result = engine.import_drawio(text.as_bytes());
        match e["error"].as_str() {
            Some(want) => assert_eq!(result.unwrap_err(), want, "input {text:?}"),
            None => {
                let got = result.unwrap_or_else(|err| panic!("input {text:?}: {err}"));
                assert_eq!(got.pages as u64, e["pages"].as_u64().unwrap(), "input {text:?}: pages");
                assert_eq!(round(&summary(&engine)), round(&e["drawing"]), "input {text:?}: the drawing");
            }
        }
    }
}
