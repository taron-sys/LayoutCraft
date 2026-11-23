// Icon, version information and manifest of layoutcraft-app.exe (see ../resources/win_resource.rs).
include!("../resources/win_resource.rs");

fn main() {
    embed("LayoutCraft", "LayoutCraft - Visio-compatible diagram editor", "layoutcraft-app");
}
