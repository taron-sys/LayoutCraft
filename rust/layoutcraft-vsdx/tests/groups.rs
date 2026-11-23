#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use serde_json::Value;
use layoutcraft_model::Document;
use layoutcraft_vsdx::{import_document, Package};

/// Pixel tolerance: members may overhang a Visio group frame by rounding.
const TOL: f64 = 1.0;

/// Corners of a shape box (x/y top-left of the unrotated box, `rot` in degrees,
/// the importer's screen convention) in page pixels.
fn corners(x: f64, y: f64, w: f64, h: f64, rot: f64) -> [(f64, f64); 4] {
    let angle = -rot.to_radians();
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let mut out = [(0.0, 0.0); 4];
    for (i, (sx, sy)) in [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)].iter().enumerate() {
        let (ox, oy) = (sx * w / 2.0, sy * h / 2.0);
        let (rx, ry) = (ox * angle.cos() - oy * angle.sin(), ox * angle.sin() + oy * angle.cos());
        out[i] = (cx + rx, cy - ry);
    }
    out
}

fn import(name: &str) -> Document {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(name);
    import_document(&Package::open(path).unwrap()).unwrap()
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Every group entry must agree with the shapes it claims as members and with
/// its parent entry; frames must cover every member box.
fn check_groups(doc: &Document) -> usize {
    let mut total = 0;
    for page in &doc.pages {
        for group in &page.groups {
            total += 1;
            let id = text(group, "visioId").expect("visioId");
            let members = group["members"].as_array().unwrap();
            for member in members {
                let member = member.as_str().unwrap();
                let shape = page
                    .shapes
                    .iter()
                    .find(|s| s.visio_id.as_deref() == Some(member))
                    .unwrap_or_else(|| panic!("member {member} of group {id} is not a shape"));
                let owner = shape.group.as_ref().and_then(|g| g.get("id")).and_then(Value::as_str);
                assert_eq!(owner, Some(id), "member {member} names its group");
                let frame = &group["frame"];
                let bounds = &group["bounds"];
                let f = |key: &str| frame[key].as_f64().unwrap();
                let (fx, fy, fw, fh, frot) = (f("x"), f("y"), f("w"), f("h"), f("rot"));
                let (cx, cy) = (fx + fw / 2.0, fy + fh / 2.0);
                let frame_angle = -frot.to_radians();
                for (px, py) in corners(shape.x, shape.y, shape.w, shape.h, shape.rot) {
                    // Into the frame's own axes (y up), where it is an upright box.
                    let (dx, dy) = (px - cx, -(py - cy));
                    let (lx, ly) = (
                        dx * frame_angle.cos() + dy * frame_angle.sin(),
                        -dx * frame_angle.sin() + dy * frame_angle.cos(),
                    );
                    assert!(
                        lx.abs() <= fw / 2.0 + TOL && ly.abs() <= fh / 2.0 + TOL,
                        "member {member} corner ({px:.1},{py:.1}) is outside the oriented frame of group {id}"
                    );
                    // And inside the axis-aligned bounds.
                    let b = |key: &str| bounds[key].as_f64().unwrap();
                    assert!(
                        px >= b("x") - TOL && px <= b("x") + b("w") + TOL
                            && py >= b("y") - TOL && py <= b("y") + b("h") + TOL,
                        "member {member} is outside the bounds of group {id}"
                    );
                }
            }
            match text(group, "parentId") {
                Some(parent) => {
                    let parent_entry = page
                        .groups
                        .iter()
                        .find(|g| text(g, "visioId") == Some(parent))
                        .expect("parent group exists");
                    assert_eq!(
                        group["depth"].as_u64().unwrap(),
                        parent_entry["depth"].as_u64().unwrap() + 1
                    );
                    assert_eq!(text(group, "rootId"), text(parent_entry, "rootId"));
                }
                None => {
                    assert_eq!(group["depth"].as_u64(), Some(1));
                    assert_eq!(text(group, "rootId"), Some(id));
                }
            }
        }
        // Every shape that claims a group is listed as a member of exactly it.
        for shape in &page.shapes {
            if let Some(owner) = shape.group.as_ref().and_then(|g| g.get("id")).and_then(Value::as_str) {
                let entry = page
                    .groups
                    .iter()
                    .find(|g| text(g, "visioId") == Some(owner))
                    .unwrap_or_else(|| panic!("group {owner} has no entry"));
                let listed = entry["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m.as_str() == shape.visio_id.as_deref());
                assert!(listed, "group {owner} lists {:?}", shape.visio_id);
            }
        }
    }
    total
}

#[test]
fn nested_groups_have_identity_parentage_and_frames() {
    let doc = import("test/corpus/groups-nested.vsdx");
    let total = check_groups(&doc);
    assert!(total >= 2, "expected nested groups, got {total}");
    // At least one group is nested inside another.
    assert!(doc.pages[0].groups.iter().any(|g| g.get("parentId").is_some_and(|p| !p.is_null())));
}

#[test]
fn rotated_groups_and_containers_are_consistent() {
    let rotated = import("test/corpus/groups-rotated.vsdx");
    assert!(check_groups(&rotated) >= 1);
    // A rotated group keeps an oriented frame (non-zero rot) whose bounds are
    // larger than the frame itself.
    let turned = rotated.pages[0]
        .groups
        .iter()
        .find(|g| g["frame"]["rot"].as_f64().is_some_and(|r| r.abs() > 1.0))
        .expect("a rotated group frame");
    assert!(turned["bounds"]["w"].as_f64().unwrap() > turned["frame"]["w"].as_f64().unwrap() - 1e-6);
    let network = import("examples/basic-network-diagram.vsdx");
    assert!(check_groups(&network) >= 1);
    assert!(network.pages[0]
        .groups
        .iter()
        .any(|g| text(g, "kind") == Some("container")));
}

#[test]
fn pages_without_groups_have_none() {
    assert!(import("test/corpus/rich-text.vsdx").pages[0].groups.is_empty());
}
