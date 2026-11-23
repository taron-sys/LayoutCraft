#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_vsdx::{import_document, Package};

#[test]
fn ecommerce_connectors_use_the_connector_scheme_and_text_pins() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package = Package::open(
        root.join("examples/E-Commerce.Shopping.Process.Flow.Diagram.Visio.1.1.vsdx"),
    )
    .unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[0];
    assert!(!page.connectors.is_empty());
    for connector in &page.connectors {
        assert_ne!(
            connector.stroke.as_deref(),
            Some("#ffffff"),
            "connector {} lost its line colour",
            connector.visio_id.as_deref().unwrap_or("?")
        );
    }
    let yes = page
        .connectors
        .iter()
        .find(|connector| connector.label.trim() == "Yes" && connector.label_point.is_some())
        .expect("top Yes connector");
    let point = yes.label_point.as_ref().unwrap();
    assert!(point["x"].as_f64().unwrap() > 500.0);
    assert!(point["y"].as_f64().unwrap() < 80.0);

    let page = &document.pages[1];
    let confirmation = page
        .connectors
        .iter()
        .find(|connector| connector.label.contains("Validation"))
        .expect("validation confirmation connector");
    assert!(confirmation.label_point.is_some());
}
