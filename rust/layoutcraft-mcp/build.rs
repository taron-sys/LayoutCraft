// Icon, version information and manifest of layoutcraft-mcp.exe (see ../resources/win_resource.rs), and `cfg(private_fixtures)` when the
// private test corpus is present (docs/38-private-fixtures.md).
include!("../resources/win_resource.rs");

fn main() {
    embed("LayoutCraft", "LayoutCraft MCP server (stdio)", "layoutcraft-mcp");
    println!("cargo:rustc-check-cfg=cfg(private_fixtures)");
    // the LayoutCraft folder: its modification time changes when `test` appears or disappears (a missing path would rebuild every time)
    println!("cargo:rerun-if-changed=../..");
    if std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("../../test/corpus").is_dir() {
        println!("cargo:rustc-cfg=private_fixtures");
    }
}
