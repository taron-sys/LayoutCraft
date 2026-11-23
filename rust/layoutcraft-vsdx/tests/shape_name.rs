//! Shape names (step 337): Shape.Name / NameU written the way Visio writes them (probe-shapename.ps1), unique on the page.
use std::io::Cursor;

use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
use layoutcraft_vsdx::shapename::{names, set_shape_name};
use layoutcraft_vsdx::{write_blank_document, BlankOptions, Package};

fn open(bytes: Vec<u8>) -> Package {
    Package::from_reader(Cursor::new(bytes)).unwrap()
}

fn two_shapes() -> (Package, String, String) {
    let blank = open(write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".into() }).unwrap());
    let (b1, a) = draw_shape(&blank, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    let (b2, b) = draw_shape(&open(b1), 0, DrawKind::Ellipse, 5.0, 1.0, 6.0, 2.0).unwrap();
    (open(b2), a, b)
}

fn name_of(package: &Package, id: &str) -> Option<String> {
    names(package, 0).unwrap().into_iter().find(|(shape, _)| shape == id).map(|(_, name)| name)
}

#[test]
fn a_name_is_written_read_back_and_made_unique_like_visio() {
    let (package, a, b) = two_shapes();
    assert!(name_of(&package, &a).is_some_and(|n| n.starts_with("Rectangle")), "our drawing tools give a shape a name of its kind: {:?}", name_of(&package, &a));
    let (bytes, given) = set_shape_name(&package, 0, &a, "My shape").unwrap();
    assert_eq!(given, "My shape");
    let one = open(bytes);
    assert_eq!(name_of(&one, &a).as_deref(), Some("My shape"));
    let xml = one.text(&one.pages().unwrap()[0].part).unwrap().to_string();
    assert!(xml.contains(&format!("<Shape ID='{a}' NameU='My shape' IsCustomNameU='1' Name='My shape' IsCustomName='1'")), "{xml}");
    // the same name on the second shape becomes "My shape.2" (Visio: probe step 337)
    let (bytes2, given2) = set_shape_name(&one, 0, &b, "my SHAPE").unwrap();
    assert_eq!(given2, "my SHAPE.2");
    assert_eq!(name_of(&open(bytes2), &b).as_deref(), Some("my SHAPE.2"));
}

#[test]
fn renaming_a_shape_to_its_own_name_changes_nothing_and_an_empty_name_removes_it() {
    let (package, a, _b) = two_shapes();
    let one = open(set_shape_name(&package, 0, &a, "Same").unwrap().0);
    let again = set_shape_name(&one, 0, &a, "Same").unwrap();
    assert_eq!(again.1, "Same", "the shape's own name is not a clash");
    let none = open(set_shape_name(&open(again.0), 0, &a, "  ").unwrap().0);
    assert_eq!(name_of(&none, &a), None);
    let xml = none.text(&none.pages().unwrap()[0].part).unwrap().to_string();
    assert!(!xml.contains("IsCustomNameU"), "{xml}");
}

#[test]
fn odd_characters_are_kept_and_bad_names_and_shapes_are_refused() {
    let (package, a, _b) = two_shapes();
    let odd = "a \"b\" & <c>.d";
    let named = open(set_shape_name(&package, 0, &a, odd).unwrap().0);
    assert_eq!(name_of(&named, &a).as_deref(), Some(odd));
    assert!(set_shape_name(&package, 0, "999", "x").is_err());
    assert!(set_shape_name(&package, 3, &a, "x").is_err());
    assert!(set_shape_name(&package, 0, &a, &"n".repeat(300)).is_err());
    assert!(set_shape_name(&package, 0, &a, "line\nbreak").is_err());
}
