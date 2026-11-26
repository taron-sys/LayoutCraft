//! A 32 bit BMP with the 40 byte header may carry transparency in its fourth byte; the PNG served to renderers keeps it.
//! The BMPs here are built by hand.
use std::io::Cursor;

use layoutcraft_vsdx::draw::insert_picture;
use layoutcraft_vsdx::{import_document, media_bytes, write_blank_document, BlankOptions, Package};

/// A one pixel wide, uncompressed 32 bit BMP; `rows` are (B, G, R, fourth byte) from the top of the picture.
fn bmp(rows: &[[u8; 4]], top_down: bool) -> Vec<u8> {
    let size = rows.len() as u32 * 4;
    let height = if top_down { -(rows.len() as i32) } else { rows.len() as i32 };
    let mut out = b"BM".to_vec();
    for value in [54 + size, 0, 54, 40, 1, height as u32] {
        out.extend(value.to_le_bytes());
    }
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend([0u8; 24]);
    let mut stored = rows.to_vec();
    if !top_down {
        stored.reverse();
    }
    out.extend(stored.concat());
    out
}

/// Insert the BMP into a blank drawing and return the (R, G, B, A) rows of the picture a renderer gets.
fn served(data: &[u8]) -> Vec<[u8; 4]> {
    let options = BlankOptions { width_in: 2.0, height_in: 2.0, metric: false, name: "Page-1".into() };
    let blank = Package::from_reader(Cursor::new(write_blank_document(&options).unwrap())).unwrap();
    let (bytes, _) = insert_picture(&blank, 0, data, 0.0, 0.0, 1.0, 1.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let document = import_document(&package).unwrap();
    let record = document.media.values().next().expect("a media record");
    let png = media_bytes(&package, record).expect("picture bytes");
    image::load_from_memory(&png).unwrap().to_rgba8().pixels().map(|pixel| pixel.0).collect()
}

#[test]
fn premultiplied_transparency_is_kept_and_the_colour_restored() {
    for top_down in [false, true] {
        let rows = served(&bmp(&[[0, 0, 0, 0], [0, 50, 100, 128], [10, 20, 30, 255]], top_down));
        assert_eq!(rows[0][3], 0, "clear stays clear (top_down {top_down})");
        let [r, g, b, a] = rows[1];
        assert_eq!(a, 128);
        assert!(r > 100 && g > 50 && b == 0, "colour scaled back up: {r} {g} {b}");
        assert_eq!(rows[2], [30, 20, 10, 255]);
    }
}

#[test]
fn straight_transparency_is_kept_with_the_colours_as_stored() {
    // a colour above its alpha cannot be premultiplied
    let rows = served(&bmp(&[[255, 255, 255, 0], [0, 50, 200, 128], [10, 20, 30, 255]], false));
    assert_eq!(rows[0][3], 0);
    assert_eq!(rows[1], [200, 50, 0, 128]);
}

#[test]
fn an_unused_fourth_byte_leaves_the_picture_opaque() {
    for fourth in [0, 255] {
        let rows = served(&bmp(&[[0, 0, 0, fourth], [10, 20, 30, fourth]], false));
        assert_eq!(rows, vec![[0, 0, 0, 255], [30, 20, 10, 255]], "fourth byte {fourth}");
    }
}
