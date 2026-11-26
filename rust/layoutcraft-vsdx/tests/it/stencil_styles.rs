//! Style sheets that come along when a stencil's master is dropped on a drawing.
use layoutcraft_vsdx::stencil::import_master;
use layoutcraft_vsdx::{write_blank_document, BlankOptions, Package};
use std::io::{Cursor, Write};

const NS: &str = "xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships'";

/// A hand-made stencil with two masters in the style `Normal` (sheet 3), whose line colour is `colour`.
fn stencil(colour: &str) -> Package {
    let master = |n: u32| format!("<Master ID='{n}' NameU='M{n}'><Rel r:id='rId{n}'/></Master>");
    let shape = format!("<MasterContents {NS}><Shapes><Shape ID='1' Type='Shape' LineStyle='3' FillStyle='3' TextStyle='3'/></Shapes></MasterContents>");
    let relation = |n: u32| format!("<Relationship Id='rId{n}' Type='http://schemas.microsoft.com/visio/2010/relationships/master' Target='master{n}.xml'/>");
    let parts = [
        ("[Content_Types].xml", "<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'/>".to_string()),
        ("visio/document.xml", format!("<VisioDocument {NS}><StyleSheets><StyleSheet ID='3' NameU='Normal' Name='Normal'>{colour}</StyleSheet></StyleSheets></VisioDocument>")),
        ("visio/masters/masters.xml", format!("<Masters {NS}>{}{}</Masters>", master(1), master(2))),
        ("visio/masters/_rels/masters.xml.rels", format!("<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>{}{}</Relationships>", relation(1), relation(2))),
        ("visio/masters/master1.xml", shape.clone()),
        ("visio/masters/master2.xml", shape),
    ];
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    Package::from_bytes(&zip.finish().unwrap().into_inner()).unwrap()
}

/// A new drawing, whose `Normal` style takes its colours from the theme.
fn themed_drawing() -> Package {
    let options = BlankOptions { width_in: 8.5, height_in: 11.0, metric: false, name: "Page-1".to_string() };
    Package::from_bytes(&write_blank_document(&options).unwrap()).unwrap()
}

fn drop_both(stencil: &Package) -> Package {
    let mut drawing = themed_drawing();
    for master in ["1", "2"] {
        drawing = Package::from_bytes(&import_master(&drawing, stencil, master).unwrap().0).unwrap();
    }
    drawing
}

/// A stencil's own black `Normal` must not be replaced by the drawing's themed one (black line art turned into one theme
/// colour); it is copied once under another name and both masters use the copy.
#[test]
fn a_plain_stencil_style_survives_a_themed_drawing() {
    let drawing = drop_both(&stencil("<Cell N='LineColor' V='#000000'/>"));
    let document = drawing.text("visio/document.xml").unwrap();
    assert_eq!(document.matches("NameU='Normal.").count(), 1, "{document}");
    for part in ["visio/masters/master1.xml", "visio/masters/master2.xml"] {
        let master = drawing.text(part).unwrap();
        assert!(!master.contains("LineStyle='3'"), "{master}");
    }
}

/// A stencil style that follows the theme itself is shared with the drawing's, so the shape takes the drawing's theme.
#[test]
fn a_themed_stencil_style_is_shared_with_the_drawing() {
    let drawing = drop_both(&stencil("<Cell N='LineColor' V='0' F='THEMEVAL()'/>"));
    assert!(!drawing.text("visio/document.xml").unwrap().contains("NameU='Normal."));
    assert!(drawing.text("visio/masters/master1.xml").unwrap().contains("LineStyle='3'"));
}
