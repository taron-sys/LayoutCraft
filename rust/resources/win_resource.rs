// Shared by the build.rs of layoutcraft-app and layoutcraft-mcp (`include!`): writes a Windows resource script (icon, version information, manifest),
// compiles it and links the result into the binary. The msvc targets (x64 and ARM64) use `rc.exe` of the Windows SDK (found in the
// installed kits, or named by the environment variable RC); the x64 gnu target uses `windres` (MinGW binutils, found on PATH).
// Without the tool the build still works (the exe has no icon / version information) and says so with a cargo warning; the Windows
// packaging script refuses such an exe for the msvc targets. No network, no download.
use std::path::{Path, PathBuf};
use std::process::Command;

fn embed(product: &str, description: &str, exe_name: &str) {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../resources/win_resource.rs");
    println!("cargo:rerun-if-changed=../resources/layoutcraft.ico");
    println!("cargo:rerun-if-changed=../resources/layoutcraft.manifest");
    println!("cargo:rerun-if-env-changed=RC");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !msvc && std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64") {
        println!("cargo:warning=no resource compiler is set up for this target: the exe has no icon / version information");
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
    std::fs::write(&rc_path, rc).expect("write .rc");
    if msvc {
        compile_with_rc(&out, exe_name);
    } else {
        compile_with_windres(&out, exe_name);
    }
}

/// `windres` makes a COFF object that the gnu linker takes like any other.
fn compile_with_windres(out: &Path, exe_name: &str) {
    let rc_path = out.join(format!("{exe_name}.rc"));
    let obj_path = out.join(format!("{exe_name}-res.o"));
    match Command::new("windres").args(["--target", "pe-x86-64", "-O", "coff", "-i"]).arg(&rc_path).arg("-o").arg(&obj_path).output() {
        Ok(result) if result.status.success() => println!("cargo:rustc-link-arg-bins={}", obj_path.display()),
        Ok(result) => println!("cargo:warning=windres failed, the exe has no icon / version information: {}", String::from_utf8_lossy(&result.stderr).trim()),
        Err(error) => println!("cargo:warning=windres not found ({error}): the exe has no icon / version information"),
    }
}

/// `rc.exe` makes a `.res` file that `link.exe` takes for any machine type. The manifest is in it, so the linker is told not to make
/// one of its own (two manifest resources fail the link).
fn compile_with_rc(out: &Path, exe_name: &str) {
    let compiler = std::env::var_os("RC").map(PathBuf::from).or_else(find_rc).unwrap_or_else(|| PathBuf::from("rc.exe"));
    let res = format!("{exe_name}.res");
    // Run in the output folder with bare file names.
    match Command::new(&compiler).current_dir(out).args(["/nologo", "/fo", &res, &format!("{exe_name}.rc")]).output() {
        Ok(result) if result.status.success() => {
            println!("cargo:rustc-link-arg-bins={}", out.join(res).display());
            println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        }
        Ok(result) => println!(
            "cargo:warning={} failed, the exe has no icon / version information: {} {}",
            compiler.display(),
            String::from_utf8_lossy(&result.stdout).trim(),
            String::from_utf8_lossy(&result.stderr).trim()
        ),
        Err(error) => println!("cargo:warning=rc.exe not found ({error}): the exe has no icon / version information"),
    }
}

/// The `rc.exe` of the newest installed Windows SDK that has one for the machine the build runs on.
fn find_rc() -> Option<PathBuf> {
    let host = if std::env::var("HOST").is_ok_and(|host| host.starts_with("aarch64")) { "arm64" } else { "x64" };
    let kits = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?).join("Windows Kits").join("10").join("bin");
    // The SDK folders are version numbers such as 10.0.22621.0: compare them number by number.
    let version = |name: &str| name.split('.').map(|part| part.parse::<u32>().ok()).collect::<Option<Vec<_>>>();
    std::fs::read_dir(kits)
        .ok()?
        .flatten()
        .filter_map(|entry| Some((version(entry.file_name().to_str()?)?, entry.path().join(host).join("rc.exe"))))
        .filter(|(_, path)| path.is_file())
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path)| path)
}
