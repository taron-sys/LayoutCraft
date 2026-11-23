//! Two compile-time switches:
//! * `cfg(private_fixtures)`: the private test corpus is present (docs/38-private-fixtures.md): tests that read `test/` or `examples/` are compiled only then,
//!   so a public clone without those folders still builds and tests green.
//! * `cfg(private_font_metrics)`: `../../private/font-metrics-windows/font_metrics.bin` (advances of the Windows fonts, never published) exists, so the
//!   maintainers' build measures text with it instead of the public table made from the open fonts in `web/fonts`. LAYOUTCRAFT_PUBLIC_BUILD=1 switches it off.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(private_fixtures)");
    println!("cargo:rustc-check-cfg=cfg(private_font_metrics)");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LAYOUTCRAFT_PUBLIC_BUILD");
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let metrics = manifest.join("../../private/font-metrics-windows/font_metrics.bin");
    println!("cargo:rerun-if-changed={}", metrics.display());
    if metrics.is_file() && std::env::var("LAYOUTCRAFT_PUBLIC_BUILD").as_deref() != Ok("1") {
        println!("cargo:rustc-cfg=private_font_metrics");
    }
    // the LayoutCraft folder: its modification time changes when `test` appears or disappears (a missing path would rebuild every time)
    println!("cargo:rerun-if-changed=../..");
    if manifest.join("../../test/corpus").is_dir() {
        println!("cargo:rustc-cfg=private_fixtures");
    }
}
