//! The Rust Compare against the web's `compareModels` (`visio-test-harness/dump-compare-oracle.mjs` wrote `oracle/compare-oracle.json`): the same pairs
//! of drawings (a drawing against itself after the stored edit recipe, against itself, and neighbouring drawings) give the same differences and the same CSV.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::compare::{compare_models, diffs_to_csv, Diff};
use layoutcraft_engine::Engine;
use serde_json::Value;

fn model_of(root: &str, rel: &str) -> Value {
    let mut engine = Engine::new();
    engine.open(std::fs::read(format!("{root}/{rel}")).unwrap()).unwrap_or_else(|e| panic!("{rel}: {e}"));
    serde_json::from_str(&engine.json().unwrap()).unwrap()
}

/// The same edit recipe as `applyRecipe` in dump-compare-oracle.mjs.
fn apply_recipe(model: &Value, recipe: &[Value]) -> Value {
    let mut m = model.clone();
    for r in recipe {
        let op = r["op"].as_str().unwrap();
        let (si, ci) = (r["shape"].as_u64().map(|n| n as usize), r["conn"].as_u64().map(|n| n as usize));
        let page = &mut m["pages"][0];
        let has_shape = si.is_some_and(|i| page["shapes"].get(i).is_some());
        let has_conn = ci.is_some_and(|i| page["connectors"].get(i).is_some());
        match op {
            "labelSuffix" if has_shape => {
                let s = &mut page["shapes"][si.unwrap()];
                s["label"] = Value::String(format!("{}{}", s["label"].as_str().unwrap_or(""), r["suffix"].as_str().unwrap()));
            }
            "addX" if has_shape => {
                let s = &mut page["shapes"][si.unwrap()];
                s["x"] = Value::from(s["x"].as_f64().unwrap() + r["dx"].as_f64().unwrap());
            }
            "scaleW" if has_shape => {
                let s = &mut page["shapes"][si.unwrap()];
                s["w"] = Value::from(s["w"].as_f64().unwrap() * r["factor"].as_f64().unwrap());
            }
            "setField" if has_shape => page["shapes"][si.unwrap()][r["key"].as_str().unwrap()] = r["value"].clone(),
            "addRot" if has_shape => {
                let s = &mut page["shapes"][si.unwrap()];
                s["rot"] = Value::from(s["rot"].as_f64().unwrap_or(0.0) + r["d"].as_f64().unwrap());
            }
            "deleteShape" if has_shape => {
                page["shapes"].as_array_mut().unwrap().remove(si.unwrap());
            }
            "cloneShape" if has_shape => {
                let mut s = page["shapes"][si.unwrap()].clone();
                s["visioId"] = r["id"].clone();
                page["shapes"].as_array_mut().unwrap().push(s);
            }
            "setConnField" if has_conn => page["connectors"][ci.unwrap()][r["key"].as_str().unwrap()] = r["value"].clone(),
            "setConnTo" if has_conn => {
                let c = &mut page["connectors"][ci.unwrap()];
                if !c["to"].is_object() {
                    c["to"] = serde_json::json!({});
                }
                c["to"]["shapeId"] = Value::String(format!("shape:visio:{}", r["id"].as_str().unwrap()));
            }
            "deleteConnector" if has_conn => {
                page["connectors"].as_array_mut().unwrap().remove(ci.unwrap());
            }
            "renamePage" => {
                let i = r["page"].as_u64().unwrap() as usize;
                if m["pages"].get(i).is_some() {
                    let name = format!("{}{}", m["pages"][i]["name"].as_str().unwrap_or(""), r["suffix"].as_str().unwrap());
                    m["pages"][i]["name"] = Value::String(name);
                }
            }
            _ => {}
        }
    }
    m
}

fn tuple(d: &Diff) -> (i64, String, String, String, String, String, String) {
    (d.page, d.page_name.clone(), d.kind.clone(), d.what.clone(), d.id.clone(), d.label.clone(), d.detail.clone())
}

fn expected(list: &Value) -> Vec<(i64, String, String, String, String, String, String)> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|d| {
            let s = |k: &str| d[k].as_str().unwrap().to_string();
            (d["page"].as_i64().unwrap(), s("pageName"), s("kind"), s("what"), s("id"), s("label"), s("detail"))
        })
        .collect()
}

#[test]
fn the_rust_compare_gives_the_differences_of_the_web_compare() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/compare-oracle.json")).expect("run node visio-test-harness/dump-compare-oracle.mjs");
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let recipe = oracle["recipe"].as_array().unwrap().clone();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut cache: std::collections::HashMap<String, Value> = Default::default();
    let (mut pairs, mut diffs_total, mut edited_with_diffs) = (0, 0, 0);
    for pair in oracle["pairs"].as_array().unwrap() {
        let (a, b) = (pair["current"].as_str().unwrap(), pair["other"].as_str().unwrap());
        for p in [a, b] {
            cache.entry(p.to_string()).or_insert_with(|| model_of(root, p));
        }
        let kind = pair["kind"].as_str().unwrap();
        let current = if kind == "edited" { apply_recipe(&cache[a], &recipe) } else { cache[a].clone() };
        let got = compare_models(&current, &cache[b]);
        assert_eq!(got.iter().map(tuple).collect::<Vec<_>>(), expected(&pair["diffs"]), "{kind}: {a} against {b}");
        assert_eq!(diffs_to_csv(&got), pair["csv"].as_str().unwrap(), "{kind}: {a} against {b}: CSV");
        pairs += 1;
        diffs_total += got.len();
        if kind == "same" {
            assert!(got.is_empty(), "{a} differs from itself");
        }
        if kind == "edited" && !got.is_empty() {
            edited_with_diffs += 1;
        }
    }
    assert!(pairs >= 300 && diffs_total > 1000 && edited_with_diffs > 100, "{pairs} pairs, {diffs_total} differences, {edited_with_diffs} edited drawings with differences");
}
