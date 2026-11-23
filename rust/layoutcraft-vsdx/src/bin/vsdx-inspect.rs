// Small headless inspector for the VSDX package/page layer.
use std::path::PathBuf;
use layoutcraft_vsdx::{Package, ResolvedShape};

fn main() {
    let Some(path) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: vsdx-inspect <file.vsdx>");
        std::process::exit(2);
    };
    let package = match Package::open(&path) {
        Ok(package) => package,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    };
    println!("{}: {} entries", path.display(), package.entry_count());
    match package.pages() {
        Ok(pages) => {
            for page in pages {
                let id = page.id.as_deref().unwrap_or("-");
                let shapes = match package.resolve_page_shapes(&page.part) {
                    Ok(shapes) => shapes,
                    Err(err) => {
                        eprintln!("shapes for '{}': {err}", page.name);
                        continue;
                    }
                };
                let mut count = 0usize;
                let mut master_derived = 0usize;
                for node in &shapes {
                    node.walk(&mut |n: &ResolvedShape| {
                        count += 1;
                        if n.is_master_derived() {
                            master_derived += 1;
                        }
                    });
                }
                println!(
                    "page {id} '{}' -> {}  shapes={count} master-derived={master_derived}",
                    page.name, page.part
                );
            }
        }
        Err(err) => {
            eprintln!("pages: {err}");
            std::process::exit(1);
        }
    }
}
