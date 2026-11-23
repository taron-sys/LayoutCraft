//! usage: add-shape <source.vsdx> <out.vsdx> <page-index> <shape-id> <text>
use std::env;
use layoutcraft_vsdx::{patch_page, NewShape, Package};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 6 {
        eprintln!("usage: add-shape <source.vsdx> <out.vsdx> <page-index> <shape-id> <text>");
        std::process::exit(2);
    }
    let page_index = match args[3].parse::<usize>() {
        Ok(index) => index,
        Err(_) => {
            eprintln!("page-index must be a non-negative integer");
            std::process::exit(2);
        }
    };
    let package = match Package::open(&args[1]) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("could not open {}: {error}", args[1]);
            std::process::exit(1);
        }
    };
    let shape = NewShape::new(&args[4], 0.0, 0.0, 100.0, 50.0).text(&args[5]);
    let bytes = match patch_page(&package, page_index, &[], &[shape], &[]) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("patch failed: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = std::fs::write(&args[2], bytes) {
        eprintln!("could not write {}: {error}", args[2]);
        std::process::exit(1);
    }
}
