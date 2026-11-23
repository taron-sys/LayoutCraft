//! Insert > Picture (step 323): a PNG / JPEG / GIF / BMP becomes a Foreign shape with a bitmap `ForeignData` and a media part on the page.
use std::io::Cursor;

use layoutcraft_vsdx::draw::{insert_picture, picture_kind};
use layoutcraft_vsdx::{import_document, write_blank_document, BlankOptions, Package};

/// A 1 x 1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xff, 0xff, 0x3f, 0x03, 0x00, 0x05, 0x00, 0x01, 0xa5, 0xf6, 0x45, 0x40, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn blank() -> Package {
    Package::from_reader(Cursor::new(write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".into() }).unwrap())).unwrap()
}

fn png() -> Vec<u8> {
    PNG.to_vec()
}

#[test]
fn the_picture_kind_comes_from_the_first_bytes() {
    assert_eq!(picture_kind(&png()), Some(("png", "image/png")));
    assert_eq!(picture_kind(&[0xff, 0xd8, 0xff, 0xe0, 0, 0]), Some(("jpg", "image/jpeg")));
    assert_eq!(picture_kind(b"GIF89a......"), Some(("gif", "image/gif")));
    assert_eq!(picture_kind(&[b'B', b'M', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Some(("bmp", "image/bmp")));
    assert_eq!(picture_kind(b"<svg></svg>"), None);
    assert_eq!(picture_kind(&[]), None);
}

#[test]
fn a_picture_is_a_foreign_shape_in_the_box_with_its_media_part() {
    let package = blank();
    let (bytes, id) = insert_picture(&package, 0, &png(), 2.0, 3.0, 5.0, 4.0).unwrap();
    let back = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert!(back.names().any(|n| n.starts_with("visio/media/") && n.ends_with(".png")), "a media part");
    let document = import_document(&back).unwrap();
    let shape = document.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).expect("the picture shape");
    assert!(shape.image.is_some(), "the shape carries an image");
    // 3 x 1 inch at the lower left corner (2, 3): the model is in pixels with y down from the top of the 8.5 inch page
    assert!((shape.w - 288.0).abs() < 1e-6 && (shape.h - 96.0).abs() < 1e-6, "{} x {}", shape.w, shape.h);
    assert!((shape.x - 192.0).abs() < 1e-6 && (shape.y - (8.5 - 4.0) * 96.0).abs() < 1e-6, "{}, {}", shape.x, shape.y);
}

#[test]
fn two_pictures_get_two_media_parts_and_ids() {
    let package = blank();
    let (one, id1) = insert_picture(&package, 0, &png(), 1.0, 1.0, 2.0, 2.0).unwrap();
    let package = Package::from_reader(Cursor::new(one)).unwrap();
    let (two, id2) = insert_picture(&package, 0, &png(), 3.0, 1.0, 4.0, 2.0).unwrap();
    assert_ne!(id1, id2);
    let back = Package::from_reader(Cursor::new(two)).unwrap();
    assert_eq!(back.names().filter(|n| n.starts_with("visio/media/")).count(), 2);
    assert_eq!(import_document(&back).unwrap().pages[0].shapes.iter().filter(|s| s.image.is_some()).count(), 2);
}

#[test]
fn bad_pictures_and_boxes_are_refused() {
    let package = blank();
    assert!(insert_picture(&package, 0, b"not a picture", 1.0, 1.0, 2.0, 2.0).is_err());
    assert!(insert_picture(&package, 0, &png(), 1.0, 1.0, 1.01, 2.0).is_err(), "too narrow");
    assert!(insert_picture(&package, 0, &png(), 1.0, f64::NAN, 2.0, 2.0).is_err());
    assert!(insert_picture(&package, 0, &png(), 0.0, 0.0, 300.0, 5.0).is_err(), "too large");
    assert!(insert_picture(&package, 5, &png(), 1.0, 1.0, 2.0, 2.0).is_err(), "no such page");
}
