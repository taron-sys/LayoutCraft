#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{render_png, scene_from_document};
use layoutcraft_vsdx::{import_document, Package};

/// `gradient-types.vsdx` (engine-written, make-gradient-types.mjs): fourteen 2 x 1.5 in boxes with a white -> blue gradient of the
/// types 0..13, 0.3 in apart, four per row (the first row of the drawing, types 0-3, is the bottom row of the picture), no outline.
/// `gradient-types-visio.png` is Visio's own export of it at 96 dpi. A plain two-stop gradient over the whole range is blended in
/// linear light along a smoothstep, over a ramp 2.5 % longer than the shape on each side.
#[test]
fn every_gradient_type_matches_visios_export() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus");
    let document = import_document(&Package::open(root.join("gradient-types.vsdx")).unwrap()).unwrap();
    let ours = image::load_from_memory(&render_png(&scene_from_document(&document, 0).unwrap(), 1.0).unwrap()).unwrap().to_rgb8();
    let visio = image::open(root.join("gradient-types-visio.png")).unwrap().to_rgb8();
    let mut worst = (0.0f64, 0);
    for dir in 0..14u32 {
        let (col, row_top) = (dir % 4, 3 - dir / 4);
        let (x0, y0) = ((col as f64 * 2.3 * 96.0).round() as u32 + 4, (row_top as f64 * 1.8 * 96.0).round() as u32 + 4);
        let (mut total, mut n) = (0.0, 0.0);
        for y in (y0..y0 + 136).step_by(3) {
            for x in (x0..x0 + 184).step_by(3) {
                let (p, q) = (visio.get_pixel(x, y), ours.get_pixel(x, y));
                total += (0..3).map(|c| (p[c] as f64 - q[c] as f64).abs()).sum::<f64>() / 3.0;
                n += 1.0;
            }
        }
        let mean = total / n;
        if mean > worst.0 {
            worst = (mean, dir);
        }
        assert!(mean < 8.0, "gradient type {dir}: mean difference {mean:.1} of 255");
    }
    eprintln!("worst gradient type {} at {:.1}", worst.1, worst.0);
}
