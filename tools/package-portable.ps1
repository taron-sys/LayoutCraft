# Portable package of the native programs (docs/36-packaging.md): one zip with layoutcraft-app.exe, layoutcraft-mcp.exe, the third-party notices,
# the licence files (when they exist) and a short README. No installer, nothing is written outside the folder it is unpacked to.
#   powershell -File tools\package-portable.ps1 [-NoBuild]
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
$layout = Split-Path -Parent $PSScriptRoot                      # the repository root
$rust = Join-Path $layout 'rust'
$dist = Join-Path $layout 'dist'                                # <repository>\dist (git-ignored)
New-Item -ItemType Directory -Force $dist | Out-Null

$cargo = Get-Content (Join-Path $rust 'layoutcraft-app\Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'no version in layoutcraft-app/Cargo.toml' }
$version = $Matches[1]
# the toolchain argument of cargo: the gnu toolchain of this machine by default; CI sets LC_TOOLCHAIN=+stable
$toolchain = if ($env:LC_TOOLCHAIN) { $env:LC_TOOLCHAIN } else { '+stable-x86_64-pc-windows-gnu' }

# Keep the maintainer's build paths (user name, home folder) out of the binaries: rustc embeds the source paths of dependencies.
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$cargoHome=/cargo" + [char]0x1f + "--remap-path-prefix=$((Resolve-Path (Join-Path $rust '..')).Path)=/layoutcraft"
if (-not $NoBuild) {
  Push-Location $rust
  try {
    cargo $toolchain build --release -p layoutcraft-app -p layoutcraft-mcp
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
  } finally { Pop-Location }
}

$stage = Join-Path ([IO.Path]::GetTempPath()) ("lc-portable-" + [guid]::NewGuid().ToString('N'))
$folder = Join-Path $stage "LayoutCraft-$version"
New-Item -ItemType Directory -Force $folder | Out-Null
foreach ($exe in 'layoutcraft-app.exe', 'layoutcraft-mcp.exe') {
  $src = Join-Path $rust "target\release\$exe"
  if (-not (Test-Path $src)) { throw "missing $src (build first)" }
  Copy-Item $src $folder
}
Copy-Item (Join-Path $layout 'THIRD-PARTY-NOTICES.txt') $folder
Copy-Item (Join-Path $layout 'PROVENANCE.md') $folder
foreach ($name in 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE', 'NOTICE') {
  $p = Join-Path $layout $name
  if (Test-Path $p) { Copy-Item $p $folder } elseif ($name -ne 'LICENSE' -and $name -ne 'NOTICE') { Write-Warning "$name is missing (docs/36-packaging.md)" }
}
$readme = @"
LayoutCraft $version (portable)

Start:   double-click layoutcraft-app.exe  (or: layoutcraft-app.exe drawing.vsdx)
Nothing is installed; settings are not written outside your own files.

AI assistants (MCP): layoutcraft-mcp.exe is the server an AI client (for example Claude) starts itself; it needs no node and opens no network port.
Open Help > AI assistant in the app: it shows the configuration with the right path (keep the two .exe files in the same folder) and the drawings folder
the assistant may read and write.

THIRD-PARTY-NOTICES.txt lists the libraries inside the programs with their licences.
PROVENANCE.md states what the project's provenance rules allow and what this package does not contain.
"@
Set-Content -Path (Join-Path $folder 'README-portable.txt') -Value $readme -Encoding UTF8

$zip = Join-Path $dist "LayoutCraft-portable-$version.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
# .NET ZipFile with '/' in the entry names (Compress-Archive of Windows PowerShell 5.1 writes '\', which other unzip tools mishandle)
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::Open($zip, 'Create')
try {
  foreach ($file in Get-ChildItem $folder -File) {
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, "LayoutCraft-$version/$($file.Name)", [IO.Compression.CompressionLevel]::Optimal)
  }
} finally { $archive.Dispose() }
Remove-Item $stage -Recurse -Force

$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
$sums = Join-Path $dist 'SHA256SUMS.txt'
$lines = @()
if (Test-Path $sums) { $lines = @(Get-Content $sums | Where-Object { $_ -notmatch [regex]::Escape((Split-Path $zip -Leaf)) }) }
$lines += "$hash  $(Split-Path $zip -Leaf)"
Set-Content -Path $sums -Value $lines -Encoding ASCII
Write-Output "wrote $zip ($((Get-Item $zip).Length) bytes), sha256 $hash"
