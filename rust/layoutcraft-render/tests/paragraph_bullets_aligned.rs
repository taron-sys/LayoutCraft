#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `paragraph-bullets-aligned.vsdx` (engine-written, visio-test-harness/make-bullets-aligned.mjs): six 2 in wide boxes, 0.2 in apart from the
/// left, centred / right-aligned paragraphs with a bullet. Visio's PNG export (192 dpi) puts the bullet's ink at 0.068 (centred, IndLeft 0.3 and
/// IndFirst -0.3), 0.13 (right), 0.016 (centred, no indent), 0.036 (right), 0.312 (centred, IndLeft 0.5 and IndFirst -0.25), 0.375 (right) in from
/// the box, i.e. one bullet gap before the first line's text, which is centred / right-aligned in the area right of the first line's start
/// (the bullet used to be an inline "• " in the line, and a centred paragraph with a bullet and no indent lost it to the left of the box).
#[test]
fn bullets_of_centred_and_right_aligned_paragraphs_sit_before_the_first_line_like_visio() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/paragraph-bullets-aligned.vsdx");
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&document, 0).unwrap());
    // the bullet <text> elements, in drawing order: (x)
    let mut bullets: Vec<f64> = Vec::new();
    for part in svg.split("<text x=\"").skip(1) {
        let (head, rest) = part.split_once('>').unwrap();
        if head.contains("text-anchor=\"start\"") && rest.trim_start().starts_with("<tspan") && part.split("</text>").next().unwrap().contains('•') {
            bullets.push(head.split('"').next().unwrap().parse().unwrap());
        }
    }
    assert_eq!(bullets.len(), 6, "{bullets:?}");
    // box k starts at 0.2 + 2.2 k inches (96 px per inch); Visio's ink positions less the glyph's side bearing (about 1.5 px)
    let want_in = [0.068, 0.13, 0.016, 0.036, 0.312, 0.375];
    for (k, (x, w)) in bullets.iter().zip(want_in).enumerate() {
        let inside = x - (0.2 + 2.2 * k as f64) * 96.0;
        let visio = w * 96.0 - 1.5;
        // 8 px: our text is a few per cent narrower than Visio's, which moves a centred / right-aligned line (and its bullet)
        assert!((inside - visio).abs() < 8.0, "box {k}: bullet at {inside:.1} px, Visio {visio:.1} px ({bullets:?})");
    }
}
