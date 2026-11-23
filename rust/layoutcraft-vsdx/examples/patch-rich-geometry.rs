//! usage: patch-rich-geometry <source.vsdx> <out.vsdx> <page-index> <shape-id>
use serde_json::json;
use std::env;
use layoutcraft_vsdx::{patch_page_cells, GeometryRow, Package, ShapeCellPatch};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: patch-rich-geometry <source.vsdx> <out.vsdx> <page-index> <shape-id>");
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
    let rich = json!({
        "paragraphs": [{
            "pp": "0",
            "align": "center",
            "bullet": false,
            "bulletStr": "•",
            "indent": 0,
            "runs": [
                {"cp":"0","text":"Rich ","style":{"fontSize":20,"textColor":"#112233","bold":true,"italic":false,"underline":false}},
                {"cp":"1","text":"geometry","style":{"fontSize":16,"textColor":"#ff0000","bold":false,"italic":true,"underline":true}}
            ],
            "style": {"align":"center","bullet":false,"bulletStr":"•","indent":0}
        }]
    });
    let rows = vec![
        GeometryRow::new("MoveTo", "1").set("X", "0").set("Y", "0"),
        GeometryRow::new("LineTo", "2")
            .set("X", "1.8")
            .set("Y", "0"),
        GeometryRow::new("LineTo", "3")
            .set("X", "0")
            .set("Y", "1.2"),
        GeometryRow::new("LineTo", "4").set("X", "0").set("Y", "0"),
    ];
    let bytes = match patch_page_cells(
        &package,
        page_index,
        &[ShapeCellPatch::new(&args[4])
            .rich(rich)
            .geometry_rows("0", rows)],
    ) {
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
