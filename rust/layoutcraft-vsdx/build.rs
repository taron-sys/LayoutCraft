// Two compile-time switches:
// * `cfg(private_themes)`: the extra themes live in ../../private/themes-extra/theme_data.rs (git-ignored, never published).
//   When that file exists the build includes it; LAYOUTCRAFT_PUBLIC_BUILD=1 switches it off, to build what the public repository builds.
// * `cfg(private_fixtures)`: the private test corpus (../../test/corpus) is present (docs/38-private-fixtures.md), so tests that read it are compiled.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(private_themes)");
    println!("cargo:rustc-check-cfg=cfg(private_fixtures)");
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let themes = manifest.join("../../private/themes-extra/theme_data.rs");
    println!("cargo:rerun-if-changed={}", themes.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LAYOUTCRAFT_PUBLIC_BUILD");
    if themes.is_file() && std::env::var("LAYOUTCRAFT_PUBLIC_BUILD").as_deref() != Ok("1") {
        println!("cargo:rustc-cfg=private_themes");
    }
    // the LayoutCraft folder: its modification time changes when `test` appears or disappears (a missing path would rebuild every time)
    println!("cargo:rerun-if-changed=../..");
    if manifest.join("../../test/corpus").is_dir() {
        println!("cargo:rustc-cfg=private_fixtures");
    }
}
