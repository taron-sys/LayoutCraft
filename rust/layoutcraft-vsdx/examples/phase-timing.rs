//! Time the import phases of one package: usage phase-timing <file>
use std::time::Instant;
use layoutcraft_vsdx::{import_document, DocumentStyles, MasterCatalog, Package, Theme};

fn main() {
    let path = std::env::args().nth(1).expect("file");
    let t = Instant::now();
    let package = Package::open(&path).unwrap();
    println!("open          {:>7.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let catalog = MasterCatalog::load(&package).unwrap();
    println!("masters load  {:>7.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let _ = DocumentStyles::load(&package).unwrap();
    println!("styles load   {:>7.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let _ = Theme::load(&package).unwrap();
    println!("theme load    {:>7.0} ms", t.elapsed().as_secs_f64() * 1e3);
    for page in package.pages().unwrap() {
        let t = Instant::now();
        let tree = package.page_shapes(&page.part).unwrap();
        println!("page parse    {:>7.0} ms", t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let _ = catalog.resolve(&tree).unwrap();
        println!("page resolve  {:>7.0} ms", t.elapsed().as_secs_f64() * 1e3);
    }
    let t = Instant::now();
    let _ = import_document(&package).unwrap();
    println!("full import   {:>7.0} ms", t.elapsed().as_secs_f64() * 1e3);
}
