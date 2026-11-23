//! Alt text authoring (step 335): Shape > Alt Text writes the row `visAltDescription` of the shape's User section, the way Visio does (probe-alttext.ps1).
use std::io::Cursor;

use layoutcraft_vsdx::alttext::set_alt_text;
use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
use layoutcraft_vsdx::{import_document, write_blank_document, BlankOptions, Package};

fn open(bytes: Vec<u8>) -> Package {
    Package::from_reader(Cursor::new(bytes)).unwrap()
}

fn drawing() -> (Package, String) {
    let blank = open(write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".into() }).unwrap());
    let (bytes, id) = draw_shape(&blank, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    (open(bytes), id)
}

fn alt_of(package: &Package, id: &str) -> Option<String> {
    import_document(package).unwrap().pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id)).and_then(|s| s.alt_text.clone())
}

fn page_xml(package: &Package) -> String {
    package.text(&package.pages().unwrap()[0].part).unwrap().to_string()
}

#[test]
fn the_alt_text_is_written_and_read_back() {
    let (package, id) = drawing();
    assert_eq!(alt_of(&package, &id), None);
    let with = open(set_alt_text(&package, 0, &id, "A box that says <hello> & more").unwrap());
    assert_eq!(alt_of(&with, &id).as_deref(), Some("A box that says <hello> & more"));
    let xml = page_xml(&with);
    assert!(xml.contains("<Section N='User'><Row N='visAltDescription'><Cell N='Value' V='A box that says &lt;hello&gt; &amp; more' U='STR'/>"), "{xml}");
    assert!(xml.contains("<Row N='visAltText'>"), "Visio writes the flag row beside it");
}

#[test]
fn changing_and_removing_the_alt_text_keeps_the_rest() {
    let (package, id) = drawing();
    let one = open(set_alt_text(&package, 0, &id, "first").unwrap());
    let two = open(set_alt_text(&one, 0, &id, "second").unwrap());
    assert_eq!(alt_of(&two, &id).as_deref(), Some("second"));
    assert_eq!(page_xml(&two).matches("visAltDescription").count(), 1, "the row is replaced, not added twice");
    let none = open(set_alt_text(&two, 0, &id, "").unwrap());
    assert_eq!(alt_of(&none, &id), None);
    assert!(!page_xml(&none).contains("visAltDescription"));
    assert!(page_xml(&none).contains("visAltText"), "the other row of the section is left alone");
    // removing where there is nothing changes nothing
    let again = open(set_alt_text(&package, 0, &id, "").unwrap());
    assert_eq!(page_xml(&again), page_xml(&package));
}

#[test]
fn a_second_shape_is_not_touched() {
    let (package, id) = drawing();
    let (bytes, id2) = draw_shape(&package, 0, DrawKind::Ellipse, 5.0, 1.0, 6.0, 2.0).unwrap();
    let both = open(bytes);
    let set = open(set_alt_text(&both, 0, &id, "alt").unwrap());
    assert_eq!(alt_of(&set, &id).as_deref(), Some("alt"));
    assert_eq!(alt_of(&set, &id2), None);
}

#[test]
fn unknown_shapes_pages_and_huge_texts_are_refused() {
    let (package, id) = drawing();
    assert!(set_alt_text(&package, 0, "999", "x").is_err());
    assert!(set_alt_text(&package, 4, &id, "x").is_err());
    assert!(set_alt_text(&package, 0, &id, &"x".repeat(5000)).is_err());
}
