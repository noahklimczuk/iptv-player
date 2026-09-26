<#
.SYNOPSIS
    Get a Windows machine from nothing to running tests-host/run.py.

.DESCRIPTION
    Four things are needed and none of them is installed by default:

      1. A build of Aurora that carries libmpv. This takes the portable zip from the
         latest release rather than building one, because that zip *is* the shipped
         artifact — libmpv-2.dll and all — and building it locally needs an MSVC
         toolchain plus an import library CI generates with `lib /def:`. See
         docs/BUILDING.md if you want the local build anyway.

      2. Microsoft Edge WebDriver, matching the installed WebView2 runtime. tauri-driver
         shells out to it on Windows the way it shells out to WebKitWebDriver on Linux.
         The versions have to agree.

      3. tauri-driver itself, which is a cargo install and the only piece needing Rust.

      4. Python 3. The harness imports nothing outside the standard library.

    Everything lands in .windows\ at the repository root, which is gitignored. Re-runs
    are cheap: anything already present and correct is left alone.

    NOTE: this has not been run. It is written from Tauri's documented Windows
    behaviour and Microsoft's published download layout. Fix it where it is wrong.

.PARAMETER Force
    Re-download even when something is already in place.
#>
[CmdletBinding()]
param([switch]$Force)

$ErrorActionPreference = 'Stop'
# Invoke-WebRequest's progress bar costs more than the download on Windows PowerShell.
$ProgressPreference = 'SilentlyContinue'

$Root  = Split-Path -Parent $PSScriptRoot
$Stage = Join-Path $Root '.windows'
$Repo  = 'noahklimczuk/iptv-player'

function Say([string]$m) { Write-Host "  $m" }
function Step([string]$m) { Write-Host "`n== $m" -ForegroundColor Cyan }
function Warn([string]$m) { Write-Host "  ! $m" -ForegroundColor Yellow }

New-Item -ItemType Directory -Force -Path $Stage | Out-Null

# --- 1. the app -------------------------------------------------------------
Step 'A build to test'
$AppDir = Join-Path $Stage 'app'
$AppExe = Join-Path $AppDir 'aurora-app.exe'

if ((Test-Path $AppExe) -and -not $Force) {
    Say "already here: $AppExe  (re-run with -Force to replace it)"
} else {
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" `
        -Headers @{ 'User-Agent' = 'aurora-bootstrap' }
    $asset = $release.assets | Where-Object { $_.name -like '*portable*.zip' } | Select-Object -First 1
    if (-not $asset) { throw "The latest release ($($release.tag_name)) publishes no portable zip." }

    Say "$($release.tag_name) -> $($asset.name)  ($([math]::Round($asset.size / 1MB)) MB)"
    $zip = Join-Path $Stage $asset.name
    Invoke-WebRequest $asset.browser_download_url -OutFile $zip -UseBasicParsing

    # GitHub publishes the digest; the app checks its own downloads the same way, and
    # there is no reason for this one to be the unchecked link in the chain.
    if ($asset.digest -match '^sha256:(?<hex>[0-9a-f]{64})$') {
        $got = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($got -ne $Matches.hex) {
            Remove-Item $zip -Force
            throw "SHA-256 mismatch on $($asset.name): expected $($Matches.hex), got $got"
        }
        Say 'sha256 ok'
    } else {
        Warn 'the release published no digest for this asset; it was not verified'
    }

    if (Test-Path $AppDir) { Remove-Item $AppDir -Recurse -Force }
    Expand-Archive -Path $zip -DestinationPath $AppDir -Force
    Remove-Item $zip -Force

    # The zip may or may not carry a top-level folder. Flatten it if it does, so the
    # path printed at the end is the path that exists.
    if (-not (Test-Path $AppExe)) {
        $found = Get-ChildItem $AppDir -Recurse -Filter 'aurora-app.exe' | Select-Object -First 1
        if (-not $found) { throw "No aurora-app.exe anywhere in $($asset.name)." }
        Get-ChildItem $found.DirectoryName | Move-Item -Destination $AppDir -Force
        Get-ChildItem $AppDir -Directory |
            Where-Object { -not (Get-ChildItem $_.FullName -Force) } |
            Remove-Item -Recurse -Force
    }
    Say "unpacked to $AppDir"
}

# libmpv is the entire point of running here rather than on Linux, so say plainly
# whether it arrived.
$mpv = Get-ChildItem $AppDir -Filter '*mpv*.dll' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($mpv) { Say "libmpv: $($mpv.Name)" }
else { Warn 'no mpv DLL beside the exe — playback will fall back to the null backend' }

# --- 2. Microsoft Edge WebDriver -------------------------------------------
Step 'Microsoft Edge WebDriver'

function Get-WebView2Version {
    # The runtime registers itself under EdgeUpdate with this fixed GUID. Per-machine
    # installs land in HKLM (under WOW6432Node on a 64-bit OS); per-user in HKCU.
    $guid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    foreach ($hive in 'HKLM:\SOFTWARE\WOW6432Node', 'HKLM:\SOFTWARE', 'HKCU:\SOFTWARE') {
        $key = "$hive\Microsoft\EdgeUpdate\Clients\$guid"
        $pv = (Get-ItemProperty -Path $key -Name pv -ErrorAction SilentlyContinue).pv
        if ($pv) { return $pv }
    }
    # Edge itself is the next best answer: the runtime tracks it closely, and a machine
    # with Edge but no registered runtime still has a WebView2 to drive.
    foreach ($p in "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
                   "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") {
        if (Test-Path $p) { return (Get-Item $p).VersionInfo.ProductVersion }
    }
    return $null
}

function Get-LatestDriverVersion {
    $raw = (Invoke-WebRequest 'https://msedgedriver.microsoft.com/LATEST_STABLE' -UseBasicParsing).Content
    # That endpoint serves octet-stream, so PowerShell hands back raw bytes rather
    # than a string — and the bytes are UTF-16 with a BOM. Interpolating the array
    # straight into a URL spells the version as "255 254 49 0 53 0 ...", which the
    # CDN answers with BlobNotFound. Decode it properly, then keep only what a
    # version is made of, which also takes the BOM off the front.
    if ($raw -is [byte[]]) { $raw = [System.Text.Encoding]::Unicode.GetString($raw) }
    return ($raw -replace '[^\d\.]', '').Trim()
}

$DriverExe = Join-Path $Stage 'msedgedriver.exe'
if ((Test-Path $DriverExe) -and -not $Force) {
    Say "already here: $DriverExe  (re-run with -Force to replace it)"
} else {
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'win64' }
    $zip = Join-Path $Stage 'edgedriver.zip'

    # Best first: the exact build of the runtime that will actually be driven. Not
    # every WebView2 build has a driver published for it, so latest-stable is the
    # fallback — a near miss usually works, and when it does not the session error
    # names both versions, which is at least an honest failure.
    $tries = @()
    $version = Get-WebView2Version
    if ($version) {
        Say "WebView2 runtime: $version"
        $tries += $version
    } else {
        Warn 'no WebView2 runtime found. Aurora needs one to draw anything at all —'
        Warn 'install it from https://developer.microsoft.com/microsoft-edge/webview2/'
    }
    $tries += Get-LatestDriverVersion

    $got = $false
    foreach ($v in ($tries | Select-Object -Unique)) {
        $url = "https://msedgedriver.microsoft.com/$v/edgedriver_$arch.zip"
        try {
            Invoke-WebRequest $url -OutFile $zip -UseBasicParsing
            Say "fetched $url"
            $got = $true
            break
        } catch {
            Warn "no driver published for $v ($arch)"
        }
    }
    if (-not $got) { throw "Could not fetch a $arch Edge WebDriver for any of: $($tries -join ', ')" }

    $tmp = Join-Path $Stage 'edgedriver'
    if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $found = Get-ChildItem $tmp -Recurse -Filter 'msedgedriver.exe' | Select-Object -First 1
    if (-not $found) { throw 'The Edge WebDriver archive contained no msedgedriver.exe.' }
    Move-Item $found.FullName $DriverExe -Force
    Remove-Item $tmp -Recurse -Force; Remove-Item $zip -Force
    Say "installed $DriverExe"
}

# --- 3. tauri-driver --------------------------------------------------------
Step 'tauri-driver'
if (Get-Command tauri-driver -ErrorAction SilentlyContinue) {
    Say 'already on PATH'
} elseif (Get-Command cargo -ErrorAction SilentlyContinue) {
    Say 'cargo install tauri-driver --locked  (a few minutes)'
    & cargo install tauri-driver --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo install tauri-driver failed.' }
} else {
    Warn 'Rust is not installed, and tauri-driver is a cargo install.'
    Warn 'Install rustup from https://rustup.rs, then re-run this script.'
    Warn 'stable-x86_64-pc-windows-gnu is enough here and needs no Visual Studio.'
    throw 'tauri-driver is missing.'
}

# --- 4. Python --------------------------------------------------------------
Step 'Python'
$py = Get-Command python -ErrorAction SilentlyContinue
if (-not $py) { $py = Get-Command py -ErrorAction SilentlyContinue }
if ($py) { Say (& $py.Source --version) } else { Warn 'No python on PATH — install 3.8 or later.' }

# --- what to do next --------------------------------------------------------
Write-Host "`nReady. From this shell:" -ForegroundColor Green
Write-Host @"

  `$env:AURORA_TEST_EXE      = "$AppExe"
  `$env:AURORA_NATIVE_DRIVER = "$DriverExe"

  # The real panel, for the scenarios that need one. Typed straight into the
  # window — never written to a file, an argument or a log.
  `$env:AURORA_PANEL_URL  = "https://..."
  `$env:AURORA_PANEL_USER = "..."
  `$env:AURORA_PANEL_PASS = "..."

  python tests-host\run.py

Run it from a desktop session — RDP or the console. A GUI app started over SSH
or from a service has no desktop to draw on and WebView2 will not start at all.
docs/TESTING_ON_WINDOWS.md has the rest.
"@
