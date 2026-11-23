#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::io::Cursor;
use std::path::PathBuf;

use layoutcraft_model::{Document, Shape, ShapePart};
use layoutcraft_vsdx::{import_document, part_style_patch, patch_page_cells, Package, PartStyle};

fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic-network-diagram.vsdx")
}

fn import(package: &Package) -> Document {
    import_document(package).unwrap()
}

/// First master-composited shape with an editable part of the given role.
fn find_part<'a>(doc: &'a Document, role: &str) -> (&'a Shape, usize) {
    for shape in &doc.pages[0].shapes {
        for (index, part) in shape.parts.iter().enumerate() {
            if let Some(source) = &part.source {
                if source.role == role && source.page_shape_id.is_some() {
                    return (shape, index);
                }
            }
        }
    }
    panic!("no {role} part with a page-level child");
}

fn same_part(a: &ShapePart, b: &ShapePart) -> bool {
    a.source == b.source
}

#[test]
fn fill_edit_changes_only_the_edited_children_paint() {
    let package = Package::open(source()).unwrap();
    let before = import(&package);
    let (shape, index) = find_part(&before, "fill");
    let part = &shape.parts[index];
    let visio_id = shape.visio_id.clone().unwrap();

    let patch = part_style_patch(
        part,
        &PartStyle {
            fill: Some("#00ff00".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let bytes = patch_page_cells(&package, 0, &[patch]).unwrap();
    let after = import(&Package::from_reader(Cursor::new(bytes)).unwrap());

    let after_shape = after.pages[0]
        .shapes
        .iter()
        .find(|s| s.visio_id.as_deref() == Some(visio_id.as_str()))
        .unwrap();
    assert_eq!(after_shape.parts.len(), shape.parts.len());
    for (old, new) in shape.parts.iter().zip(&after_shape.parts) {
        assert!(same_part(old, new));
        if same_part(old, part) {
            assert_eq!(new.fill.as_deref(), Some("#00ff00"));
        } else {
            assert_eq!(new.fill, old.fill, "other parts keep their fill");
        }
    }
    // Nothing else on the page moved or changed colour.
    for (old, new) in before.pages[0].shapes.iter().zip(&after.pages[0].shapes) {
        assert_eq!(old.visio_id, new.visio_id);
        if old.visio_id.as_deref() != Some(visio_id.as_str()) {
            assert_eq!(old.fill, new.fill);
        }
    }
}

#[test]
fn stroke_edit_writes_colour_and_width() {
    let package = Package::open(source()).unwrap();
    let before = import(&package);
    let (shape, index) = find_part(&before, "stroke");
    let part = &shape.parts[index];
    let visio_id = shape.visio_id.clone().unwrap();

    let patch = part_style_patch(
        part,
        &PartStyle {
            stroke: Some("#0000ff".into()),
            stroke_width: Some(4.0),
            ..Default::default()
        },
    )
    .unwrap();
    let bytes = patch_page_cells(&package, 0, &[patch]).unwrap();
    let after = import(&Package::from_reader(Cursor::new(bytes)).unwrap());
    let after_shape = after.pages[0]
        .shapes
        .iter()
        .find(|s| s.visio_id.as_deref() == Some(visio_id.as_str()))
        .unwrap();
    let new = after_shape
        .parts
        .iter()
        .find(|p| same_part(p, part))
        .expect("part survives the edit");
    assert_eq!(new.stroke.as_deref(), Some("#0000ff"));
    assert!((new.stroke_width.unwrap() - 4.0).abs() < 0.05, "{:?}", new.stroke_width);
}

#[test]
fn invalid_edits_are_rejected() {
    let package = Package::open(source()).unwrap();
    let doc = import(&package);
    let (shape, index) = find_part(&doc, "fill");
    let fill_part = &shape.parts[index];

    // A fill part cannot take a stroke edit, and an edit needs something to say.
    assert!(part_style_patch(
        fill_part,
        &PartStyle { stroke: Some("#000000".into()), ..Default::default() }
    )
    .is_err());
    assert!(part_style_patch(fill_part, &PartStyle::default()).is_err());

    // Tint parts are derived from the fill.
    let mut tint = fill_part.clone();
    tint.source.as_mut().unwrap().role = "tint".into();
    assert!(part_style_patch(
        &tint,
        &PartStyle { fill: Some("#ffffff".into()), ..Default::default() }
    )
    .is_err());

    // A part with no page-level child, or no provenance, cannot be written back.
    let mut orphan = fill_part.clone();
    orphan.source.as_mut().unwrap().page_shape_id = None;
    assert!(part_style_patch(
        &orphan,
        &PartStyle { fill: Some("#ffffff".into()), ..Default::default() }
    )
    .is_err());
    let mut plain = fill_part.clone();
    plain.source = None;
    assert!(part_style_patch(
        &plain,
        &PartStyle { fill: Some("#ffffff".into()), ..Default::default() }
    )
    .is_err());
}
