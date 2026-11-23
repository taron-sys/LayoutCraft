//! The Rust borders against the web's `borderShapes` / `applyBorder` / `removeBorder` (`visio-test-harness/dump-borders-oracle.mjs` wrote `oracle/borders-oracle.json`).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::borders::{border_shapes, BORDER_STYLES};
use layoutcraft_engine::Engine;
use serde_json::{json, Value};

/// JSON numbers compare by value (the web writes 1, Rust 1.0).
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        other => other.clone(),
    }
}

fn oracle() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/borders-oracle.json")).expect("run node visio-test-harness/dump-borders-oracle.mjs");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_border_designs_are_those_of_the_web() {
    let oracle = oracle();
    // (serde_json sorts object keys, so the style NAMES are compared as sets; the order of the list is fixed by BORDER_STYLES)
    let mut names: Vec<&str> = oracle["styles"].as_object().unwrap().keys().map(String::as_str).collect();
    let mut ours: Vec<&str> = BORDER_STYLES.iter().map(|(n, _)| *n).collect();
    names.sort();
    ours.sort();
    assert_eq!(names, ours);
    for (name, tip) in BORDER_STYLES {
        assert_eq!(oracle["styles"][name].as_str(), Some(tip));
    }
    let designs = oracle["designs"].as_array().unwrap();
    assert_eq!(designs.len(), 48);
    for d in designs {
        let shapes = border_shapes(d["style"].as_str().unwrap(), d["w"].as_f64().unwrap(), d["h"].as_f64().unwrap(), d["title"].as_str().unwrap()).unwrap();
        let got: Vec<Value> = shapes
            .iter()
            .map(|s| {
                let mut v = json!({"kind": s.kind, "x0": s.x0, "y0": s.y0, "x1": s.x1, "y1": s.y1, "style": s.style});
                if let Some(t) = &s.text {
                    v["text"] = json!(t);
                }
                v
            })
            .collect();
        assert_eq!(norm(&Value::Array(got)), norm(&d["shapes"]), "{} {}x{} {:?}", d["style"], d["w"], d["h"], d["title"]);
    }
}

fn summary(model: &Value) -> Value {
    Value::Array(
        model["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let shapes: Vec<Value> = p["shapes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| {
                        let mut o = serde_json::Map::new();
                        for k in ["x", "y", "w", "h", "fill", "stroke", "strokeWidth", "label", "bold", "fontSize"] {
                            if let Some(v) = s.get(k).filter(|v| !v.is_null()) {
                                o.insert(k.to_string(), v.clone());
                            }
                        }
                        Value::Object(o)
                    })
                    .collect();
                json!({"name": p["name"], "shapes": shapes})
            })
            .collect(),
    )
}

#[test]
fn applying_and_removing_a_border_does_what_the_web_does() {
    let oracle = oracle();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let applied = oracle["applied"].as_array().unwrap();
    assert_eq!(applied.len(), 12);
    for a in applied {
        let file = a["file"].as_str().unwrap();
        let style = a["style"].as_str().unwrap();
        let mut engine = Engine::new();
        engine.open(std::fs::read(format!("{root}/{file}")).unwrap()).unwrap();
        let before_depth = engine.undo_depth();
        let id = engine.apply_border(0, style, a["title"].as_str().unwrap()).unwrap();
        assert_eq!(json!(id), a["id"].as_str().map(|s| json!(s)).unwrap_or(a["id"].clone()), "{file} {style}: the background id");
        assert_eq!(engine.undo_depth(), before_depth + 1, "{file} {style}: one undo step");
        let model: Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        assert_eq!(norm(&summary(&model)), norm(&a["pages"]).clone(), "{file} {style}: the pages");
        let info: Value = serde_json::from_str(&engine.background_info_json().unwrap()).unwrap();
        assert_eq!(info, a["background"], "{file} {style}: the background list");
        let size: Value = serde_json::from_str(&engine.page_size_json(model["pages"].as_array().unwrap().len() - 1).unwrap()).unwrap();
        assert_eq!(norm(&size), norm(&a["size"]), "{file} {style}: the border page size");
        assert!(engine.page_has_background(0));
        engine.remove_border(0).unwrap();
        let after: Value = serde_json::from_str(&engine.background_info_json().unwrap()).unwrap();
        assert_eq!(after, a["afterRemove"], "{file} {style}: after removing");
        assert!(!engine.page_has_background(0));
        assert!(engine.undo().unwrap() && engine.undo().unwrap(), "both steps undo");
        assert_eq!(engine.undo_depth(), before_depth);
    }
}
