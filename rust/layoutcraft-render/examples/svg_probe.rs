//! Rasterise an SVG file with resvg and print the mean darkness of the middle
//! row band, to probe renderer support for SVG features.
//! usage: svg_probe <in.svg> [out.png]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).unwrap();
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&data, &options).unwrap();
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let mut dark = 0u64;
    for p in pixmap.pixels() {
        dark += 255 - ((p.red() as u64 + p.green() as u64 + p.blue() as u64) / 3);
    }
    println!("{}x{} total darkness {}", size.width(), size.height(), dark);
    if let Some(out) = args.get(2) {
        pixmap.save_png(out).unwrap();
    }
}
