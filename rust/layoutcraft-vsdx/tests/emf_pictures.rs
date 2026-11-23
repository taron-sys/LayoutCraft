//! EMF pictures (Supermicro drawings from the examples survey): converted to
//! SVG so they render, with the shape's hidden label left hidden.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_vsdx::emf::emf_to_svg;
use layoutcraft_vsdx::{import_document, media_bytes, Package};

fn open(name: &str) -> Package {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/Supermicro")
        .join(name);
    Package::open(path).unwrap()
}

#[test]
fn real_emf_converts_to_a_drawable_svg() {
    let package = open("ARS-110M-NR.vsdx");
    let emf = package.entry("visio/media/image1.emf").expect("emf part");
    let svg = emf_to_svg(emf).expect("converted");
    // 830 polygons and 116 polylines in the source.
    assert!(svg.svg.matches("<path ").count() > 800, "paths: {}", svg.svg.matches("<path ").count());
    assert!(svg.svg.contains("viewBox=\"0.00 0.00 1144.00 104.00\""), "{}", &svg.svg[..200]);
    // 48.28 mm x 4.35 mm at 96 dpi.
    assert!((svg.width_px - 182.5).abs() < 0.5, "{}", svg.width_px);
    // The result is well-formed SVG that a parser accepts.
    assert!(roxmltree::Document::parse(&svg.svg).is_ok());
}

#[test]
fn emf_media_is_marked_renderable_and_bytes_are_svg() {
    let package = open("5MW Cooling Tower(Side).vsdx");
    let document = import_document(&package).unwrap();
    let record = document
        .media
        .values()
        .find(|media| media.ext.as_deref() == Some("emf"))
        .expect("an EMF media record");
    assert_eq!(record.media_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(record.unsupported, Some(false));
    assert!(record.width.unwrap_or(0) > 100);
    let image = document.pages[0]
        .shapes
        .iter()
        .find_map(|shape| shape.image.as_ref())
        .expect("a picture shape");
    assert!(!image.unsupported);
    let bytes = media_bytes(&package, record).expect("bytes");
    assert!(bytes.starts_with(b"<svg"));
}

#[test]
fn hidden_group_label_is_not_shown() {
    // The master group sets HideText (cached 1) so the model name is hidden.
    let document = import_document(&open("ARS-110M-NR.vsdx")).unwrap();
    for shape in &document.pages[0].shapes {
        assert!(shape.label.is_empty(), "label {:?} on {:?}", shape.label, shape.visio_id);
    }
}

#[test]
fn the_header_size_of_an_emf_equals_the_size_of_its_conversion() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Electronics/Hikvision v2017.vsdx");
    if !path.exists() {
        return;
    }
    let package = Package::open(path).unwrap();
    let mut compared = 0;
    for name in package.names() {
        if !name.to_ascii_lowercase().ends_with(".emf") {
            continue;
        }
        let bytes = package.entry(name).unwrap();
        if !layoutcraft_vsdx::emf::is_enhanced_metafile(bytes) {
            continue;
        }
        let converted = layoutcraft_vsdx::emf::metafile_to_svg_at(bytes, None);
        let header = layoutcraft_vsdx::emf::emf_natural_size(bytes);
        assert_eq!(converted.as_ref().map(|s| (s.width_px, s.height_px)), header, "{name}");
        compared += 1;
    }
    assert!(compared >= 3, "{compared} EMF parts compared");
}

/// The PDU83107 labels are EMF text in "Gotham Medium", a face most machines lack: the SVG names a sans-serif fallback (Visio's
/// export shows a thin sans there, not the browser's serif).
#[test]
fn an_emf_face_that_is_not_installed_falls_back_to_a_sans_serif() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../visio-test-runs/pdu83107/drop.vsdx");
    let Ok(package) = Package::open(path) else { return };
    let emf = package.entry("visio/media/image7.emf").expect("label emf");
    let svg = emf_to_svg(emf).expect("converted").svg;
    assert!(svg.contains("font-family=\"Gotham Medium, Helvetica, Arial, sans-serif\""), "{}", &svg[..svg.len().min(400)]);
}
