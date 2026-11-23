//! Two metafile regressions found on the F5 and Hitachi drawings in the
//! visioStencils collection, both of which rendered blank or wrong:
//!   * an OLE-embedded object is shown as a preview image that is only reachable
//!     through the embedded part's own relationships;
//!   * `EXTSELECTCLIPRGN` rectangles are device units, not logical units.
use std::io::{Cursor, Write};

use layoutcraft_vsdx::emf::emf_to_svg_at;
use layoutcraft_vsdx::{import_document, Package};
use zip::write::SimpleFileOptions;

const SIGNATURE: u32 = 0x464D_4520;

fn rec(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut size = 8 + payload.len();
    let pad = (4 - size % 4) % 4;
    size += pad;
    let mut out = Vec::new();
    out.extend(kind.to_le_bytes());
    out.extend((size as u32).to_le_bytes());
    out.extend(payload);
    out.extend(std::iter::repeat(0u8).take(pad));
    out
}

fn header(w: i32, h: i32) -> Vec<u8> {
    let mut p = Vec::new();
    for v in [0, 0, w - 1, h - 1] {
        p.extend(v.to_le_bytes());
    }
    for v in [0, 0, w * 10, h * 10] {
        p.extend(v.to_le_bytes());
    }
    p.extend(SIGNATURE.to_le_bytes());
    p.extend(0x0001_0000u32.to_le_bytes());
    p.extend(0u32.to_le_bytes());
    p.extend(0u32.to_le_bytes());
    p.extend(8u16.to_le_bytes());
    p.extend(0u16.to_le_bytes());
    p.extend([0u8; 12]);
    for v in [w, h, w / 10, h / 10] {
        p.extend(v.to_le_bytes());
    }
    rec(1, &p)
}

fn polygon(points: &[(i16, i16)]) -> Vec<u8> {
    let mut p = vec![0u8; 16];
    p.extend((points.len() as u32).to_le_bytes());
    for (x, y) in points {
        p.extend(x.to_le_bytes());
        p.extend(y.to_le_bytes());
    }
    rec(86, &p)
}

fn tiny_emf() -> Vec<u8> {
    let mut emf = header(100, 50);
    emf.extend(polygon(&[(0, 0), (60, 0), (60, 30)]));
    emf.extend(rec(14, &[0u8; 12]));
    emf
}

#[test]
fn region_clip_rectangles_are_device_units() {
    // Logical -> device scale 0.1: a logical point (500, 500) lands at (50, 50).
    let mut emf = header(1000, 500);
    emf.extend(rec(17, &8u32.to_le_bytes())); // MM_ANISOTROPIC
    emf.extend(rec(9, &[0xe8, 0x03, 0, 0, 0xe8, 0x03, 0, 0])); // window ext 1000 x 1000
    emf.extend(rec(11, &[100, 0, 0, 0, 100, 0, 0, 0])); // viewport ext 100 x 100
    // EXTSELECTCLIPRGN, RGN_COPY, one device rectangle (0,0)-(80,40).
    let mut region = Vec::new();
    region.extend(48u32.to_le_bytes()); // cbRgnData
    region.extend(5u32.to_le_bytes()); // RGN_COPY
    for v in [32u32, 1, 1, 16] {
        region.extend(v.to_le_bytes()); // RGNDATAHEADER: size, type, count, rgn size
    }
    for v in [0i32, 0, 80, 40, 0, 0, 80, 40] {
        region.extend(v.to_le_bytes()); // bounds, then the one rectangle
    }
    emf.extend(rec(75, &region));
    emf.extend(polygon(&[(0, 0), (900, 0), (900, 900)]));
    emf.extend(rec(14, &[0u8; 12]));
    let svg = emf_to_svg_at(&emf, None).expect("converted").svg;
    assert!(
        svg.contains("M0.00 0.00L80.00 0.00L80.00 40.00L0.00 40.00Z"),
        "clip must be the device rectangle unchanged: {svg}"
    );
    // The polygon itself still goes through the mapping.
    assert!(svg.contains("M0.00 0.00L90.00 0.00L90.00 90.00Z"), "{svg}");
}

fn package_with_ole_preview(with_image_rel: bool) -> Package {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default();
        let mut put = |name: &str, body: &[u8]| {
            zip.start_file(name, options).unwrap();
            zip.write_all(body).unwrap();
        };
        put("[Content_Types].xml", b"<?xml version='1.0'?><Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'/>");
        put("_rels/.rels", b"<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'/>");
        put("visio/document.xml", b"<?xml version='1.0'?><VisioDocument xmlns='http://schemas.microsoft.com/office/visio/2012/main'/>");
        put("visio/_rels/document.xml.rels", b"<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/pages' Target='pages/pages.xml'/></Relationships>");
        put("visio/pages/pages.xml", b"<?xml version='1.0'?><Pages xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships'><Page ID='0' NameU='Page-1'><PageSheet><Cell N='PageWidth' V='8.5'/><Cell N='PageHeight' V='11'/></PageSheet><Rel r:id='rId1'/></Page></Pages>");
        put("visio/pages/_rels/pages.xml.rels", b"<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/page' Target='page1.xml'/></Relationships>");
        put(
            "visio/pages/page1.xml",
            b"<?xml version='1.0'?><PageContents xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships'><Shapes><Shape ID='1' Type='Foreign'><Cell N='PinX' V='3'/><Cell N='PinY' V='4'/><Cell N='Width' V='2'/><Cell N='Height' V='1'/><ForeignData ForeignType='Object' ObjectType='33280'><Rel r:id='rId1'/></ForeignData></Shape></Shapes></PageContents>",
        );
        // The shape's relationship points at the embedded object ...
        put("visio/pages/_rels/page1.xml.rels", b"<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject' Target='../embeddings/object1.vsd'/></Relationships>");
        put("visio/embeddings/object1.vsd", b"not a real embedded drawing");
        // ... whose own relationships name the preview image.
        if with_image_rel {
            put("visio/embeddings/_rels/object1.vsd.rels", b"<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/image' Target='../media/image1.emf'/></Relationships>");
        }
        put("visio/media/image1.emf", &tiny_emf());
        zip.finish().unwrap();
    }
    Package::from_reader(Cursor::new(buffer)).unwrap()
}

#[test]
fn ole_object_shows_its_preview_image() {
    let document = import_document(&package_with_ole_preview(true)).expect("imports");
    let shape = &document.pages[0].shapes[0];
    let image = shape.image.as_ref().expect("the OLE shape becomes a picture");
    assert_eq!(image.media_type.as_deref(), Some("image/svg+xml"), "preview EMF is converted");
    assert!(!image.unsupported);
    let record = document.media.values().next().expect("a media record");
    assert_eq!(record.part.as_deref(), Some("visio/media/image1.emf"));
    assert_eq!(record.ext.as_deref(), Some("emf"));
}

#[test]
fn ole_object_without_a_preview_stays_an_opaque_placeholder() {
    let document = import_document(&package_with_ole_preview(false)).expect("imports");
    let shape = &document.pages[0].shapes[0];
    // No image relationship: the embedded object itself is the "media".
    let record = document.media.values().next().expect("a media record");
    assert_eq!(record.part.as_deref(), Some("visio/embeddings/object1.vsd"));
    assert_ne!(record.media_type.as_deref(), Some("image/svg+xml"));
    let _ = shape;
}
