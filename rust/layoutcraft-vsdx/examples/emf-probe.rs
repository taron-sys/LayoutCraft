//! usage: emf-probe <package> <part> [out.svg] [display-width-px]
//! Convert one metafile part and report; optionally write the SVG.
use layoutcraft_vsdx::emf::metafile_to_svg_at;
use layoutcraft_vsdx::Package;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let package = Package::open(&args[1]).expect("open");
    let bytes = package.entry(&args[2]).expect("part");
    let display = args.get(4).and_then(|v| v.parse::<f64>().ok());
    let started = std::time::Instant::now();
    match metafile_to_svg_at(bytes, display) {
        Some(svg) => {
            println!(
                "OK {} chars, {:.0}x{:.0} px, {} paths, {} images, {} ms",
                svg.svg.len(),
                svg.width_px,
                svg.height_px,
                svg.svg.matches("<path ").count(),
                svg.svg.matches("<image ").count(),
                started.elapsed().as_millis()
            );
            if let Some(out) = args.get(3) {
                std::fs::write(out, svg.svg).unwrap();
            }
        }
        None => println!("NONE"),
    }
}
