//! Regressions found by the 2026 examples survey. Expected colours are the
//! values Visio itself evaluated (cell oracle, Visio 16.0.14332).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_model::{Document, Shape};
use layoutcraft_vsdx::{import_document, Package};

fn import(name: &str) -> Document {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    import_document(&Package::open(path).unwrap()).unwrap()
}

fn shape<'a>(doc: &'a Document, id: &str) -> &'a Shape {
    doc.pages[0]
        .shapes
        .iter()
        .find(|s| s.visio_id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("shape {id}"))
}

/// Channel-wise distance between two `#rrggbb` colours.
fn near(actual: &str, expected: &str, tolerance: i32) -> bool {
    let channel = |hex: &str, i: usize| i32::from_str_radix(&hex[1 + i * 2..3 + i * 2], 16).unwrap();
    (0..3).all(|i| (channel(actual, i) - channel(expected, i)).abs() <= tolerance)
}

/// The BPMN template's page overrides the document's colour variation
/// (`VariationColorIndex` 2 in the PageSheet). Using the document's variation 0
/// painted every shape red where Visio draws blue.
#[test]
fn page_level_theme_variation_is_used() {
    let doc = import("BPMN cross-functional chart.vstx");
    // Swimlane header child: Visio FillForegnd #1b587b; Decision: #1b587b;
    // Decision line #144866.
    let decision = shape(&doc, "30");
    let fill = decision.fill.as_deref().expect("decision fill");
    assert!(near(fill, "#1b587b", 2), "decision fill {fill}");
    let stroke = decision.stroke.as_deref().expect("decision stroke");
    assert!(near(stroke, "#144866", 2), "decision stroke {stroke}");
    // A Document shape (QuickStyle 102): light fill #f6f1f2, red-brown line #83202b.
    let document = shape(&doc, "24");
    assert!(near(document.fill.as_deref().unwrap(), "#f6f1f2", 2));
    assert!(near(document.stroke.as_deref().unwrap(), "#83202b", 2));
}

/// A master-child title with no Character colour takes it from the TextStyle
/// chain (`Themed`), which is white on the entity header, not the header's own
/// colour.
#[test]
fn themed_master_child_text_is_light_on_a_filled_header() {
    let doc = import("Inventory management.vsdx");
    let entity = shape(&doc, "1");
    let title = entity
        .sub_texts
        .iter()
        .find(|text| text.text == "Manufacturer")
        .expect("entity title");
    assert!(near(&title.color, "#ffffff", 2), "title colour {}", title.color);
}

/// ER attribute rows hide their own background geometry with `NoShow`; the
/// UML list-band heuristic must not paint a header-coloured rectangle there.
#[test]
fn hidden_row_geometry_gets_no_list_band() {
    let doc = import("Inventory management.vsdx");
    let row = shape(&doc, "6");
    assert!(
        row.parts.iter().all(|part| part.source.is_some()),
        "every part of an ER attribute row comes from a master child"
    );
}
