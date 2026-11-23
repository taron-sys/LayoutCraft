//! A stencil master whose group body is a picture (the CyberPower / APC rack units: one EMF each) must draw that picture when it is dropped
//! on a page. The importer only drew pictures that were children of the page shape, so these shapes came out blank.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::io::Cursor;
use std::path::PathBuf;

use layoutcraft_vsdx::stencil::{import_master, master_list};
use layoutcraft_vsdx::writer::add_master_instance;
use layoutcraft_vsdx::{import_document, write_blank_document, BlankOptions, Package};

#[test]
fn a_dropped_master_whose_body_is_a_picture_draws_the_picture_and_the_picture_belongs_to_the_group() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU v2024.vssx");
    if !path.exists() {
        return; // the example stencils are not part of every checkout
    }
    let stencil = Package::open(path).unwrap();
    let list = master_list(&stencil).unwrap();
    let id = list.as_array().unwrap().iter().find(|m| m["name"] == "PDU15M10AT Front").unwrap()["id"].as_str().unwrap().to_string();
    let blank = Package::from_reader(Cursor::new(
        write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".into() }).unwrap(),
    ))
    .unwrap();
    let (bytes, master) = import_master(&blank, &stencil, &id).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with, 0, &master, 5.0, 4.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[0];
    let pictures: Vec<_> = page.shapes.iter().filter(|s| s.image.is_some()).collect();
    assert_eq!(pictures.len(), 1, "the master's EMF is drawn once: {} shapes", page.shapes.len());
    let picture = pictures[0];
    // a click on the picture selects the group that owns it: the picture is the group's own synthetic shape, or a member (a shrunk drop writes
    // its sub-shapes out) whose group root is the dropped group
    let root = picture.group.as_ref().and_then(|g| g["rootId"].as_str()).map(str::to_string);
    assert!(picture.visio_id.as_deref() == Some(shape.as_str()) || root.as_deref() == Some(shape.as_str()), "owner {:?} / {:?}, group {shape}", picture.visio_id, root);
    // the master is 19 x 1.75 in on its own 1:10 page, so 1.9 x 0.175 in (182 x 17 px) when dropped on a 1:1 page
    assert!(picture.w > 150.0 && picture.w < 200.0 && picture.h > 12.0 && picture.h < 25.0, "it fills the 1.9 x 0.175 in group: {} x {} px", picture.w, picture.h);
}

fn drop_pdu83107(width_in: f64) -> (Package, String) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83107 v2024.vssx");
    let stencil = Package::open(path).unwrap();
    let list = master_list(&stencil).unwrap();
    let id = list.as_array().unwrap()[0]["id"].as_str().unwrap().to_string();
    let blank = Package::from_reader(Cursor::new(
        write_blank_document(&BlankOptions { width_in, height_in: 76.0, metric: false, name: "Page-1".into() }).unwrap(),
    ))
    .unwrap();
    let (bytes, master) = import_master(&blank, &stencil, &id).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with, 0, &master, 2.0, 38.0).unwrap();
    (Package::from_reader(Cursor::new(bytes)).unwrap(), shape)
}

/// Probed with Visio (visio-test-harness/probe-pdu-drop.ps1, visio-test-runs/pdu83107): the master is authored on a 1:10 page, a drop on a
/// 1:1 page is 0.22 x 7.2094 in, not 2.2 x 72.094 in, and its 100 pictures (the breakers, the outlets, the RJ45 ports sit in a group inside
/// the master's top group) are all drawn, each at the size and place Visio gives it (bounding boxes read back with COM).
#[test]
fn a_master_authored_on_a_one_to_ten_page_is_dropped_a_tenth_of_its_size_with_every_nested_picture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83107 v2024.vssx");
    if !path.exists() {
        return;
    }
    let (package, shape) = drop_pdu83107(4.0);
    let editor = layoutcraft_vsdx::groupedit::GroupEditor::open(&package, 0).unwrap();
    let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == shape.as_str()).unwrap().clone();
    let (w, h) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
    assert!((w - 0.22).abs() < 1e-6 && (h - 7.2094).abs() < 1e-6, "dropped size {w} x {h}");
    let document = import_document(&package).unwrap();
    let pictures: Vec<_> = document.pages[0].shapes.iter().filter(|s| s.image.is_some()).collect();
    assert_eq!(pictures.len(), 100, "every picture of the master is drawn, nested ones too");
    assert!(pictures.iter().all(|p| p.w < 20.0 && p.h < 20.0), "all pictures are scaled with the group");
    // the first breaker picture: Visio puts it 0.0804 x 0.1410 in
    assert!(pictures.iter().any(|p| (p.w - 7.72).abs() < 0.05 && (p.h - 13.53).abs() < 0.05), "a breaker picture has Visio's size");
}

/// Visio does not recalculate the cells an instance inherits when its Width changes: a shrunk group instance must carry its sub-shapes
/// written out and scaled (`MasterShape='70'` is the first picture). The PDU's NURBS rows used to make the writer give up.
#[test]
fn a_shrunk_drop_writes_the_masters_sub_shapes_scaled() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83107 v2024.vssx");
    if !path.exists() {
        return;
    }
    let (package, _) = drop_pdu83107(4.0);
    let part = package.pages().unwrap()[0].part.clone();
    let xml = package.text(&part).unwrap().to_string();
    assert!(xml.matches("<Shape ").count() > 100, "the sub-shapes are written out: {} shapes", xml.matches("<Shape ").count());
    assert!(xml.contains("MasterShape='48'"), "the first breaker picture is one of them");
}

/// Visio (visio-test-harness/probe-tall-1d-end.ps1): a tall 1-D group master (the PDU: Height = GUARD(Width * 32.77)) keeps that ratio when its
/// end moves (Width 0.22 -> 0.32 gives Height 7.2094 -> 10.4864, its sub-shapes scale with it). Our resize of such a shape keeps the ratio the same way
/// whatever size is asked for.
#[test]
fn resizing_a_tall_one_dimensional_group_keeps_height_over_width_like_visio() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83107 v2024.vssx");
    if !path.exists() {
        return;
    }
    let (package, shape) = drop_pdu83107(4.0);
    let editor = layoutcraft_vsdx::groupedit::GroupEditor::open(&package, 0).unwrap();
    let patches = editor.resize_member(&shape, (0.32, 5.0), (0.5, 0.5), false).unwrap();
    let bytes = layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let frame = layoutcraft_vsdx::groupedit::GroupEditor::open(&after, 0).unwrap().describe().as_array().unwrap().iter().find(|f| f["id"] == shape.as_str()).unwrap().clone();
    let (w, h) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
    assert!((w - 0.32).abs() < 1e-6, "the width asked for: {w}");
    assert!((h - 10.4864).abs() < 2e-3, "Visio's height for a width of 0.32 in is 10.4864 in, got {h}");
}
