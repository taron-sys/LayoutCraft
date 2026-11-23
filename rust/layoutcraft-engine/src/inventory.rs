//! Review > Reports: a Rust port of `web/inventory.js` (docs/34, step N4.3). Rows = the shapes of one page (or every page); columns = Page, Shape ID, Text and
//! each Shape Data label. `tests/inventory_oracle.rs` compares the result with what the web code gives on every example drawing.
use std::collections::HashMap;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub page: String,
    /// The shape's `visioId` as the model has it (string or number).
    pub id: Value,
    pub text: String,
    pub values: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataCol {
    pub key: String,
    pub label: String,
}

/// JavaScript `String(value)` of a model value (`null` / missing give "").
fn js_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// JavaScript white space (the `\s` class: white space and U+FEFF, without U+0085).
fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// `text.replace(/\s+/g, ' ').trim()`.
fn squash(text: &str) -> String {
    let mut out = String::new();
    let mut in_space = false;
    for c in text.chars() {
        if is_js_space(c) {
            in_space = true;
        } else {
            if in_space {
                out.push(' ');
            }
            in_space = false;
            out.push(c);
        }
    }
    if in_space {
        out.push(' ');
    }
    out.trim_matches(is_js_space).to_string()
}

/// The rows and the Shape Data columns of page `page_index` (every page with `all_pages`); `names` are the page names.
pub fn inventory_data(model: &Value, names: &[String], page_index: usize, all_pages: bool) -> (Vec<Row>, Vec<DataCol>) {
    let pages = model.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
    let (mut rows, mut cols): (Vec<Row>, Vec<DataCol>) = (Vec::new(), Vec::new());
    let indexes: Vec<usize> = if all_pages { (0..pages.len()).collect() } else { vec![page_index] };
    for pi in indexes {
        let Some(shapes) = pages.get(pi).and_then(|p| p.get("shapes")).and_then(Value::as_array) else {
            continue;
        };
        for shape in shapes {
            let mut values = HashMap::new();
            for row in shape.get("data").and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
                let name = js_string(row.get("name"));
                if !cols.iter().any(|c| c.key == name) {
                    let label = js_string(row.get("label"));
                    cols.push(DataCol { key: name.clone(), label: if label.is_empty() { name.clone() } else { label } });
                }
                values.insert(name, js_string(row.get("value")));
            }
            rows.push(Row {
                page: names.get(pi).cloned().unwrap_or_else(|| (pi + 1).to_string()),
                id: shape.get("visioId").cloned().unwrap_or(Value::Null),
                text: squash(&js_string(shape.get("label"))),
                values,
            });
        }
    }
    (rows, cols)
}

/// One cell: `page`, `id`, `text` or a Shape Data name.
pub fn cell(row: &Row, key: &str) -> String {
    match key {
        "page" => row.page.clone(),
        "id" => js_string(Some(&row.id)),
        "text" => row.text.clone(),
        other => row.values.get(other).cloned().unwrap_or_default(),
    }
}

/// The column heading of `key`.
pub fn label(cols: &[DataCol], key: &str) -> String {
    match key {
        "page" => "Page".to_string(),
        "id" => "Shape ID".to_string(),
        "text" => "Text".to_string(),
        other => cols.iter().find(|c| c.key == other).map_or_else(|| other.to_string(), |c| if c.label.is_empty() { other.to_string() } else { c.label.clone() }),
    }
}

/// CSV text (quotes doubled, fields with comma / quote / line break quoted, CRLF) for the chosen columns.
pub fn inventory_csv(model: &Value, names: &[String], page_index: usize, columns: &[String], all_pages: bool) -> String {
    let (rows, cols) = inventory_data(model, names, page_index, all_pages);
    fn q(v: &str) -> String {
        if v.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v.to_string() }
    }
    let mut lines = vec![columns.iter().map(|k| q(&label(&cols, k))).collect::<Vec<_>>().join(",")];
    for r in &rows {
        lines.push(columns.iter().map(|k| q(&cell(r, k))).collect::<Vec<_>>().join(","));
    }
    lines.join("\r\n") + "\r\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model() -> Value {
        json!({"pages": [
            {"shapes": [{"visioId": "1", "label": "  Web \n  server ", "data": [{"name": "Owner", "label": "Owner name", "value": "Ann"}, {"name": "Cost", "value": 5}]}, {"visioId": 2, "label": null}]},
            {"shapes": [{"visioId": "3", "label": "x,y", "data": [{"name": "Owner", "value": null}]}]}
        ]})
    }

    #[test]
    fn rows_text_is_squashed_and_columns_come_in_first_seen_order() {
        let names = vec!["One".to_string()];
        let (rows, cols) = inventory_data(&model(), &names, 0, true);
        assert_eq!(rows.iter().map(|r| (r.page.as_str(), r.text.as_str())).collect::<Vec<_>>(), [("One", "Web server"), ("One", ""), ("2", "x,y")]);
        assert_eq!(cols, [DataCol { key: "Owner".into(), label: "Owner name".into() }, DataCol { key: "Cost".into(), label: "Cost".into() }]);
        assert_eq!(rows[0].values["Cost"], "5");
        assert_eq!(cell(&rows[1], "id"), "2");
        assert_eq!(cell(&rows[2], "Owner"), "");
    }

    #[test]
    fn csv_uses_the_heading_labels_and_quotes() {
        let names = vec!["One".to_string(), "Two".to_string()];
        let columns: Vec<String> = ["page", "id", "text", "Owner"].map(String::from).to_vec();
        assert_eq!(inventory_csv(&model(), &names, 0, &columns, true), "Page,Shape ID,Text,Owner name\r\nOne,1,Web server,Ann\r\nOne,2,,\r\nTwo,3,\"x,y\",\r\n");
        assert_eq!(inventory_csv(&model(), &names, 1, &columns[1..3], false), "Shape ID,Text\r\n3,\"x,y\"\r\n");
    }
}
