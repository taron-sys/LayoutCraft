# Portable package of the native programs (docs/36-packaging.md): one zip with layoutcraft-app.exe, layoutcraft-mcp.exe, the third-party notices,
# the licence files, PROVENANCE.md and a short README. No installer, nothing is written outside the folder it is unpacked to.
#   pwsh tools/package-portable.ps1 [-Arch x64|arm64] [-NoBuild] [-Target <triple>] [-Toolchain +<name>] [-OutDir <folder>]
# Writes <OutDir>\LayoutCraft-<version>-windows-<arch>-portable.zip (default OutDir: <repository>\dist, git-ignored).
# Without -NoBuild it builds first (tools/packaging/windows/build.ps1); CI builds and signs in separate steps and passes -NoBuild.
# The checksums of all release files are written by the release workflow (SHA256SUMS.txt), not here.
param(
  [ValidateSet('x64', 'arm64')][string]$Arch = 'x64',
  [switch]$NoBuild,
  [string]$Target = '',
  [string]$Toolchain = $env:LC_TOOLCHAIN,
  [string]$OutDir = ''
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'packaging\windows\common.ps1')
$layout = $script:LcRoot
$Target = Resolve-LcTarget $Arch $Target
if (-not $OutDir) { $OutDir = Join-Path $layout 'dist' }
$OutDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutDir)   # absolute, for the .NET calls below
New-Item -ItemType Directory -Force $OutDir | Out-Null
$version = Get-LcVersion

if (-not $NoBuild) {
  & (Join-Path $PSScriptRoot 'packaging\windows\build.ps1') -Arch $Arch -Target $Target -Toolchain $Toolchain
  if (-not $?) { throw 'build failed' }
}
$bin = Get-LcBinDir $Target

$stage = Join-Path ([IO.Path]::GetTempPath()) ("lc-portable-" + [guid]::NewGuid().ToString('N'))
$folder = Join-Path $stage "LayoutCraft-$version"
New-Item -ItemType Directory -Force $folder | Out-Null
foreach ($exe in 'layoutcraft-app.exe', 'layoutcraft-mcp.exe') {
  $src = Join-Path $bin $exe
  if (-not (Test-Path $src)) { throw "missing $src (build first)" }
  Copy-Item $src $folder
}
foreach ($name in 'THIRD-PARTY-NOTICES.txt', 'PROVENANCE.md', 'LICENSE-MIT', 'LICENSE-APACHE') {
  $p = Join-Path $layout $name
  if (-not (Test-Path $p)) { throw "$name is missing from the repository root" }
  Copy-Item $p $folder
}
$readme = @"
LayoutCraft $version (portable, Windows $Arch)

Start:   double-click layoutcraft-app.exe  (or: layoutcraft-app.exe drawing.vsdx)
Nothing is installed; settings are not written outside your own files.

AI assistants (MCP): layoutcraft-mcp.exe is the server an AI client (for example Claude) starts itself; it needs no node and opens no network port.
Open Help > AI assistant in the app: it shows the configuration with the right path (keep the two .exe files in the same folder) and the drawings folder
the assistant may read and write.

THIRD-PARTY-NOTICES.txt lists the libraries inside the programs with their licences.
PROVENANCE.md states what the project's provenance rules allow and what this package does not contain.
LICENSE-MIT and LICENSE-APACHE: LayoutCraft is available under either licence.
"@
Set-Content -Path (Join-Path $folder 'README-portable.txt') -Value $readme -Encoding UTF8

$zip = Join-Path $OutDir "LayoutCraft-$version-windows-$Arch-portable.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
# .NET ZipFile with '/' in the entry names (Compress-Archive of Windows PowerShell 5.1 writes '\', which other unzip tools mishandle)
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::Open($zip, 'Create')
try {
  foreach ($file in Get-ChildItem $folder -File | Sort-Object Name) {
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, "LayoutCraft-$version/$($file.Name)", [IO.Compression.CompressionLevel]::Optimal)
  }
} finally { $archive.Dispose() }
Remove-Item $stage -Recurse -Force

Write-Output "wrote $zip ($((Get-Item $zip).Length) bytes)"
