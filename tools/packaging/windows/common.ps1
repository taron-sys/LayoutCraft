# Shared by the Windows packaging scripts (dot-source it). docs/36-packaging.md.
$script:LcRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))   # the repository root

# The version of the two programs (they must agree; tools/packaging/check-version.sh enforces the tag in CI).
function Get-LcVersion {
  $versions = foreach ($crate in 'layoutcraft-app', 'layoutcraft-mcp') {
    $text = Get-Content (Join-Path $script:LcRoot "rust\$crate\Cargo.toml") -Raw
    # the version of the [package] table: the first "version = " line in the file
    if ($text -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw "no version in rust/$crate/Cargo.toml" }
    $Matches[1]
  }
  if ($versions[0] -ne $versions[1]) { throw "layoutcraft-app ($($versions[0])) and layoutcraft-mcp ($($versions[1])) have different versions" }
  $versions[0]
}

# The Rust target of an architecture (msvc; arm64 is cross-compiled).
function Resolve-LcTarget([string]$Arch, [string]$Target) {
  if ($Target) { return $Target }
  @{ x64 = 'x86_64-pc-windows-msvc'; arm64 = 'aarch64-pc-windows-msvc' }[$Arch]
}

# Where cargo leaves the exes of a target (CARGO_TARGET_DIR moves the target folder, as on a machine where the default path is too long).
function Get-LcBinDir([string]$Target) {
  $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $script:LcRoot 'rust\target' }
  Join-Path $targetDir "$Target\release"
}
