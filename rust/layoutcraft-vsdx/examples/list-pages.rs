use layoutcraft_vsdx::Package;
fn main() {
    let p = Package::open(std::env::args().nth(1).unwrap()).unwrap();
    for (i, page) in p.pages().unwrap().iter().enumerate() {
        println!("{i} {} {}", page.name, page.part);
    }
}
