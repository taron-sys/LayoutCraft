//! Document properties: core.xml / app.xml elements are set, replaced and cleared; the rest of both parts and the
//! drawing stay as they were; parts that are missing are created with relationship and content type.
use std::io::{Cursor, Read, Write};

use layoutcraft_vsdx::docprops::{read_doc_props, write_doc_props, DocProps};
use layoutcraft_vsdx::{import_document, write_blank_document, BlankOptions, Package};

fn blank_bytes() -> Vec<u8> {
    write_blank_document(&BlankOptions { width_in: 8.5, height_in: 11.0, metric: false, name: "Page-1".to_string() }).unwrap()
}

fn blank() -> Package {
    Package::from_bytes(&blank_bytes()).unwrap()
}

fn full() -> DocProps {
    DocProps {
        title: "Plan <A&B>".into(),
        subject: "Subject".into(),
        creator: "Ann".into(),
        keywords: "k1; k2".into(),
        description: "line one\nline two".into(),
        category: "Cat".into(),
        manager: "Mgr".into(),
        company: "Comp & Co".into(),
    }
}

#[test]
fn properties_round_trip_with_escaping() {
    let package = blank();
    let bytes = write_doc_props(&package, &full()).unwrap();
    let again = Package::from_bytes(&bytes).unwrap();
    assert_eq!(read_doc_props(&again), full());
    let core = again.text("docProps/core.xml").unwrap();
    assert!(core.contains("<dc:title>Plan &lt;A&amp;B&gt;</dc:title>"), "{core}");
    assert!(core.contains("<cp:keywords>k1; k2</cp:keywords>"));
    assert!(again.text("docProps/app.xml").unwrap().contains("<Company>Comp &amp; Co</Company>"));
}

#[test]
fn changing_and_clearing_keeps_the_rest() {
    let first = Package::from_bytes(&write_doc_props(&blank(), &full()).unwrap()).unwrap();
    let mut props = full();
    props.title = "New".into();
    props.subject = String::new();
    props.company = String::new();
    let second = Package::from_bytes(&write_doc_props(&first, &props).unwrap()).unwrap();
    assert_eq!(read_doc_props(&second), props);
    let core = second.text("docProps/core.xml").unwrap();
    assert!(!core.contains("dc:subject"), "an empty value removes the element: {core}");
    assert_eq!(core.matches("<dc:title>").count(), 1);
    assert!(core.contains("dcterms:created"), "other core properties stay: {core}");
    assert!(second.text("docProps/app.xml").unwrap().contains("<Application>"), "other app properties stay");
}

#[test]
fn the_drawing_is_unchanged() {
    let package = blank();
    let before = serde_json::to_string(&import_document(&package).unwrap()).unwrap();
    let after = Package::from_bytes(&write_doc_props(&package, &full()).unwrap()).unwrap();
    assert_eq!(serde_json::to_string(&import_document(&after).unwrap()).unwrap(), before);
}

#[test]
fn too_long_or_control_characters_are_refused() {
    let package = blank();
    let mut props = DocProps::default();
    props.title = "x".repeat(5000);
    assert!(write_doc_props(&package, &props).is_err());
    props.title = "bad\u{1}".into();
    assert!(write_doc_props(&package, &props).is_err());
}

/// Rebuild a package without its docProps parts (and without their relationships and overrides).
fn strip_doc_props(bytes: &[u8]) -> Package {
    let mut source = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..source.len() {
        let mut file = source.by_index(i).unwrap();
        let name = file.name().unwrap().to_string();
        if name.starts_with("docProps/") {
            continue;
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if name == "_rels/.rels" || name == "[Content_Types].xml" {
            let text = String::from_utf8(data).unwrap();
            let kept: Vec<&str> = text.split("/>").filter(|piece| !piece.contains("docProps")).collect();
            data = kept.join("/>").into_bytes();
            if !String::from_utf8_lossy(&data).trim_end().ends_with('>') {
                data.extend_from_slice(b"/>");
            }
        }
        out.start_file(name, options).unwrap();
        out.write_all(&data).unwrap();
    }
    Package::from_bytes(&out.finish().unwrap().into_inner()).unwrap()
}

#[test]
fn missing_parts_are_created_with_relationship_and_content_type() {
    let stripped = strip_doc_props(&blank_bytes());
    assert!(!stripped.contains("docProps/core.xml"));
    let written = Package::from_bytes(&write_doc_props(&stripped, &full()).unwrap()).unwrap();
    assert_eq!(read_doc_props(&written), full());
    assert!(written.text("_rels/.rels").unwrap().contains("docProps/core.xml"));
    assert!(written.text("[Content_Types].xml").unwrap().contains("/docProps/app.xml"));
}
