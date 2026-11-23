// Shared by the build.rs of layoutcraft-app and layoutcraft-mcp (`include!`): writes a Windows resource script (icon, version information, manifest),
// compiles it with `windres` (MinGW binutils, found on PATH) and links the object into the binary. Without windres the build still works
// (the exe has no icon / version information) and says so with a cargo warning. No network, no download.
use std::path::{Path, PathBuf};
use std::process::Command;

fn embed(product: &str, description: &str, exe_name: &str) {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../resources/win_resource.rs");
    println!("cargo:rerun-if-changed=../resources/layoutcraft.ico");
    println!("cargo:rerun-if-changed=../resources/layoutcraft.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") || std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64") {
        return;
    }
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let resources = manifest_dir.join("../resources");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let mut parts = version.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u16>().unwrap_or(0));
    let nums = [parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0), 0u16];
    let slash = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
    let rc = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},{d}
PRODUCTVERSION {a},{b},{c},{d}
FILEOS 0x40004
FILETYPE 1
{{
  BLOCK "StringFileInfo"
  {{
    BLOCK "040904B0"
    {{
      VALUE "CompanyName", "LayoutCraft contributors"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{exe_name}"
      VALUE "LegalCopyright", "Copyright (C) LayoutCraft contributors"
      VALUE "OriginalFilename", "{exe_name}.exe"
      VALUE "ProductName", "{product}"
      VALUE "ProductVersion", "{version}"
    }}
  }}
  BLOCK "VarFileInfo"
  {{
    VALUE "Translation", 0x0409, 0x04B0
  }}
}}
"#,
        icon = slash(&resources.join("layoutcraft.ico")),
        manifest = slash(&resources.join("layoutcraft.manifest")),
        a = nums[0],
        b = nums[1],
        c = nums[2],
        d = nums[3],
    );
    let rc_path = out.join(format!("{exe_name}.rc"));
    let obj_path = out.join(format!("{exe_name}-res.o"));
    std::fs::write(&rc_path, rc).expect("write .rc");
    match Command::new("windres").args(["--target", "pe-x86-64", "-O", "coff", "-i"]).arg(&rc_path).arg("-o").arg(&obj_path).output() {
        Ok(result) if result.status.success() => println!("cargo:rustc-link-arg-bins={}", obj_path.display()),
        Ok(result) => println!("cargo:warning=windres failed, the exe has no icon / version information: {}", String::from_utf8_lossy(&result.stderr).trim()),
        Err(error) => println!("cargo:warning=windres not found ({error}): the exe has no icon / version information"),
    }
}
