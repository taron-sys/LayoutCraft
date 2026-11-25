# Release build of the two Windows programs (docs/36-packaging.md): layoutcraft-app.exe and layoutcraft-mcp.exe for one architecture.
#   pwsh tools/packaging/windows/build.ps1 [-Arch x64|arm64] [-Target <rust triple>] [-Toolchain +<name>]
# The exes end up in rust\target\<triple>\release. arm64 is cross-compiled (it needs the Rust target aarch64-pc-windows-msvc and the
# MSVC ARM64 build tools, both on the GitHub runners).
# The default target is the msvc one. The maintainers' gnu toolchain is -Toolchain +stable-x86_64-pc-windows-gnu -Target x86_64-pc-windows-gnu
# (LC_TOOLCHAIN=+name in the environment does the same for the toolchain, as the older instructions had it).
param(
  [ValidateSet('x64', 'arm64')][string]$Arch = 'x64',
  [string]$Target = '',
  [string]$Toolchain = $env:LC_TOOLCHAIN
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')
$Target = Resolve-LcTarget $Arch $Target

# Public build only (no private modules); symbols stripped from the release profile without touching rust/Cargo.toml.
$env:LAYOUTCRAFT_PUBLIC_BUILD = '1'
$env:CARGO_PROFILE_RELEASE_STRIP = 'symbols'
# Keep the build machine's paths (user name, home folder, checkout) out of the binaries: rustc embeds the source paths of dependencies.
# CARGO_ENCODED_RUSTFLAGS has the separator 0x1f, so a path with spaces survives.
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$cargoHome=/cargo" + [char]0x1f + "--remap-path-prefix=$script:LcRoot=/layoutcraft"

$cargoArgs = @()
if ($Toolchain) { $cargoArgs += $Toolchain }
$cargoArgs += @('build', '--release', '--locked', '--target', $Target, '-p', 'layoutcraft-app', '-p', 'layoutcraft-mcp')
Push-Location (Join-Path $script:LcRoot 'rust')
try {
  cargo @cargoArgs
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $Target" }
} finally { Pop-Location }

$bin = Get-LcBinDir $Target
foreach ($exe in 'layoutcraft-app.exe', 'layoutcraft-mcp.exe') {
  if (-not (Test-Path (Join-Path $bin $exe))) { throw "missing $(Join-Path $bin $exe) after the build" }
}
# The icon and version information are linked in by the build.rs of both crates when windres (MinGW binutils) is on PATH, for x64 only
# (rust/resources/win_resource.rs). Say so when they are missing instead of shipping that silently.
$info = (Get-Item (Join-Path $bin 'layoutcraft-app.exe')).VersionInfo
if ($info.ProductName -ne 'LayoutCraft') {
  Write-Warning "layoutcraft-app.exe has no version information or icon (windres was not used for $Target). It works, but Explorer shows the generic icon."
  if ($env:GITHUB_ACTIONS) { Write-Host "::warning title=No exe icon ($Target)::layoutcraft-app.exe was built without the embedded icon and version information (windres is only used for x64, see docs/36-packaging.md)" }
}
Write-Host "built $bin"
