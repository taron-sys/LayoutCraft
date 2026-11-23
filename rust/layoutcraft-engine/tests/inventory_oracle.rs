//! The Rust inventory against the web's `inventoryData` / `inventoryCsv` (`visio-test-harness/dump-inventory-oracle.mjs` wrote `oracle/inventory-oracle.json`).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::inventory::{inventory_csv, inventory_data, DataCol, Row};
use layoutcraft_engine::Engine;
use serde_json::{json, Value};

fn rows_json(rows: &[Row], cols: &[DataCol]) -> Value {
    json!({
        // JSON.stringify of the web's rows leaves out an `id` that is undefined (a shape without Visio id)
        "rows": rows.iter().map(|r| { let mut row = json!({"page": r.page, "text": r.text, "values": r.values}); if !r.id.is_null() { row["id"] = r.id.clone(); } row }).collect::<Vec<_>>(),
        "dataCols": cols.iter().map(|c| json!({"key": c.key, "label": c.label})).collect::<Vec<_>>(),
    })
}

#[test]
fn the_rust_inventory_gives_the_rows_and_csv_of_the_web_inventory() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle/inventory-oracle.json")).expect("run node visio-test-harness/dump-inventory-oracle.mjs");
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let (mut drawings, mut rows_total, mut with_data) = (0, 0, 0);
    for file in oracle["files"].as_array().unwrap() {
        let path = file["path"].as_str().unwrap();
        let mut engine = Engine::new();
        engine.open(std::fs::read(format!("{root}/{path}")).unwrap()).unwrap();
        let model: Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        let names: Vec<String> = model["pages"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap_or("").to_string()).collect();
        let (rows, cols) = inventory_data(&model, &names, 0, true);
        assert_eq!(rows_json(&rows, &cols), file["all"], "{path}: all pages");
        let (rows0, cols0) = inventory_data(&model, &names, 0, false);
        assert_eq!(rows_json(&rows0, &cols0), file["page0"], "{path}: page 0");
        let columns: Vec<String> = file["columns"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect();
        assert_eq!(inventory_csv(&model, &names, 0, &columns, true), file["csvAll"].as_str().unwrap(), "{path}: CSV all");
        assert_eq!(inventory_csv(&model, &names, 0, &columns[1..], false), file["csvPage0"].as_str().unwrap(), "{path}: CSV page 0");
        drawings += 1;
        rows_total += rows.len();
        if !cols.is_empty() {
            with_data += 1;
        }
    }
    assert!(drawings >= 100 && rows_total > 1000 && with_data > 5, "{drawings} drawings, {rows_total} rows, {with_data} with Shape Data");
}
