# 36 - Packaging the native programs (N6)

What ships: `layoutcraft-app.exe` (the editor) and `layoutcraft-mcp.exe` (the MCP server, started by an AI client; Help > AI assistant shows the configuration for the exe that sits next to the app). Both are single files, no installer is needed to run them.

## Step 402: third-party licences (done)

`cargo xtask notices` (in `rust/`; it replaces the Node script `gen-third-party.mjs`, [42](42-javascript-removal.md)) runs `cargo metadata` for the Windows target, takes the normal dependencies of the two programs (231 crates besides our own `layoutcraft-*` crates), and writes

* `LayoutCraft/THIRD-PARTY-NOTICES.txt`: every crate with version, licence and repository, then the 155 distinct licence texts found in the crate sources (the font crate keeps its texts in `fonts/`: OFL 1.1, Ubuntu Font Licence, Hack, the emoji font MIT text; the egui family does not package its LICENSE files, the standard text of the same licence is taken from another crate and marked as such);
* `LayoutCraft/docs/36-licences.json`: the machine readable list and the `flagged` list (crates whose licence is not on the permissive list: none) and `withoutText` (none).

`cargo xtask notices --check` says whether both files are current (CI runs it; run `cargo xtask notices` again after a dependency change). The target is `LC_TARGET` (default `x86_64-pc-windows-gnu`) and the toolchain argument `CARGO_TOOLCHAIN` (default `+stable-x86_64-pc-windows-gnu`; empty means none, which is what CI and every machine without that toolchain use). The licence files of a crate are read in file-name (byte) order on every platform, so the output does not depend on the operating system.

Licence mix (crates): about 170 MIT OR Apache-2.0 (either order), 21 MIT, 18 Unicode-3.0, 8 Apache-2.0, 5 with Zlib alternatives, 4 Unlicense OR MIT, 2 BSL-1.0, 3 BSD variants, 1 ISC, one `Apache-2.0 OR GPL-2.0-only` (`self_cell`: we use it under Apache-2.0), and `epaint_default_fonts` (code MIT OR Apache-2.0; fonts OFL-1.1 and Ubuntu-font-1.0, whose texts are in the notices file). No copyleft licence is used.

Not covered by this list: the web shell (`web/`) and its fonts (Liberation, Carlito, Caladea: SIL OFL 1.1); they are not part of the native programs.

## Step 403: portable zip (done)

`powershell -File LayoutCraft\tools\package-portable.ps1 [-NoBuild]` builds both exes in release mode (unless `-NoBuild`), and writes `dist\LayoutCraft-portable-<version>.zip` (version from `layoutcraft-app/Cargo.toml`, now 0.1.0) with `LayoutCraft-<version>/layoutcraft-app.exe`, `layoutcraft-mcp.exe`, `THIRD-PARTY-NOTICES.txt`, `PROVENANCE.md`, `README-portable.txt` and the licence files `LICENSE-MIT` / `LICENSE-APACHE`, plus `dist\SHA256SUMS.txt`. Entry names use `/` (Windows PowerShell 5.1 `Compress-Archive` writes `\`). Checked: the listing of the 0.1.0 zip (23,977,797 bytes) has the four files above.

## Step 404: icon, version information, manifest (done)

* `rust/resources/layoutcraft.svg` is the icon (two boxes joined by an arrow connector, blue and yellow), drawn by the project; no third-party artwork is used. `cargo run -p layoutcraft-app --example make_icon` renders it with resvg to `layoutcraft.ico` (7 PNG sizes 16-256) and `layoutcraft-256.png`; both are committed, regenerate only when the SVG changes.
* `rust/resources/win_resource.rs` (included by the `build.rs` of `layoutcraft-app` and `layoutcraft-mcp`) writes a resource script (icon, VERSIONINFO from `CARGO_PKG_VERSION`: ProductName LayoutCraft, FileDescription, FileVersion, OriginalFilename, copyright text, and `layoutcraft.manifest`), compiles it with `windres` from PATH (MinGW binutils; found here in the MinGW the Rust gnu toolchain uses) and links the object into the exe. Without `windres` (other platforms, or a machine without MinGW) the build still succeeds and prints a cargo warning; only Windows x86_64 builds embed it. No download.
* Manifest: asInvoker (no admin), UTF-8 active code page, long paths, per-monitor DPI awareness v2.
* The window and taskbar icon at runtime is the same picture (`window_icon()` in `main.rs`, ViewportBuilder::with_icon).
* Checked: `(Get-Item exe).VersionInfo` shows LayoutCraft / 0.1.0 / the description / OriginalFilename for both exes; ExtractAssociatedIcon gives the icon; the title bar of the running app shows it.
* The copyright line names "LayoutCraft contributors". The project licence is MIT OR Apache-2.0 (`LICENSE-MIT` / `LICENSE-APACHE`), and both files are included in the portable zip. See [`PROVENANCE.md`](../PROVENANCE.md).

## Step 405: installer candidate (written, NOT built or tested)

None of WiX, Inno Setup, NSIS or cargo-packager is installed on this machine, and nothing is downloaded, so the installer has not been built. What exists:

* `tools/installer/layoutcraft.wxs`: a WiX Toolset v4 definition of a per-user MSI (no administrator rights): both exes, the third-party notices and `PROVENANCE.md` in `%LOCALAPPDATA%\Programs\LayoutCraft`, a Start menu shortcut with the icon, an Add/Remove Programs icon, a fixed UpgradeCode (`bf8ebfc9-2d6b-4648-8c01-7f0588f308a4`) with MajorUpgrade, and an optional feature "Open .vsdx files with LayoutCraft" that is OFF by default (it only adds LayoutCraft to the "Open with" list of .vsdx, it does not take Visio's association). The file is well-formed XML; it has never been through `wix build`, so expect small fixes on the first real build.
* `tools/installer/build-msi.ps1`: builds the exes (unless `-NoBuild`) and runs `wix build` into `dist\LayoutCraft-<version>.msi`; without the `wix` command it prints the install hint and exits with code 2 (tested: that is what happens here).
* To build it: `dotnet tool install --global wix`, then `powershell -File LayoutCraft\tools\installer\build-msi.ps1`. An unsigned MSI makes Windows SmartScreen show a warning; signing is a user decision (step 406).
* Alternative if WiX is not wanted: an Inno Setup script would be shorter, but it needs the Inno compiler installed; cargo-packager would need a download as well. The portable zip (step 403) needs none of this.

## Step 406: CI and release workflows (drafts, never run)

* `.github/workflows/ci.yml`: on push to main and on pull requests. Job `native` on windows / ubuntu / macos: `cargo test --workspace`, release build of `layoutcraft-app` and `layoutcraft-mcp`, the public fixture conformance run. Job `web` (ubuntu): `cargo xtask provenance`, the public wasm build (`build-web-wasm.sh public`) and `cargo xtask notices --check`. Job `deny`: `cargo deny check`. No Node is needed in CI.
* `.github/workflows/release.yml`: on a tag `v*`. Windows: `tools/package-portable.ps1` (with `LC_TOOLCHAIN=+stable`, the runner's msvc toolchain instead of the gnu one of this machine); Linux and macOS: a tar.gz with both binaries and the notices plus SHA256 sums; a `publish` job (the only one with `contents: write`) creates a DRAFT release with all artifacts. Signing, the MSI and notarisation are commented placeholders.
* Layout caveat: the workflows were written for a working tree in which `LayoutCraft/` sat next to a `visio-test-harness/` folder. In this repository the repository root is the former `LayoutCraft/` folder and the build scripts are in `tools/`, so the paths in both workflows, and the jobs that run the Visio-based test scripts (which need the private corpus), have to be adapted before the first run.
* Neither file has been run or validated by a YAML parser here (none installed); expect a first-run fix or two. The Linux job lists the system libraries eframe needs (libxkbcommon, wayland, x11, xcursor/xrandr/xi, GL/EGL, GTK for the file dialogs if `rfd` is ever added; the app currently uses the Win32 dialogs only on Windows, so on Linux/macOS the Open / Save dialogs are NOT available yet).

## Platform status

* Verified: Windows x86_64 (gnu toolchain): build, 17 + 4 Rust tests, the gate / UI / Visio checkpoint s401, the exes, the portable zip.
* Not verified: macOS and Linux builds of `layoutcraft-app` (never compiled here). The native file dialogs (`file_dialog` in `main.rs`) are implemented for Windows only (windows-sys); on other systems Open, Save as, Export and Open stencil return "no file chosen" until a portable dialog is added (an open item, N7). `layoutcraft-mcp` has no platform-specific code and is expected to build everywhere.

## Signing and notarisation: user decisions

* Windows: an Authenticode code-signing certificate (OV or EV) from a certificate authority; without it Windows SmartScreen warns about the exes and the MSI. The release workflow has the signtool step ready as a comment (secrets `WINDOWS_CERT_PFX_BASE64`, `WINDOWS_CERT_PASSWORD`). Alternative: a cloud signing service (Azure Trusted Signing).
* macOS: an Apple Developer Program membership (Developer ID Application certificate, notarisation with an app-specific password); a bare binary cannot be stapled, so a `.app` bundle / `.dmg` is wanted for macOS. Secrets: `APPLE_DEVELOPER_ID_CERT_BASE64`, `APPLE_CERT_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`.
* Linux: no signing needed; checksums (and later a GPG or sigstore signature) are enough.

## Open items for the user (decisions)

* Project licence: `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`). The portable zip includes both files.
* Signing certificates and Apple notarisation (see "Signing and notarisation: user decisions").
* Whether an MSI is wanted (needs the WiX Toolset installed) and whether macOS / Linux builds are wanted at all (they need portable file dialogs first).

## Status

N6 steps 402-406 are done as far as they can be on this machine; see the caveats above (installer and workflows written but never run, macOS/Linux untested).
