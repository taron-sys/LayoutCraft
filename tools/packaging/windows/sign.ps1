# Authenticode signing of the given files (docs/36-packaging.md, "Adding signing"). Does nothing, and succeeds, without a certificate:
# the release is then unsigned. Secrets, read from the environment of THIS step only (never of the build step):
#   WINDOWS_CERT_PFX_BASE64   the code-signing certificate (.pfx), base64
#   WINDOWS_CERT_PASSWORD     its password
#   WINDOWS_TIMESTAMP_URL     optional, default http://timestamp.digicert.com
#   pwsh tools/packaging/windows/sign.ps1 -Path a.exe, b.exe
# Not exercised yet (no certificate exists); signtool comes with the Windows SDK that the GitHub runners have.
param([Parameter(Mandatory)][string[]]$Path)
$ErrorActionPreference = 'Stop'
if (-not $env:WINDOWS_CERT_PFX_BASE64) {
  Write-Output 'No WINDOWS_CERT_PFX_BASE64 secret: not signing (the files stay unsigned).'
  exit 0
}
$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
if (-not $signtool) { throw 'signtool.exe not found (Windows SDK)' }
$stamp = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { 'http://timestamp.digicert.com' }
$pfx = Join-Path ([IO.Path]::GetTempPath()) ("lc-cert-" + [guid]::NewGuid().ToString('N') + '.pfx')
try {
  [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_PFX_BASE64))
  foreach ($file in $Path) {
    & $signtool.FullName sign /f $pfx /p $env:WINDOWS_CERT_PASSWORD /fd sha256 /td sha256 /tr $stamp /d 'LayoutCraft' $file
    if ($LASTEXITCODE -ne 0) { throw "signtool failed for $file" }
    & $signtool.FullName verify /pa $file
    if ($LASTEXITCODE -ne 0) { throw "signature of $file does not verify" }
  }
} finally {
  Remove-Item $pfx -Force -ErrorAction SilentlyContinue
}
