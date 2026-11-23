#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `alttext.vsdx` is Visio-authored: one rectangle whose alt text ("Description") is `my alt`.
#[test]
fn alt_text_and_page_title_reach_the_svg() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/alttext.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 0).unwrap());
    assert!(svg.contains(r#"role="img" aria-labelledby="vl-title"><title id="vl-title">Page-1</title>"#), "{}", &svg[..300]);
    assert!(svg.contains(r#"role="img" aria-label="my alt"><title>my alt</title>"#));
}
