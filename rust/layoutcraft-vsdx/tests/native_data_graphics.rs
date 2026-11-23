#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_model::Shape;
use layoutcraft_vsdx::{import_document, Package};

fn collect<'a>(shapes: &'a [Shape], out: &mut Vec<&'a Shape>) {
    for shape in shapes {
        out.push(shape);
    }
}

/// `Network Diagram_start.vsdx` is Visio-authored and carries native Text
/// callout, Heading, Data bar and Icon set instances.
#[test]
fn native_data_graphics_are_tagged_without_changing_geometry() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/Network Diagram_start.vsdx");
    let document = import_document(&Package::open(&source).unwrap()).unwrap();

    let mut tagged = Vec::new();
    for page in &document.pages {
        let mut shapes = Vec::new();
        collect(&page.shapes, &mut shapes);
        tagged.extend(shapes.into_iter().filter(|s| s.native_data_graphic.is_some()));
    }
    assert!(tagged.len() >= 90, "expected ~100 native graphics, got {}", tagged.len());

    let kinds = |kind: &str| {
        tagged
            .iter()
            .filter(|s| s.native_data_graphic.as_ref().unwrap().kind == kind)
            .count()
    };
    assert!(kinds("text") > 0 && kinds("heading") > 0 && kinds("bar") > 0 && kinds("icon") > 0);

    // The first callout on page 1 reads IPAddress from Visio shape 15.
    let first = document.pages[0]
        .shapes
        .iter()
        .find_map(|s| s.native_data_graphic.as_ref().filter(|n| n.field == "IPAddress"))
        .expect("an IP address callout");
    assert_eq!(first.target_visio_id.as_deref(), Some("15"));
    assert_eq!(first.master, "Text callout");

    // Native graphics are metadata only: no regenerated callout, geometry kept.
    for shape in &tagged {
        assert!(shape.callout.is_none());
        assert_ne!(shape.shape_type, "dataCallout");
    }
}
