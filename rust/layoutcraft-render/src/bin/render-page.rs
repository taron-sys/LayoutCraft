use std::env;
use std::path::Path;
use layoutcraft_model::Document;
use layoutcraft_render::{render_png, scene_from_document, to_svg};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: render-page <model.json> <page-index> <out.svg|out.png> [scale]");
        std::process::exit(2);
    }
    let page_index = match args[2].parse::<usize>() {
        Ok(index) => index,
        Err(_) => {
            eprintln!("page index must be a non-negative integer");
            std::process::exit(2);
        }
    };
    let document = match Document::open(&args[1]) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("could not open {}: {error}", args[1]);
            std::process::exit(1);
        }
    };
    let scene = match scene_from_document(&document, page_index) {
        Ok(scene) => scene,
        Err(error) => {
            eprintln!("could not build scene: {error}");
            std::process::exit(1);
        }
    };
    let output = Path::new(&args[3]);
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension == "png" {
        let scale = args
            .get(4)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(1.0);
        match render_png(&scene, scale) {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(output, bytes) {
                    eprintln!("could not write {}: {error}", output.display());
                    std::process::exit(1);
                }
            }
            Err(error) => {
                eprintln!("raster failed: {error}");
                std::process::exit(1);
            }
        }
    } else {
        if let Err(error) = std::fs::write(output, to_svg(&scene)) {
            eprintln!("could not write {}: {error}", output.display());
            std::process::exit(1);
        }
    }
}
