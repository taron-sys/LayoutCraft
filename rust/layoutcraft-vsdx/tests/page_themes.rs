//! A theme per page (step 272, probe-page-themes.ps1): Visio writes one theme part per page theme (theme1.xml, theme2.xml, both related from
//! document.xml.rels) and ties a page to its part through the page sheet's `ThemeIndex` = the part's `schemeEnum`, not through the part order.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::io::Cursor;

use layoutcraft_vsdx::{import_document, Package};

fn open(bytes: &[u8]) -> Package {
    Package::from_reader(Cursor::new(bytes.to_vec())).unwrap()
}

#[test]
fn each_page_resolves_its_own_theme() {
    // two pages, a rectangle each: page 1 Integral (ThemeIndex 36), page 2 Slice (42); Visio resolved RGB(0; 131; 185) and RGB(20; 97; 148)
    let package = open(include_bytes!("../../../test/corpus/page-themes.vsdx"));
    let document = import_document(&package).unwrap();
    assert_eq!(document.pages.len(), 2);
    let fill = |page: usize, text: &str| document.pages[page].shapes.iter().find(|s| s.label == text).and_then(|s| s.fill.clone());
    assert_eq!(fill(0, "P1").as_deref(), Some("#0083b9"));
    assert_eq!(fill(1, "P2").as_deref(), Some("#146194"));
}

// applies the private extra themes (private module, see private/README.md)
#[cfg(private_themes)]
#[test]
fn a_theme_applied_to_one_page_changes_that_page_only() {
    use layoutcraft_vsdx::pageops::{apply_theme, apply_theme_page, page_themes};
    let bytes = include_bytes!("../../../test/corpus/page-themes.vsdx");
    let package = open(bytes);
    assert_eq!(page_themes(&package).iter().map(|t| t.0).collect::<Vec<_>>(), vec![Some(36), Some(42)]);
    // page 1 gets Whiteboard (58): a third theme part appears, page 2 keeps Slice
    let changed = open(&apply_theme_page(&package, "Whiteboard", Some(0)).unwrap());
    assert_eq!(page_themes(&changed).iter().map(|t| t.0).collect::<Vec<_>>(), vec![Some(58), Some(42)]);
    let parts = changed.names().filter(|n| n.starts_with("visio/theme/theme")).count();
    assert_eq!(parts, 2, "Integral is no page's theme any more and its part is dropped; Whiteboard is a new part beside Slice");
    assert!(changed.text("[Content_Types].xml").unwrap().matches("theme+xml").count() == 2 && !changed.text("visio/_rels/document.xml.rels").unwrap().contains("theme/theme1.xml"), "content type and relationship follow");
    let fills = |p: &Package| {
        let d = import_document(p).unwrap();
        let f = |page: usize, text: &str| d.pages[page].shapes.iter().find(|s| s.label == text).and_then(|s| s.fill.clone());
        (f(0, "P1"), f(1, "P2"))
    };
    let (p1, p2) = fills(&changed);
    assert_eq!(p2.as_deref(), Some("#146194"), "page 2 is still Slice");
    // the same theme applied to every page gives the same page-1 fill
    let all = open(&apply_theme(&package, "Whiteboard").unwrap());
    assert_eq!(p1, fills(&all).0);
    assert_ne!(p1.as_deref(), Some("#0083b9"), "page 1 is not Integral any more");
    // applying a theme that exists already adds no part (and Integral is unused now: one part is left)
    let same = open(&apply_theme_page(&package, "Slice", Some(0)).unwrap());
    assert_eq!(same.names().filter(|n| n.starts_with("visio/theme/theme")).count(), 1);
    assert_eq!(fills(&same).0.as_deref(), Some("#146194"));
}

#[test]
fn a_page_without_a_theme_looks_as_if_the_drawing_had_none() {
    // probe-page-themes-one.ps1: the theme was applied to page 1 only; page 2 has ThemeIndex 0 and its fill is not themed (Visio: the plain default)
    let package = open(include_bytes!("../../../test/corpus/page-themes-one.vsdx"));
    let d = import_document(&package).unwrap();
    let fill = |page: usize, text: &str| d.pages[page].shapes.iter().find(|s| s.label == text).and_then(|s| s.fill.clone());
    assert_eq!(fill(0, "P1").as_deref(), Some("#0083b9"));
    assert_ne!(fill(1, "P2").as_deref(), Some("#0083b9"), "page 2 does not follow page 1's theme");
}
