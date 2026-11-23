#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, SceneItem};
use layoutcraft_vsdx::{import_document, Package};

/// computer-network.vsdx lists its connectors BEFORE the device shapes in the page's shape tree, and Visio draws in tree order: its
/// export has the lines stop at the device pictures. Shapes and connectors share one drawing order (`Connector.order_z`).
#[test]
fn connectors_listed_before_the_shapes_are_drawn_below_them() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/computer-network.vsdx");
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    let scene = scene_from_document(&document, 0).unwrap();
    let last_connector = scene.items.iter().rposition(|item| matches!(item, SceneItem::Connector(_))).expect("connectors");
    let first_shape = scene.items.iter().position(|item| matches!(item, SceneItem::Shape(_))).expect("shapes");
    assert!(last_connector < first_shape, "connectors at {last_connector}, first shape at {first_shape}");
}
