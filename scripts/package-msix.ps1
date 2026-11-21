<#
.SYNOPSIS
  Package a built Aurora TV folder as an MSIX for the Microsoft Store (docs/RELEASE_2.0.md).

.DESCRIPTION
  Takes the same folder the portable zip is made from — aurora-app.exe, libmpv, the
  licence texts — and lays it out with the manifest and the Store tile set, builds the
  resource index that lets Windows pick a scale-200 logo on a high-DPI display, and packs
  it. The result is unsigned on purpose: Partner Center signs what it publishes. To
  install one locally first, see "Trying the package" in docs/RELEASE_2.0.md.

  Nothing here is Store-specific about the binary. The same exe works out of an MSIX
  because it asks Windows whether it is packaged (aurora-app/src/packaged.rs) and, when
  it is, leaves updating to the Store.

.EXAMPLE
  pwsh scripts/package-msix.ps1 -AppDir 'portable\Aurora TV' -Version 2.0.0 -Out aurora-tv.msix
#>
param(
  [Parameter(Mandatory)] [string] $AppDir,
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $Out,
  # Placeholders until the name is reserved; Partner Center refuses a package whose
  # identity does not match the reservation, so CI passes the real values.
  [string] $IdentityName = 'AuroraTV.Dev',
  [string] $Publisher = 'CN=Aurora TV Development',
  [string] $PublisherDisplayName = 'Aurora TV'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$msix = Join-Path $root 'src-native\crates\aurora-app\msix'

# Store versions are four numbers and the last must be 0 — Partner Center reserves the
# revision for itself.
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must be major.minor.patch, got '$Version'." }
$storeVersion = "$Version.0"

function Find-SdkTool([string] $name) {
  $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  $tool = Get-ChildItem $kits -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^10\.' } |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64\$name" } |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1
  if (-not $tool) { throw "$name not found under $kits. Install the Windows 10/11 SDK." }
  $tool
}
$makeappx = Find-SdkTool 'makeappx.exe'
$makepri = Find-SdkTool 'makepri.exe'

if (-not (Test-Path (Join-Path $AppDir 'aurora-app.exe'))) { throw "No aurora-app.exe in $AppDir." }

$stage = Join-Path ([IO.Path]::GetTempPath()) "aurora-msix-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $stage | Out-Null
try {
  Copy-Item (Join-Path $AppDir '*') $stage -Recurse
  # A packaged copy is never portable: its folder is read-only, and the marker would
  # send the database there. Nor does a Store user need the tester's notes.
  foreach ($f in 'portable.txt', 'HOW-TO-TEST.md', 'data') {
    $p = Join-Path $stage $f
    if (Test-Path $p) { Remove-Item $p -Recurse -Force }
  }
  Copy-Item (Join-Path $msix 'Assets') (Join-Path $stage 'Assets') -Recurse

  # XML-escape the values: a publisher display name with an ampersand is legal and
  # would otherwise produce a manifest that does not parse.
  $esc = { param($s) [Security.SecurityElement]::Escape($s) }
  $manifest = (Get-Content (Join-Path $msix 'AppxManifest.xml') -Raw).
    Replace('{{IDENTITY_NAME}}', (& $esc $IdentityName)).
    Replace('{{PUBLISHER}}', (& $esc $Publisher)).
    Replace('{{PUBLISHER_DISPLAY_NAME}}', (& $esc $PublisherDisplayName)).
    Replace('{{VERSION}}', $storeVersion)
  if ($manifest -match '\{\{[A-Z_]+\}\}') { throw "Unfilled token in the manifest: $($Matches[0])" }
  [IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, [Text.UTF8Encoding]::new($false))

  # resources.pri maps Assets\StoreLogo.png to StoreLogo.scale-100/200.png. Without it
  # the manifest names files that do not exist and the package fails validation.
  $priConfig = Join-Path $stage 'priconfig.xml'
  & $makepri createconfig /cf $priConfig /dq en-US /o | Out-Host
  if ($LASTEXITCODE -ne 0) { throw "makepri createconfig failed ($LASTEXITCODE)." }
  & $makepri new /pr $stage /cf $priConfig /mn (Join-Path $stage 'AppxManifest.xml') /of (Join-Path $stage 'resources.pri') /o | Out-Host
  if ($LASTEXITCODE -ne 0) { throw "makepri new failed ($LASTEXITCODE)." }
  Remove-Item $priConfig

  & $makeappx pack /d $stage /p $Out /o /h SHA256 | Out-Host
  if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed ($LASTEXITCODE)." }
  $mb = [math]::Round((Get-Item $Out).Length / 1MB, 1)
  Write-Host "Packed $Out ($mb MB) as $IdentityName $storeVersion"
}
finally {
  Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}
