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

`pwsh tools/package-portable.ps1 [-Arch x64|arm64] [-NoBuild] [-Target <triple>] [-Toolchain +<name>] [-OutDir <folder>]` builds both exes in release mode (unless `-NoBuild`; `tools/packaging/windows/build.ps1` does the build) and writes `<OutDir>\LayoutCraft-<version>-windows-<arch>-portable.zip` (default OutDir `dist\`, version from the two `Cargo.toml` files, now 0.1.0) with `LayoutCraft-<version>/layoutcraft-app.exe`, `layoutcraft-mcp.exe`, `THIRD-PARTY-NOTICES.txt`, `PROVENANCE.md`, `README-portable.txt`, `LICENSE-MIT` and `LICENSE-APACHE`. Entry names use `/` (Windows PowerShell 5.1 `Compress-Archive` writes `\`). The default target is msvc (`x86_64-pc-windows-msvc`, or `aarch64-pc-windows-msvc` for arm64); on a machine with the gnu toolchain pass `-Toolchain +stable-x86_64-pc-windows-gnu -Target x86_64-pc-windows-gnu` (or set `LC_TOOLCHAIN`). `SHA256SUMS.txt` is written by the release pipeline over all files, no longer by this script. Checked: the 0.1.0 zip of the x64 build has the seven files above.

## Step 404: icon, version information, manifest (done)

* `rust/resources/layoutcraft.svg` is the icon (two boxes joined by an arrow connector, blue and yellow), drawn by the project; no third-party artwork is used. `cargo run -p layoutcraft-app --example make_icon` renders it with resvg to `layoutcraft.ico` (7 PNG sizes 16-256) and `layoutcraft-256.png`; both are committed, regenerate only when the SVG changes.
* `rust/resources/win_resource.rs` (included by the `build.rs` of `layoutcraft-app` and `layoutcraft-mcp`) writes a resource script (icon, VERSIONINFO from `CARGO_PKG_VERSION`: ProductName LayoutCraft, FileDescription, FileVersion, ProductVersion, OriginalFilename, copyright text, and `layoutcraft.manifest`) and links the compiled result into the exe. The msvc targets, x64 and ARM64, compile it with `rc.exe` of the newest installed Windows SDK (or the program named by `RC`) and pass `/MANIFEST:NO` to the linker, because the manifest is in the resource. The x64 gnu target uses `windres` from PATH. No download. Without the tool the build succeeds with a cargo warning; `tools/packaging/windows/build.ps1` then fails an msvc build whose product name, version or file name is wrong, and only warns for gnu. Other architectures of the gnu toolchain get no resource. The MSI shortcut and the Add/Remove Programs entry carry the icon in any case.
* Manifest: asInvoker (no admin), UTF-8 active code page, long paths, per-monitor DPI awareness v2.
* The window and taskbar icon at runtime is the same picture (`window_icon()` in `main.rs`, ViewportBuilder::with_icon).
* Checked: `(Get-Item exe).VersionInfo` shows LayoutCraft / 0.1.0 / the description / OriginalFilename for both exes; ExtractAssociatedIcon gives the icon; the title bar of the running app shows it.
* The copyright line names "LayoutCraft contributors". The project licence is MIT OR Apache-2.0 (`LICENSE-MIT` / `LICENSE-APACHE`), and both files are included in the portable zip. See [`PROVENANCE.md`](../PROVENANCE.md).

## Step 405: installer (MSI)

`tools/installer/layoutcraft.wxs` is a WiX Toolset v4+ definition of a per-user MSI (no administrator rights): both exes, the third-party notices, the licence files and `PROVENANCE.md` in `%LOCALAPPDATA%\Programs\LayoutCraft`, a Start menu shortcut with the icon, an Add/Remove Programs icon, a fixed UpgradeCode (`bf8ebfc9-2d6b-4648-8c01-7f0588f308a4`) with MajorUpgrade, and an optional feature "Open .vsdx files with LayoutCraft" that is OFF by default (it only adds LayoutCraft to the "Open with" list of .vsdx, it does not take over another program's association).

`pwsh tools/installer/build-msi.ps1 [-Arch x64|arm64] [-NoBuild] [-OutDir <folder>]` writes `LayoutCraft-<version>-windows-<arch>.msi`. The architecture is `wix build -arch`; the same `.wxs` serves both. An MSI version is numeric, so a pre-release suffix (`0.2.0-rc1`) is dropped from it (the file name keeps it). Without the `wix` command the script says how to install it and exits with code 2. To build it yourself: `dotnet tool install --global wix --version <WIX_VERSION in tools/packaging/versions.env>`. The pipeline uses WiX 5.0.2, the newest line before the Open Source Maintenance Fee agreement of later versions. An unsigned MSI makes Windows SmartScreen show a warning (see "Adding signing").

Status: the `.wxs` is well-formed XML; `wix build` has not been run on it yet (no .NET SDK on the maintainer's machine), so the first dry run of the pipeline is its first real build.

## Release pipeline
* `.github/workflows/ci.yml`: on push to main and on pull requests. Job `native` on windows / ubuntu / macos: `cargo test --workspace`, release build of `layoutcraft-app` and `layoutcraft-mcp`, the public fixture conformance run. Job `web` (ubuntu): `cargo xtask provenance`, the public wasm build (`build-web-wasm.sh public`) and `cargo xtask notices --check`. Job `deny`: `cargo deny check`. No Node is needed in CI.
* `.github/workflows/release.yml`: on a tag `v*`. Windows: `tools/package-portable.ps1` (with `LC_TOOLCHAIN=+stable`, the runner's msvc toolchain instead of the gnu one of this machine); Linux and macOS: a tar.gz with both binaries and the notices plus SHA256 sums; a `publish` job (the only one with `contents: write`) creates a DRAFT release with all artifacts. Signing, the MSI and notarisation are commented placeholders.
* Layout caveat: the workflows were written for a working tree in which `LayoutCraft/` sat next to a `visio-test-harness/` folder. In this repository the repository root is the former `LayoutCraft/` folder and the build scripts are in `tools/`, so the paths in both workflows, and the jobs that run the Visio-based test scripts (which need the private corpus), have to be adapted before the first run.
* Neither file has been run or validated by a YAML parser here (none installed); expect a first-run fix or two. The Linux job lists the system libraries eframe needs (libxkbcommon, wayland, x11, xcursor/xrandr/xi, GL/EGL, GTK for the file dialogs if `rfd` is ever added; the app currently uses the Win32 dialogs only on Windows, so on Linux/macOS the Open / Save dialogs are NOT available yet).

`.github/workflows/release.yml` is thin; the work is in `tools/packaging/`:

| Path | What it does |
|---|---|
| `versions.env` | every pinned tool version (Rust, WiX, appimagetool and its runtime with SHA-256) |
| `lib.sh`, `archive.py` | shared helpers; deterministic zip and tar.gz (sorted entries, fixed time and owner) |
| `check-version.sh` | both `Cargo.toml` versions equal and well-formed, and the tag is `v<version>` |
| `smoke-mcp.sh` | starts a built `layoutcraft-mcp` and checks its `initialize` answer |
| `windows/` | `build.ps1`, `sign.ps1` (the packaging itself is `tools/package-portable.ps1` and `tools/installer/build-msi.ps1`) |
| `linux/` | `build.sh`, `package.sh` (tar.gz, deb, rpm, AppImage from one staged tree), `verify.sh`, desktop file, MIME type, rpm spec |
| `macos/` | `build.sh` (lipo), `make-app.sh`, `sign.sh`, `make-dmg.sh`, `notarize.sh`, `verify.sh`, `Info.plist.in` |
| `web/package.sh` | public wasm build, load check, zip |
| `assemble.sh`, `publish-release.sh` | expected-file check and `SHA256SUMS.txt`; draft, upload, verify, publish |

Triggers: a pushed tag `v*` (release), `workflow_dispatch`, and pull requests that change the workflow, `tools/packaging/**`, `tools/installer/**`, `tools/package-portable.ps1`, `tools/build-web-wasm.sh` or `rust/resources/**`. The last two are dry runs: everything is built and checked and kept as workflow artifacts, no release is made. Every build uses `LAYOUTCRAFT_PUBLIC_BUILD=1`, `--locked`, the Rust version in `versions.env` (not "stable"), `--remap-path-prefix` for the cargo home and the checkout, and `strip=symbols` set through the environment (not in `rust/Cargo.toml`, so local release builds keep their symbols). A tag build starts from an empty cache; dry runs use `Swatinem/rust-cache`.

Jobs (all with `contents: read` except the last):

| Job | Runner | Makes |
|---|---|---|
| `prepare` | ubuntu-24.04 | fails early when the tag is not `v<version>` of both programs |
| `windows` (x64, arm64) | windows-2025 | exes (ARM64 cross-compiled), portable zip, MSI |
| `linux` (x86_64, aarch64) | ubuntu-22.04 | tar.gz, deb, rpm, AppImage (aarch64 cross-compiled) |
| `macos` | macos-15 | universal exes (lipo), `LayoutCraft.app`, dmg, tar.gz |
| `web` | ubuntu-24.04 | the browser zip |
| `assemble` | ubuntu-24.04 | checks the 15 expected files, writes `SHA256SUMS.txt`, uploads the artifact `release-files` (also in a dry run) |
| `release` | ubuntu-24.04 | tags only, `contents: write`: creates the draft release with generated notes, uploads, compares GitHub's SHA-256 of each file with `SHA256SUMS.txt`, then publishes |

Release files (`<v>` is the version, for example 0.1.0): `LayoutCraft-<v>-windows-x64-portable.zip`, `-windows-x64.msi`, `-windows-arm64-portable.zip`, `-windows-arm64.msi`, `-macos-universal.dmg`, `-macos-universal.tar.gz`, `-linux-x86_64.tar.gz`, `.deb`, `.rpm`, `.AppImage`, the same four for `-linux-aarch64`, `-web.zip`, and `SHA256SUMS.txt`. Every archive and the deb carry `THIRD-PARTY-NOTICES.txt`, `LICENSE-MIT`, `LICENSE-APACHE` and `PROVENANCE.md` (`assemble.sh` checks the zip, tar.gz and deb files; the rpm, AppImage, MSI and dmg are made from the same staged files).

Why these choices:

* Linux is built on Ubuntu 22.04, the oldest runner image, so the programs need glibc 2.35 or newer (Debian 12, Ubuntu 22.04, Fedora 36, RHEL 9). `build.sh` reads the newest glibc symbol from the binaries and fails above 2.35. When GitHub retires the 22.04 image, move to the oldest one left and raise `GLIBC_MAX` in `build.sh` and the `libc6 (>= 2.35)` dependency in `package.sh`.
* Linux aarch64 and Windows ARM64 are cross-compiled on x86_64 runners. GitHub's free ARM runners (`ubuntu-*-arm`, `windows-11-arm`) are only for public repositories and this repository is private. The build needs no library of the target system (the window system, OpenGL, xkbcommon and fontconfig are loaded at run time), so a cross compiler and the Rust target are all it takes. The cost: the ARM64 programs are not started in CI; only their architecture is checked.
* The .deb and .rpm are made from one staged tree with `dpkg-deb` and `rpmbuild`, not with cargo-deb or cargo-generate-rpm: those take built files from cargo's layout, which is awkward for a workspace where one package ships another crate's binary, and when cross-compiling. The staged tree is also what the tarball and the AppImage use, and nothing has to be `cargo install`ed.
* The AppImage is packed with appimagetool 1.9.1 and the type-2 runtime, both downloaded from their GitHub releases and checked against SHA-256 values in `versions.env`; the x86_64 tool packs the aarch64 image too (it only joins the runtime and a squashfs of files). The AppImage does not bundle libraries: window system, OpenGL and fontconfig come from the host, as they do for the other packages.
* The dependencies of the deb and rpm are written out because the programs load libxkbcommon, OpenGL and EGL at run time. The packages install the .desktop file, the icons (256 px PNG and the SVG) and the MIME types of the files the app opens as drawings (`.vsdx`, `.vsdm`, `.vstx`, `.vstm`, `.vsd`, `.vst`, `.vss`, `.vdx`, `.vtx`, `.vsx`; not `.vssx` / `.vssm`, which are opened from the Shapes panel; `tools/packaging/linux/check-mime.sh` checks that the two files agree); the desktop file sets `StartupWMClass=LayoutCraft` (the window class eframe uses) so a taskbar groups the windows under the launcher.
* macOS is built on one Apple-silicon runner for both architectures (`aarch64-apple-darwin`, `x86_64-apple-darwin`) and joined with `lipo`, deployment target 11.0. The bundle is `LayoutCraft.app` with `Info.plist`, `AppIcon.icns` (made from the 256 px PNG with `sips` and `iconutil`) and both programs in `Contents/MacOS` (the app expects the MCP server next to it). The bundle declares no document type for `.vsdx` yet, because the app takes a file only as a start argument, not as an open event.
* Each build job checks its output before uploading: the MCP server is started and must answer `initialize` (Windows x64, Linux x86_64, the arm64 half on macOS), the .deb is installed on the x86_64 runner, the AppImage is unpacked, the dmg is mounted, the wasm module is loaded with the shell's own loader.

## How to cut a release

1. Set the same version in `rust/layoutcraft-app/Cargo.toml` and `rust/layoutcraft-mcp/Cargo.toml`, run `cargo build --locked` (this updates `rust/Cargo.lock`; commit it), check that `node tools/gen-third-party.mjs --check` is clean, and merge to `main`.
2. Run the pipeline once without a tag: Actions > release > Run workflow on `main` (a dry run). Download the artifacts of a platform you can test and start the app.
3. Tag the commit and push the tag: `git tag v0.2.0 && git push origin v0.2.0`. The tag must be exactly `v` plus the version (`v0.2.0-rc1` for a pre-release, which is marked as one automatically). If it is not, the first job fails with a message saying what to fix.
4. When all jobs are green, the `release` job has published the release. If it fails after the draft was created, the draft is left for you to look at; re-running the failed job completes the same draft. A published release is never changed by the pipeline.
5. To redo a tag after a failure, delete the tag and the draft release yourself (the pipeline never deletes anything), fix, and tag again.

## Adding signing

Signing steps exist in the workflow and the scripts but do nothing without their secrets. The secrets are only passed to the signing steps (never to a build step) and only on a tag build. Add them under Settings > Secrets and variables > Actions of the repository (or of an environment).

* Windows (Authenticode, `tools/packaging/windows/sign.ps1`): `WINDOWS_CERT_PFX_BASE64` (the code-signing certificate with its private key as a .pfx, base64-encoded) and `WINDOWS_CERT_PASSWORD`. The exes are signed before they go into the zip and the MSI, and the MSI is signed afterwards. Optional `WINDOWS_TIMESTAMP_URL` (default DigiCert's). A cloud signing service (for example Azure Trusted Signing) needs a different step in place of `sign.ps1`.
* macOS (`tools/packaging/macos/sign.sh`, `notarize.sh`): `APPLE_DEVELOPER_ID_CERT_BASE64` (Developer ID Application certificate with its private key, .p12, base64) and `APPLE_CERT_PASSWORD` sign the programs, the bundle (hardened runtime, secure timestamp) and the dmg; `APPLE_ID`, `APPLE_TEAM_ID` and `APPLE_APP_PASSWORD` (an app-specific password) notarise the dmg and staple the ticket. Without a certificate the bundle gets an ad-hoc signature, which Apple-silicon Macs need to run it at all; Gatekeeper still asks the user (the README in the dmg says how to open it).
* Linux: nothing to sign. `SHA256SUMS.txt` is the integrity check; a detached GPG or sigstore signature of that file can be added to `publish-release.sh` later.

None of the signing code has run, since no certificate exists yet: expect a fix or two on first use, and try it with a pre-release tag first.

## Verified and not verified

Checked on the maintainer's Windows machine for this pipeline: `tools/package-portable.ps1 -Arch x64` end to end (build with `--locked`, strip and path remapping; the zip has the seven files), `check-version.sh` (match and mismatch), `archive.py`, the staging and tar.gz of `linux/package.sh` (with stand-in binaries), the web zip with a real public wasm build and the load check (byte-identical on a second run), `assemble.sh` (missing, unexpected and complete sets), the structure of the workflow (YAML, `needs`, matrix keys, action pins), and `bash -n` of every script. Not run locally, so only the first dry run on GitHub shows whether they work: the whole Linux build and packaging (cross compile, dpkg-deb, rpmbuild, appimagetool), the whole macOS build, the MSI (no .NET SDK or WiX here), Windows ARM64, `publish-release.sh`, and all signing steps.

## Known gaps

* On Linux and macOS the app opens a file given as a start argument (`layoutcraft-app drawing.vsdx`, which the desktop file's `%f` uses). The macOS bundle has no `.vsdx` document type because the app does not handle the macOS open-file event yet.
* In an AppImage the MCP server lives inside the mounted image, so the path shown by Help > AI assistant changes with every start. Use the tar.gz or the deb for AI assistants.
* `layoutcraft-mcp` reports its version from a constant (`serverInfo.version` in `layoutcraft-mcp/src/lib.rs`), which the version check does not cover.
* The release has no build provenance attestation and no signature (see above). Add `actions/attest-build-provenance` with the `id-token` and `attestations` permissions to the `release` job when the repository can use it.

## Platform status

* Windows x64: built, started and packaged on a maintainer's machine.
* Windows ARM64, Linux x86_64 and aarch64, macOS universal: built by the pipeline only. The Linux app needs a desktop session with OpenGL. The file dialogs on Linux and macOS use `rfd` (the desktop portal on Linux).

## Open items for the user (decisions)

* Project licence: `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`), included in every package.
* Signing certificates and Apple notarisation (see "Adding signing").
* Whether the `release` job should wait for a manual approval: create an environment (Settings > Environments), add required reviewers, and add `environment: release` to that job.

## Status

N6 steps 402-406 are done; the release pipeline is written and its dry run is the next test (see "Verified and not verified").
