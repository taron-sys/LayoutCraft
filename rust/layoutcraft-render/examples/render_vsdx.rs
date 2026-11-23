//! Render a page directly from a VSDX/VSTX package, including picture bytes.
//!
//! usage: render_vsdx <source.vsdx> <page-index> <out.svg|out.png> [scale]
//!
//! Set LAYOUTCRAFT_PRINT=1 to leave out shapes on non-printing layers, which is what
//! Visio's `Page.Export` does (the survey comparisons use this).
use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use layoutcraft_render::{render_png, scene_from_document_with_options, to_svg, SceneOptions};
use layoutcraft_vsdx::{import_document, media_bytes, Package};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: render_vsdx <source.vsdx> <page-index> <out.svg|out.png> [scale]");
        std::process::exit(2);
    }
    let package = match Package::open(&args[1]) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("could not open {}: {error}", args[1]);
            std::process::exit(1);
        }
    };
    let document = match import_document(&package) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("could not import {}: {error}", args[1]);
            std::process::exit(1);
        }
    };
    let page_index = match args[2].parse::<usize>() {
        Ok(index) => index,
        Err(_) => {
            eprintln!("page index must be a non-negative integer");
            std::process::exit(2);
        }
    };
    let mut media = BTreeMap::new();
    for (id, record) in &document.media {
        if let Some(bytes) = media_bytes(&package, record) {
            media.insert(id.clone(), bytes);
        }
    }
    let options = SceneOptions {
        print: env::var("LAYOUTCRAFT_PRINT").is_ok_and(|v| v == "1"),
        fit_text: env::var("LAYOUTCRAFT_FITTEXT").is_ok_and(|v| v == "1"),
    };
    let scene = match scene_from_document_with_options(&document, page_index, &media, options) {
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
    } else if let Err(error) = std::fs::write(output, to_svg(&scene)) {
        eprintln!("could not write {}: {error}", output.display());
        std::process::exit(1);
    }
}
