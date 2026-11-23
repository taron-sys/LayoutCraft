//! List groups of a page with their frames: usage dump-groups <file> [page]
use layoutcraft_vsdx::{MasterCatalog, Package};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let package = Package::open(&args[1]).unwrap();
    let page: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    let pages = package.pages().unwrap();
    let tree = package.page_shapes(&pages[page].part).unwrap();
    let roots = MasterCatalog::load(&package).unwrap().resolve(&tree).unwrap();
    fn walk(nodes: &[layoutcraft_vsdx::ResolvedShape], depth: usize) {
        for n in nodes {
            let g = |k: &str| n.cells.get(k).and_then(|c| c.value.clone()).unwrap_or_default();
            println!("{}{} {:?} {:?} pin=({},{}) size=({},{}) loc=({},{}) ang={} flip=({},{})", "  ".repeat(depth), n.id, n.shape_type, n.name, g("PinX"), g("PinY"), g("Width"), g("Height"), g("LocPinX"), g("LocPinY"), g("Angle"), g("FlipX"), g("FlipY"));
            walk(&n.children, depth + 1);
        }
    }
    walk(&roots, 0);
}
