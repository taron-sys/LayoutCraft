# Builds <OutDir>\LayoutCraft-<version>-windows-<arch>.msi from layoutcraft.wxs with the WiX Toolset v4+ (`wix` command). The toolset is NOT part of this
# repository and is never downloaded by it: install it yourself once (dotnet tool install --global wix --version <tools/packaging/versions.env>) or let the
# release workflow do it. Without it this script says what is missing and exits with code 2.
#   pwsh tools/installer/build-msi.ps1 [-Arch x64|arm64] [-NoBuild] [-Target <triple>] [-Toolchain +<name>] [-OutDir <folder>]
# Without -NoBuild it builds the exes first (tools/packaging/windows/build.ps1); CI builds and signs them earlier and passes -NoBuild.
param(
  [ValidateSet('x64', 'arm64')][string]$Arch = 'x64',
  [switch]$NoBuild,
  [string]$Target = '',
  [string]$Toolchain = $env:LC_TOOLCHAIN,
  [string]$OutDir = ''
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\packaging\windows\common.ps1')
$layout = $script:LcRoot
$Target = Resolve-LcTarget $Arch $Target
if (-not $OutDir) { $OutDir = Join-Path $layout 'dist' }
$OutDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutDir)   # absolute, for the .NET calls below
$wix = Get-Command wix -ErrorAction SilentlyContinue
if (-not $wix) {
  Write-Warning 'The WiX Toolset v4+ (command "wix") is not installed: no MSI was built. Install it with: dotnet tool install --global wix   (see docs/36-packaging.md)'
  exit 2
}
$version = Get-LcVersion
# An MSI version is numeric (MAJOR.MINOR.BUILD, the first two below 256): a pre-release suffix such as -rc1 cannot be expressed and is dropped.
$msiVersion = ($version -split '[-+]')[0]
if (-not $NoBuild) {
  & (Join-Path $PSScriptRoot '..\packaging\windows\build.ps1') -Arch $Arch -Target $Target -Toolchain $Toolchain
  if (-not $?) { throw 'build failed' }
}
$bin = Get-LcBinDir $Target
New-Item -ItemType Directory -Force $OutDir | Out-Null
$msi = Join-Path $OutDir "LayoutCraft-$version-windows-$Arch.msi"
& wix build (Join-Path $PSScriptRoot 'layoutcraft.wxs') -arch $Arch -d "Version=$msiVersion" -d "Bin=$bin" -d "Layout=$layout" -d "Resources=$(Join-Path $layout 'rust\resources')" -o $msi
if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }
Write-Output "wrote $msi ($((Get-Item $msi).Length) bytes)"
