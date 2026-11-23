//! Apply a group edit and write the package, or print world centres.
//!   edit-group <in> <out> resize <id> <w> <h> <ax> <ay>
//!   edit-group <in> <out> move   <id> <dx> <dy> <refit 0|1>
//!   edit-group <in> centres      (prints "id cx cy" in inches, y up)
use layoutcraft_vsdx::groupedit::GroupEditor;
use layoutcraft_vsdx::writer::patch_page_cells;
use layoutcraft_vsdx::{import_document, Package};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let package = Package::open(&a[1]).unwrap();
    if a[2] == "centres" {
        let document = import_document(&package).unwrap();
        let page_h = package.pages().unwrap()[0].height.unwrap();
        for s in &document.pages[0].shapes {
            println!("{} {:.4} {:.4}", s.visio_id.clone().unwrap(), (s.x + s.w / 2.0) / 96.0, page_h - (s.y + s.h / 2.0) / 96.0);
        }
        return;
    }
    let editor = GroupEditor::open(&package, 0).unwrap();
    let f = |i: usize| a[i].parse::<f64>().unwrap();
    let patches = match a[3].as_str() {
        "resize" => editor.resize_group(&a[4], (f(5), f(6)), (f(7), f(8))).unwrap(),
        "move" => editor.move_member(&a[4], (f(5), f(6)), a[7] == "1").unwrap(),
        other => panic!("unknown op {other}"),
    };
    std::fs::write(&a[2], patch_page_cells(&package, 0, &patches).unwrap()).unwrap();
}
