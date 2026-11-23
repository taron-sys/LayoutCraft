#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// The swimlane headers of `patient-check-in.vsdx` are master children whose `TxtAngle`
/// is -270 degrees; Visio draws their labels reading bottom to top.
#[test]
fn master_child_text_angle_turns_the_lane_label() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 0).unwrap());
    let at = svg.find("<tspan>Front</tspan>").expect("lane label drawn");
    let before = &svg[..at];
    let turn = before.rfind("<g transform=\"rotate(").expect("label inside a rotate group");
    assert!(before[turn..].starts_with("<g transform=\"rotate(270 "), "{}", &before[turn..turn + 40]);
}
