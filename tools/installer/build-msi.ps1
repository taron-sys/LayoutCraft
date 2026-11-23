# Builds dist\LayoutCraft-<version>.msi from layoutcraft.wxs with the WiX Toolset v4+ (`wix` command). The toolset is NOT part of this repository and is never
# downloaded by it: install it yourself once (dotnet tool install --global wix) or let the release workflow do it. Without it this script says what is missing.
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
$layout = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)   # the repository root
$rust = Join-Path $layout 'rust'
$dist = Join-Path $layout 'dist'
$wix = Get-Command wix -ErrorAction SilentlyContinue
if (-not $wix) {
  Write-Warning 'The WiX Toolset v4+ (command "wix") is not installed: no MSI was built. Install it with: dotnet tool install --global wix   (see docs/36-packaging.md)'
  exit 2
}
$cargo = Get-Content (Join-Path $rust 'layoutcraft-app\Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'no version in layoutcraft-app/Cargo.toml' }
$version = $Matches[1]
if (-not $NoBuild) {
  Push-Location $rust
  try { cargo +stable-x86_64-pc-windows-gnu build --release -p layoutcraft-app -p layoutcraft-mcp; if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' } } finally { Pop-Location }
}
New-Item -ItemType Directory -Force $dist | Out-Null
$msi = Join-Path $dist "LayoutCraft-$version.msi"
& wix build (Join-Path $PSScriptRoot 'layoutcraft.wxs') -arch x64 -d Version=$version -d "Bin=$(Join-Path $rust 'target\release')" -d "Layout=$layout" -d "Resources=$(Join-Path $rust 'resources')" -o $msi
if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }
Write-Output "wrote $msi ($((Get-Item $msi).Length) bytes)"
