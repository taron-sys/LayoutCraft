//! Ruler guides: added, moved and deleted as `Type='Guide'` page shapes in Visio's form; the importer lists them in `page.guides`
//! (not as shapes) and a guide of a Visio-authored file can be moved and deleted.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_vsdx::guides::{add_guide, delete_guide, move_guide};
use layoutcraft_vsdx::{import_document, write_blank_document, BlankOptions, Package};

fn blank() -> Package {
    let bytes = write_blank_document(&BlankOptions { width_in: 8.5, height_in: 11.0, metric: false, name: "Main".to_string() }).unwrap();
    Package::from_bytes(&bytes).unwrap()
}

fn pkg(bytes: Vec<u8>) -> Package {
    Package::from_bytes(&bytes).unwrap()
}

fn guides(p: &Package) -> Vec<(String, f64, f64, f64)> {
    import_document(p).unwrap().pages[0].guides.iter().map(|g| (g.visio_id.clone().unwrap(), g.x_in, g.y_in, g.angle)).collect()
}

#[test]
fn added_guides_are_read_back_and_are_not_shapes() {
    let p = blank();
    let shapes = import_document(&p).unwrap().pages[0].shapes.len();
    let (b, h) = add_guide(&p, 0, false, 3.25).unwrap();
    let p = pkg(b);
    let (b, v) = add_guide(&p, 0, true, 5.5).unwrap();
    let p = pkg(b);
    assert_ne!(h, v);
    let g = guides(&p);
    assert_eq!(g.len(), 2);
    let hg = g.iter().find(|g| g.0 == h).unwrap();
    assert!((hg.2 - 3.25).abs() < 1e-9 && hg.3 == 0.0, "{hg:?}");
    let vg = g.iter().find(|g| g.0 == v).unwrap();
    assert!((vg.1 - 5.5).abs() < 1e-9 && (vg.3 - std::f64::consts::FRAC_PI_2).abs() < 1e-6, "{vg:?}");
    assert_eq!(import_document(&p).unwrap().pages[0].shapes.len(), shapes, "guides are page furniture, not shapes");
}

#[test]
fn move_and_delete() {
    let p = blank();
    let (b, h) = add_guide(&p, 0, false, 3.0).unwrap();
    let p = pkg(b);
    let (b, v) = add_guide(&p, 0, true, 2.0).unwrap();
    let p = pkg(b);
    let p = pkg(move_guide(&p, 0, &h, 4.5).unwrap());
    let p = pkg(move_guide(&p, 0, &v, 7.25).unwrap());
    let g = guides(&p);
    assert!((g.iter().find(|g| g.0 == h).unwrap().2 - 4.5).abs() < 1e-9);
    assert!((g.iter().find(|g| g.0 == v).unwrap().1 - 7.25).abs() < 1e-9);
    let p = pkg(delete_guide(&p, 0, &h).unwrap());
    let g = guides(&p);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].0, v);
    assert!(delete_guide(&p, 0, &h).is_err(), "gone already");
    assert!(delete_guide(&p, 0, "1").is_err() || guides(&p).len() == 1, "a shape that is not a guide is not deleted");
}

#[test]
fn refusals() {
    let p = blank();
    assert!(add_guide(&p, 0, true, f64::NAN).is_err());
    assert!(add_guide(&p, 3, true, 1.0).is_err());
    assert!(move_guide(&p, 0, "99", 1.0).is_err());
}

#[test]
fn guides_of_a_visio_file_can_be_moved_and_deleted() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/layers-guides.vsdx");
    let p = Package::open(path).unwrap();
    let before = guides(&p);
    assert_eq!(before.len(), 3);
    let horizontal = before.iter().find(|g| g.3 == 0.0 && g.2 == 3.0).expect("the horizontal ruler guide at y = 3").0.clone();
    let vertical = before.iter().find(|g| g.3 > 1.0).expect("the vertical ruler guide").0.clone();
    let p = pkg(move_guide(&p, 0, &horizontal, 6.0).unwrap());
    let p = pkg(move_guide(&p, 0, &vertical, 1.25).unwrap());
    let after = guides(&p);
    assert!((after.iter().find(|g| g.0 == horizontal).unwrap().2 - 6.0).abs() < 1e-9);
    assert!((after.iter().find(|g| g.0 == vertical).unwrap().1 - 1.25).abs() < 1e-9);
    let p = pkg(delete_guide(&p, 0, &horizontal).unwrap());
    assert_eq!(guides(&p).len(), 2);
}
