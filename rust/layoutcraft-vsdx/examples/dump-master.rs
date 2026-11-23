//! List the parsed shapes of one master: usage dump-master <file> <master-id>
use layoutcraft_vsdx::{MasterCatalog, Package};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let package = Package::open(&a[1]).unwrap();
    let catalog = MasterCatalog::load(&package).unwrap();
    let master = catalog.get(&a[2]).expect("master");
    println!("master {} {} top={:?}", master.id, master.name, master.top);
    for (id, s) in &master.shapes {
        println!("  {id} type={:?} parent={:?} order={} text={} geom={}", s.shape_type, s.parent, s.order, s.has_text, s.sections.iter().any(|x| x.name == "Geometry"));
    }
}
