//! Sets `cfg(private_fixtures)` when the private test corpus is present (docs/38-private-fixtures.md): tests that read
//! `test/` or `examples/` are compiled only then, so a public clone without those folders still builds and tests green.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(private_fixtures)");
    // the LayoutCraft folder: its modification time changes when `test` appears or disappears (a missing path would rebuild every time)
    println!("cargo:rerun-if-changed=../..");
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus");
    if corpus.is_dir() {
        println!("cargo:rustc-cfg=private_fixtures");
    }
}
