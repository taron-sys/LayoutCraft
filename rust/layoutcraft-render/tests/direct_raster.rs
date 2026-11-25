//! `render_rgba` (straight RGBA from the pixmap) must give the very pixels the PNG round trip it replaced gave: `render_png`, then a PNG decode.
use std::collections::BTreeMap;
use std::path::PathBuf;

use layoutcraft_render::{render_png, render_region_rgba, render_rgba, scene_from_document_with_media, Scene};
use layoutcraft_vsdx::{import_document, media_bytes, Package};

fn decode_png(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).expect("decode").to_rgba8();
    (image.width(), image.height(), image.into_raw())
}

/// Every page of every public fixture, at 1x and 2x.
#[test]
fn the_direct_picture_is_pixel_identical_to_the_png_round_trip_on_every_public_fixture_page() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/public/sources");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir).expect("fixture directory").filter_map(|entry| entry.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "vsdx")).collect();
    paths.sort();
    assert!(!paths.is_empty(), "no public fixtures found in {}", dir.display());
    let mut pages = 0;
    for path in paths {
        let package = Package::open(&path).expect("open fixture");
        let document = import_document(&package).expect("import fixture");
        let mut media = BTreeMap::new();
        for (id, record) in &document.media {
            if let Some(bytes) = media_bytes(&package, record) {
                media.insert(id.clone(), bytes);
            }
        }
        for index in 0..document.pages.len() {
            let scene = scene_from_document_with_media(&document, index, &media).expect("scene");
            for scale in [1.0, 2.0] {
                let old = decode_png(&render_png(&scene, scale).expect("png"));
                let new = render_rgba(&scene, scale).expect("rgba");
                assert_eq!((old.0, old.1), (new.0, new.1), "{} page {index} at {scale}x: size", path.display());
                let differing = old.2.iter().zip(&new.2).filter(|(a, b)| a != b).count();
                assert_eq!(differing, 0, "{} page {index} at {scale}x: {differing} differing bytes", path.display());
            }
            pages += 1;
        }
    }
    assert!(pages >= 7, "only {pages} pages compared");
}

fn scene_with_background(background: &str) -> Scene {
    Scene { fit_text: false, title: "t".into(), min_x: 0.0, min_y: 0.0, width: 8.0, height: 4.0, background: background.into(), items: vec![] }
}

/// A half transparent red page: tiny-skia keeps premultiplied pixels (128, 0, 0, 128); the straight bytes must be (255, 0, 0, 128), which is
/// what `egui::ColorImage::from_rgba_unmultiplied` expects, and what the PNG held.
#[test]
fn semi_transparent_pixels_come_out_straight_not_premultiplied() {
    let scene = scene_with_background("#ff000080");
    let (w, h, rgba) = render_rgba(&scene, 1.0).expect("rgba");
    assert_eq!((w, h), (8, 4));
    for px in rgba.chunks(4) {
        assert_eq!(px[3], 128, "alpha {px:?}");
        assert!(px[0] >= 254, "red must be demultiplied, got {px:?}");
        assert_eq!((px[1], px[2]), (0, 0), "{px:?}");
    }
    let old = decode_png(&render_png(&scene, 1.0).expect("png"));
    assert_eq!(old.2, rgba);
    let (_, _, region) = render_region_rgba(&scene, 1.0, [0.0, 0.0, 8.0, 4.0]).expect("region");
    assert_eq!(region, rgba, "the window path and the page path convert the same way");
}

#[test]
fn a_bad_scale_is_an_error_not_a_panic() {
    let scene = scene_with_background("#ffffff");
    assert!(render_rgba(&scene, 0.0).is_err());
    assert!(render_rgba(&scene, f64::NAN).is_err());
}
