//! The Rust Check Diagram against the web's `checkDiagram` (`visio-test-harness/dump-check-oracle.mjs` wrote `oracle/check-oracle.json` from
//! web/checkdiagram.js): the same drawings, opened by the Rust engine, give the same issues in the same order with the same texts.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::check::{check_page, issues_to_csv, Issue};
use layoutcraft_engine::Engine;
use serde_json::Value;

fn oracle() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/check-oracle.json")).expect("run node visio-test-harness/dump-check-oracle.mjs");
    serde_json::from_str(&text).unwrap()
}

fn expected(list: &Value) -> Vec<(String, String, String, Option<String>)> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|i| (i["kind"].as_str().unwrap().to_string(), i["id"].as_str().unwrap().to_string(), i["message"].as_str().unwrap().to_string(), i["fix"].as_str().map(str::to_string)))
        .collect()
}

fn got(issues: &[Issue]) -> Vec<(String, String, String, Option<String>)> {
    issues.iter().map(|i| (i.kind.clone(), i.id.clone(), i.message.clone(), i.fix.clone())).collect()
}

#[test]
fn the_rust_check_gives_the_issues_of_the_web_check_on_every_drawing() {
    let oracle = oracle();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let (mut drawings, mut pages, mut issues) = (0, 0, 0);
    for file in oracle["files"].as_array().unwrap() {
        let path = format!("{root}/{}", file["path"].as_str().unwrap());
        let mut engine = Engine::new();
        engine.open(std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{path}: {e}"));
        let model: Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        drawings += 1;
        let mut all = Vec::new();
        for page in file["pages"].as_array().unwrap() {
            let index = page["index"].as_u64().unwrap() as usize;
            let size = engine.page_size_json(index).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|s| (s["w"].as_f64().unwrap(), s["h"].as_f64().unwrap()));
            assert_eq!(size, Some((page["size"]["w"].as_f64().unwrap(), page["size"]["h"].as_f64().unwrap())), "{path} page {index}: the size");
            let plain = check_page(&model, index, size, false);
            assert_eq!(got(&plain), expected(&page["issues"]), "{path} page {index}");
            assert_eq!(got(&check_page(&model, index, size, true)), expected(&page["issuesAlt"]), "{path} page {index} with alt text");
            pages += 1;
            issues += plain.len();
            all.extend(plain.into_iter().map(|mut i| {
                i.page = Some(index);
                i
            }));
        }
        let names: Vec<String> = model["pages"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap_or("").to_string()).collect();
        assert_eq!(issues_to_csv(&all, &names), file["csv"].as_str().unwrap(), "{path}: the CSV");
        // Engine::check_diagram is the same check on every page
        assert_eq!(engine.check_diagram(false).unwrap().len(), all.len(), "{path}");
    }
    assert!(drawings >= 100 && pages >= drawings && issues > 100, "{drawings} drawings, {pages} pages, {issues} issues were compared");
}
