//! usage: add-picture <source.vsdx> <media-source.vsdx> <media-part> <out.vsdx> <page-index> <shape-id>
use std::env;
use layoutcraft_vsdx::{patch_page_with_media, NewPicture, Package};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 7 {
        eprintln!("usage: add-picture <source.vsdx> <media-source.vsdx> <media-part> <out.vsdx> <page-index> <shape-id>");
        std::process::exit(2);
    }
    let package = match Package::open(&args[1]) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("could not open {}: {error}", args[1]);
            std::process::exit(1);
        }
    };
    let media_package = match Package::open(&args[2]) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("could not open {}: {error}", args[2]);
            std::process::exit(1);
        }
    };
    let Some(bytes) = media_package.entry(&args[3]) else {
        eprintln!("media part {} not found", args[3]);
        std::process::exit(1);
    };
    let page_index = match args[5].parse::<usize>() {
        Ok(index) => index,
        Err(_) => {
            eprintln!("page-index must be a non-negative integer");
            std::process::exit(2);
        }
    };
    let picture =
        NewPicture::new(&args[6], 10.0, 20.0, 200.0, 150.0, bytes.to_vec()).text("Added picture");
    let patched = match patch_page_with_media(&package, page_index, &[], &[], &[], &[], &[picture])
    {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("patch failed: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = std::fs::write(&args[4], patched) {
        eprintln!("could not write {}: {error}", args[4]);
        std::process::exit(1);
    }
}
