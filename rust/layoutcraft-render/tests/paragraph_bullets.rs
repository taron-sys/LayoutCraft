#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `paragraph-bullets.vsdx` (engine-written, make-bullets.mjs): seven 1.7 in wide boxes with one long left-aligned paragraph each.
/// Visio's PNG export (192 dpi) puts the ink at: bullet only 0 / text 0.573 in; IndLeft 0.3 + IndFirst -0.3 with a bullet: bullet 0,
/// text and continuation lines 0.3; IndLeft 0.5: every line 0.5; IndFirst 0.3: first line 0.3, others 0; IndLeft 0.5 + IndFirst
/// -0.25: bullet 0.25, text 0.5.
#[test]
fn bullets_and_hanging_indents_start_where_visio_starts_them() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/paragraph-bullets.vsdx");
    let document = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&document, 0).unwrap());
    // every <text>: (x, content)
    let mut texts: Vec<(f64, String)> = Vec::new();
    for part in svg.split("<text x=\"").skip(1) {
        let x: f64 = part.split('"').next().unwrap().parse().unwrap();
        let body = part.split('>').nth(1).unwrap_or("");
        let content: String = part.split("</text>").next().unwrap().split('>').skip(1).collect::<Vec<_>>().join(">");
        let _ = body;
        texts.push((x, content.replace("</tspan", "").replace("<tspan", "")));
    }
    let left = texts[0].0; // the first box: no indent
    let xs_in_box = |i: usize| -> Vec<(f64, String)> {
        let lo = left + 182.4 * i as f64 - 1.0;
        let hi = lo + 2.0 + 163.2;
        texts.iter().filter(|(x, _)| *x >= lo && *x < hi).map(|(x, c)| (x - left - 182.4 * i as f64, c.clone())).collect()
    };
    let close = |a: f64, b: f64| (a - b).abs() < 1.0;
    // bullet only: the bullet at 0, the first line's text 0.573 in later
    let b = xs_in_box(1);
    assert!(b[0].1.contains('•') && close(b[0].0, 0.0), "{b:?}");
    assert!(close(b[1].0, 55.0), "text after a bullet without indent: {b:?}");
    // hanging: bullet 0, text and the following lines 0.3 in
    let h = xs_in_box(2);
    assert!(close(h[0].0, 0.0) && close(h[1].0, 28.8) && close(h[2].0, 28.8), "{h:?}");
    // IndLeft 0.5: every line 0.5 in; IndFirst 0.3: first line 0.3 in, the others 0
    assert!(xs_in_box(3).iter().all(|(x, _)| close(*x, 48.0)), "{:?}", xs_in_box(3));
    let f = xs_in_box(4);
    assert!(close(f[0].0, 28.8) && close(f[1].0, 0.0), "{f:?}");
    // IndLeft 0.5 + IndFirst -0.25: bullet 0.25 in, text 0.5 in
    let g = xs_in_box(5);
    assert!(close(g[0].0, 24.0) && close(g[1].0, 48.0) && close(g[2].0, 48.0), "{g:?}");
}
