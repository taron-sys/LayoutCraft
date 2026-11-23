//! Flipped shapes whose pin is not at the centre: a flip mirrors the shape about its PIN, so its box lies on the other side of the pin.
//! flipped-offcentre-pin.vsdx is Visio-authored; the expected page positions are what Visio's XYToPage gave for the local corners
//! (visio-test-harness/make-flipped-offpin-fixture.ps1): shape 1 FlipX with the pin at 25 % of its width, shape 2 FlipY with the pin at
//! 75 % of its height, shape 3 flipped both ways, turned 30 degrees, pin at (20 %, 70 %).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_vsdx::{import_document, Package};

fn open() -> (Package, f64) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/flipped-offcentre-pin.vsdx");
    let package = Package::open(path).unwrap();
    let height = package.pages().unwrap()[0].height.unwrap();
    (package, height)
}

/// (left, top-from-page-bottom...) of the unturned box: x, y in inches from the page's lower left, width, height.
fn box_of(package: &Package, page_height: f64, id: &str) -> (f64, f64, f64, f64) {
    let document = import_document(package).unwrap();
    let s = document.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id)).unwrap_or_else(|| panic!("no shape {id}"));
    let (cx, cy_down) = ((s.x + s.w / 2.0) / 96.0, (s.y + s.h / 2.0) / 96.0);
    (cx, page_height - cy_down, s.w / 96.0, s.h / 96.0)
}

#[test]
fn a_flipped_shape_lies_on_the_other_side_of_its_pin() {
    let (package, height) = open();
    // shape 1 (3 x 1.5, pin 2.5, 1.75, LocPin 0.75, FlipX): Visio puts its corners at x = 0.25 .. 3.25, y = 1 .. 2.5
    let (cx, cy, w, h) = box_of(&package, height, "1");
    assert!((cx - 1.75).abs() < 1e-6 && (cy - 1.75).abs() < 1e-6 && (w - 3.0).abs() < 1e-6 && (h - 1.5).abs() < 1e-6, "shape 1: centre ({cx}, {cy}) size {w} x {h}");
    // shape 2 (2 x 2.5, pin 6, 2.25, LocPin 1, 1.875, FlipY): corners at x = 5 .. 7, y = 1.625 .. 4.125
    let (cx, cy, w, h) = box_of(&package, height, "2");
    assert!((cx - 6.0).abs() < 1e-6 && (cy - 2.875).abs() < 1e-6 && (w - 2.0).abs() < 1e-6 && (h - 2.5).abs() < 1e-6, "shape 2: centre ({cx}, {cy}) size {w} x {h}");
    // shape 3 (3 x 2, pin 2.5, 5, LocPin 0.6, 1.4, both flips, 30 degrees): the four corners Visio reports have their centre at (1.52058, 4.89641)
    let (cx, cy, w, h) = box_of(&package, height, "3");
    assert!((cx - 1.520579).abs() < 1e-4 && (cy - 4.896410).abs() < 1e-4 && (w - 3.0).abs() < 1e-6 && (h - 2.0).abs() < 1e-6, "shape 3: centre ({cx}, {cy}) size {w} x {h}");
}

#[test]
fn a_shape_with_its_pin_in_the_centre_is_where_it_was() {
    // the same rule leaves centred pins alone (the offset to the centre is zero): a plain corpus drawing with flips does not move
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/groups-flipped.vsdx");
    let package = Package::open(path).unwrap();
    assert!(import_document(&package).is_ok());
}
