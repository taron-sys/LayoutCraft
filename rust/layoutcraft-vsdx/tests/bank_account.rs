//! The ER "Bank Account database" example lost its ellipses and diamonds:
//!  * attribute groups own the ellipse Geometry and only their leader line is a child;
//!  * the ER "Relationship" master is a 2-D diamond, not a connector.
//! Visio's cell oracle reports 0 output issues for this file.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_vsdx::{import_document, Package};

fn bank() -> layoutcraft_model::Document {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/Bank Account database.vsdx");
    import_document(&Package::open(path).unwrap()).unwrap()
}

#[test]
fn attribute_groups_draw_their_ellipse_and_leader_line() {
    let document = bank();
    let shapes = &document.pages[0].shapes;
    let balance = shapes.iter().find(|s| s.label == "Balance").expect("Balance attribute");
    assert!(
        balance.parts.iter().any(|p| p.d.contains('A') && p.fill.as_deref() == Some("#4672c4")),
        "the blue ellipse must be drawn: {:?}",
        balance.parts.iter().map(|p| (&p.d, &p.fill)).collect::<Vec<_>>()
    );
    // The leader line is the group's child and is blue (QuickStyleVariation 4).
    assert!(balance.parts.iter().any(|p| p.stroke.as_deref() == Some("#4672c4") && p.d.starts_with('M') && !p.d.contains('A')));
}

#[test]
fn er_relationship_diamonds_are_shapes_not_connectors() {
    let document = bank();
    let shapes = &document.pages[0].shapes;
    for label in ["Contained", "Owned by", "Attached to"] {
        let diamond = shapes
            .iter()
            .find(|s| s.label.replace(['\u{200b}', '\u{2028}', '\n'], " ").contains(label))
            .unwrap_or_else(|| panic!("diamond {label} missing"));
        assert_eq!(diamond.fill.as_deref().or(diamond.parts.first().and_then(|p| p.fill.as_deref())), Some("#ed7d31"));
        assert_eq!(diamond.stroke.as_deref(), Some("#ffffff"), "Visio draws the diamonds' outline white");
    }
}
