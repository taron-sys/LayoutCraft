//! Renders rust/resources/layoutcraft.svg to resources/layoutcraft.ico (PNG entries of 16, 24, 32, 48, 64, 128 and 256 px) and resources/layoutcraft-256.png.
//! Run when the SVG changes:  cargo +stable-x86_64-pc-windows-gnu run -p layoutcraft-app --example make_icon
use std::path::PathBuf;

fn render(svg: &str, side: u32) -> Vec<u8> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).expect("svg");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side).expect("pixmap");
    let scale = side as f32 / tree.size().width();
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().expect("png")
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources");
    let svg = std::fs::read_to_string(dir.join("layoutcraft.svg")).expect("layoutcraft.svg");
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let pngs: Vec<Vec<u8>> = sizes.iter().map(|s| render(&svg, *s)).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (side, png) in sizes.iter().zip(&pngs) {
        ico.push(if *side >= 256 { 0 } else { *side as u8 });
        ico.push(if *side >= 256 { 0 } else { *side as u8 });
        ico.extend_from_slice(&[0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += png.len();
    }
    for png in &pngs {
        ico.extend_from_slice(png);
    }
    std::fs::write(dir.join("layoutcraft.ico"), &ico).expect("write ico");
    std::fs::write(dir.join("layoutcraft-256.png"), pngs.last().unwrap()).expect("write png");
    println!("wrote layoutcraft.ico ({} bytes, {} sizes) and layoutcraft-256.png", ico.len(), sizes.len());
}
