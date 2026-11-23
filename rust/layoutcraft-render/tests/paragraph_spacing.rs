#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `paragraph-spacing.vsdx` (written by the engine, make-linespacing.mjs) has seven boxes of three paragraphs. Visio's PNG export
/// of it, measured at 96 dpi: the baseline distance of the lines is 28.5 px with 20 pt before OR after, 55.5 px with 20 pt before
/// AND after, 28.5 px with 10 + 10 pt, and about 59.5 px with 1.5 line spacing and 30 pt before.
#[test]
fn paragraph_spacing_moves_the_lines_like_visio() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/paragraph-spacing.vsdx");
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&document, 0).unwrap());
    // baselines per box (the x of the text identifies the box)
    let mut boxes: Vec<(f64, Vec<f64>)> = Vec::new();
    for part in svg.split("<text x=\"").skip(1) {
        let x: f64 = part.split('"').next().unwrap().parse().unwrap();
        let y: f64 = part.split("y=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        match boxes.iter_mut().find(|(bx, _)| (*bx - x).abs() < 1.0) {
            Some((_, ys)) => ys.push(y),
            None => boxes.push((x, vec![y])),
        }
    }
    let pitch = |index: usize| {
        let ys = &boxes[index].1;
        assert_eq!(ys.len(), 3, "box {index} has three lines");
        (ys[2] - ys[0]) / 2.0
    };
    assert!((pitch(0) - 28.5).abs() < 1.5, "20 pt before: {}", pitch(0));
    assert!((pitch(1) - 28.5).abs() < 1.5, "20 pt after: {}", pitch(1));
    assert!((pitch(2) - 55.5).abs() < 2.0, "20 pt before and after: {}", pitch(2));
    assert!((pitch(3) - 28.5).abs() < 1.5, "10 + 10 pt: {}", pitch(3));
    assert!((pitch(6) - 59.5).abs() < 2.5, "1.5 lines and 30 pt before: {}", pitch(6));
}
