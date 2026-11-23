//! The Rust Excalidraw import against the web's `importExcalidraw` (`visio-test-harness/dump-excalidraw-import-oracle.mjs` wrote `oracle/excalidraw-import-oracle.json`):
//! the same scenes (hand made ones and round trips of example drawings) give the same summary and the same drawing (numbers compared to 10 significant digits).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::excalidraw_import::parse_scene;
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

/// The drawing summary of the oracle script, with the keys of the web's JSON (`undefined` fields are left out like JSON.stringify does).
fn summary(engine: &Engine) -> Value {
    let size: Value = serde_json::from_str(&engine.page_size_json(0).unwrap()).unwrap();
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
        .map(|p| {
            let shapes: Vec<Value> = p["shapes"].as_array().unwrap().iter().map(|s| pick(s, &["visioId", "type", "x", "y", "w", "h", "rot", "fill", "stroke", "strokeWidth", "fillOpacity", "label", "bold", "fontSize", "fontFamily", "textColor", "strokeStyle", "rounding"])).collect();
            let connectors: Vec<Value> = p["connectors"]
                .as_array()
                .map_or_else(Vec::new, |cs| {
                    cs.iter()
                        .map(|c| {
                            let mut m = Map::new();
                            for k in ["visioId", "arrowStart", "arrowEnd", "stroke", "strokeWidth"] {
                                if let Some(x) = c.get(k) {
                                    m.insert(k.to_string(), x.clone());
                                }
                            }
                            for (k, side) in [("from", "from"), ("to", "to")] {
                                // JS `c.from && c.from.shapeId`: null when the end is null, left out when it is missing or has no shapeId
                                match c.get(side) {
                                    Some(Value::Null) => {
                                        m.insert(k.to_string(), Value::Null);
                                    }
                                    Some(end) => {
                                        if let Some(id) = end.get("shapeId") {
                                            m.insert(k.to_string(), id.clone());
                                        }
                                    }
                                    None => {}
                                }
                            }
                            Value::Object(m)
                        })
                        .collect()
                });
            json!({"name": p["name"], "shapes": shapes, "connectors": connectors})
        })
        .collect();
    json!({"size": size, "pages": pages})
}

#[test]
fn the_rust_import_makes_the_drawings_of_the_web_import() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/excalidraw-import-oracle.json")).expect("run node visio-test-harness/dump-excalidraw-import-oracle.mjs");
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/excalidraw");
    let (mut scenes, mut shapes, mut connectors) = (0, 0, 0);
    for file in oracle["files"].as_array().unwrap() {
        let name = file["name"].as_str().unwrap();
        let input = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
        let mut engine = Engine::new();
        let result = engine.import_excalidraw(&input);
        if let Some(want) = file["error"].as_str() {
            assert_eq!(result.unwrap_err(), want, "{name}: the error");
            continue;
        }
        let got = result.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(got.page as u64, file["page"].as_u64().unwrap(), "{name}: page");
        assert_eq!(json!(got.shapes), file["shapes"], "{name}: the shape ids");
        assert_eq!(got.connectors as u64, file["connectors"].as_u64().unwrap(), "{name}: connectors");
        assert_eq!(json!(got.skipped), file["skipped"], "{name}: skipped");
        assert_eq!(json!(got.map), file["map"], "{name}: map");
        assert_eq!(round(&summary(&engine)), round(&file["drawing"]), "{name}: the drawing");
        scenes += 1;
        shapes += got.shapes.len();
        connectors += got.connectors;
    }
    assert!(scenes >= 7 && shapes > 50 && connectors > 5, "{scenes} scenes, {shapes} shapes, {connectors} connectors were compared");
}

#[test]
fn bad_input_gives_the_messages_of_the_web_import() {
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/excalidraw-import-oracle.json")).unwrap()).unwrap();
    for e in oracle["errors"].as_array().unwrap() {
        let text = e["text"].as_str().unwrap();
        let got = parse_scene(text).err();
        assert_eq!(got.as_deref(), e["error"].as_str(), "input {text:?}");
    }
}
