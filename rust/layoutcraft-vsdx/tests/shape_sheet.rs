//! The read-only ShapeSheet of a shape (what the shell's Shape Sheet pane lists): the shape's cells and sections, its master's merged in.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
use layoutcraft_vsdx::groupedit::GroupEditor;
use layoutcraft_vsdx::{write_blank_document, BlankOptions, Package};

fn cell<'a>(sheet: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    sheet["cells"].as_array()?.iter().find(|c| c["name"] == name)
}

#[test]
fn a_drawn_shape_lists_its_transform_cells_and_geometry() {
    let bytes = write_blank_document(&BlankOptions { width_in: 8.5, height_in: 11.0, metric: false, name: "Main".to_string() }).unwrap();
    let p = Package::from_bytes(&bytes).unwrap();
    let (b, id) = draw_shape(&p, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    let p = Package::from_bytes(&b).unwrap();
    let sheet = GroupEditor::open(&p, 0).unwrap().shape_sheet(&id).unwrap();
    assert_eq!(sheet["id"], id.as_str());
    assert_eq!(cell(&sheet, "Width").unwrap()["value"], "2");
    assert_eq!(cell(&sheet, "Height").unwrap()["value"], "1");
    assert_eq!(cell(&sheet, "PinX").unwrap()["value"], "2");
    assert_eq!(cell(&sheet, "PinY").unwrap()["value"], "1.5");
    let geometry = sheet["sections"].as_array().unwrap().iter().find(|s| s["name"] == "Geometry").expect("a Geometry section");
    assert_eq!(geometry["fromMaster"], false, "a drawn shape carries its own geometry");
    assert!(geometry["rows"].as_array().unwrap().len() >= 4, "MoveTo and LineTo rows");
    assert!(GroupEditor::open(&p, 0).unwrap().shape_sheet("999").is_none(), "an unknown shape has no sheet");
}

#[test]
fn a_visio_authored_shape_keeps_formulas_and_units() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/comments.vsdx");
    let p = Package::from_bytes(&std::fs::read(path).unwrap()).unwrap();
    let sheet = GroupEditor::open(&p, 0).unwrap().shape_sheet("1").unwrap();
    let loc = cell(&sheet, "LocPinX").unwrap();
    assert_eq!(loc["value"], "1");
    assert_eq!(loc["formula"], "Width*0.5", "the stored formula is shown as written");
    let geometry = sheet["sections"].as_array().unwrap().iter().find(|s| s["name"] == "Geometry").unwrap();
    let rows = geometry["rows"].as_array().unwrap();
    assert_eq!(rows[1]["cells"].as_array().unwrap().iter().find(|c| c["name"] == "X").unwrap()["formula"], "Width*1");
}
