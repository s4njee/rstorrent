<#
.SYNOPSIS
    Assemble a portable Windows build of the GPUI shell, with the WSL runtime
    beside it, and zip it.

.DESCRIPTION
    The Windows counterpart of tools/bundle-gpui-macos.sh. The GPUI shell has no
    bundler, so this is that bundler: it builds the web console the binary
    embeds (when stale), builds the binary, lays out

        dist-gpui\windows\rstorrent\
            rstorrent-gpui.exe
            runtime\
                rstorrent-rootfs.tar.gz

    and zips that folder to
    dist-gpui\windows\rstorrent-<version>-windows-x64.zip.

    The runtime is a WSL root filesystem (Alpine plus a static rtorrent) built
    by tools/build-rtorrent-wsl.sh into binaries\rtorrent-wsl\. That script
    is bash and builds on Linux x86-64 as root, so it is not run from here:
    run it inside WSL on this machine (sudo tools/build-rtorrent-wsl.sh from
    the checkout under /mnt/c), or from another machine with
    BUILD_HOST=root@<linux-host>, or pass -SkipRuntime.

    The app looks for the rootfs at <exe dir>\runtime\rstorrent-rootfs.tar.gz
    (see crates/gpui/src/daemon_wsl.rs, ROOTFS_BESIDE_EXE), so that layout is a
    contract - keep the two in step.

    The exe's icon and version resource are embedded at compile time by
    crates/gpui/build.rs; nothing here touches them.

    Follow-up: there is no installer yet. An installer (MSIX, WiX or NSIS)
    should also create a Start-menu shortcut carrying the AppUserModelID
    "com.rstorrent.app", so completion toasts are attributed to rstorrent
    rather than PowerShell (see crates/gpui/src/notifications.rs).

    Prerequisites: the Rust toolchain (MSVC target, with the Windows SDK's
    rc.exe and fxc.exe) and Node/npm for the web console.

    Compatible with Windows PowerShell 5.1 and PowerShell 7.

.PARAMETER SkipRuntime
    Package without the WSL rootfs. The app then cannot start a bundled
    daemon and can only connect to an existing one.

.PARAMETER ForceWebBuild
    Rebuild the web console even when its output looks current.

.PARAMETER BuildProfile
    Cargo profile to build and package: release (default) or debug.

.EXAMPLE
    pwsh tools\bundle-gpui-windows.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\bundle-gpui-windows.ps1 -SkipRuntime
#>
[CmdletBinding()]
param(
    [switch]$SkipRuntime,
    [switch]$ForceWebBuild,
    [ValidateSet('release', 'debug')]
    [string]$BuildProfile = 'release'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Say([string]$Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Die([string]$Message) {
    Write-Host "error: $Message" -ForegroundColor Red
    exit 1
}

# Native commands do not throw on failure (in 5.1 at all, in 7 only with
# $PSNativeCommandUseErrorActionPreference), so check each exit code instead.
# 'Stop' is relaxed around the call because some Windows PowerShell 5.1 hosts
# turn a native command's stderr (cargo's progress lines) into a terminating
# error.
function Invoke-Native([string]$What, [scriptblock]$Command) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Command
    }
    finally {
        $ErrorActionPreference = $previous
    }
    if ($LASTEXITCODE -ne 0) {
        Die "$What failed (exit code $LASTEXITCODE)"
    }
}

# $IsWindows only exists in PowerShell 6+; Windows PowerShell is always Windows.
if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
    Die 'this script builds the Windows package; run it on Windows.'
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Binary = 'rstorrent-gpui'
$Rootfs = Join-Path $RepoRoot 'binaries\rtorrent-wsl\rstorrent-rootfs.tar.gz'
$OutDir = Join-Path $RepoRoot 'dist-gpui\windows'
$Stage = Join-Path $OutDir 'rstorrent'

# The package version is the GPUI crate's, as the macOS bundle's is.
$CargoToml = Get-Content -LiteralPath (Join-Path $RepoRoot 'crates\gpui\Cargo.toml')
$VersionLine = $CargoToml | Where-Object { $_ -match '^version\s*=\s*"([^"]+)"' } | Select-Object -First 1
if (-not $VersionLine) {
    Die 'could not read the version from crates\gpui\Cargo.toml'
}
$null = $VersionLine -match '^version\s*=\s*"([^"]+)"'
$Version = $Matches[1]
$Zip = Join-Path $OutDir "rstorrent-$Version-windows-x64.zip"

# --- 1. The runtime has to exist before we can package it -----------------
# Unlike the macOS script this does not build the runtime itself: the rootfs
# build is a bash script that needs Linux x86-64 and root (WSL works).
# Checked first so a missing tarball fails in seconds, not after a long build.
if (-not $SkipRuntime -and -not (Test-Path -LiteralPath $Rootfs -PathType Leaf)) {
    Die ("no WSL runtime at $Rootfs. Build it with tools/build-rtorrent-wsl.sh " +
        'inside WSL (sudo tools/build-rtorrent-wsl.sh) or with BUILD_HOST=root@<linux-host>, or pass ' +
        '-SkipRuntime to package without it.')
}

# --- 2. The web console the binary embeds -----------------------------------
# rust-embed reads dist-web at *compile* time, so it must exist and be current
# before cargo runs. Rebuilding it unconditionally would change files under
# dist-web and force a full relink on every package, so only rebuild when a
# source is newer than the console's entry (web.html; its assets are
# content-hashed). -ForceWebBuild when the timestamps lie.
$ConsoleEntry = Join-Path $RepoRoot 'dist-web\web.html'
$ConsoleStale = $true
if (Test-Path -LiteralPath $ConsoleEntry -PathType Leaf) {
    $Built = (Get-Item -LiteralPath $ConsoleEntry).LastWriteTimeUtc
    $Sources = @()
    foreach ($dir in 'src', 'public') {
        $path = Join-Path $RepoRoot $dir
        if (Test-Path -LiteralPath $path -PathType Container) {
            $Sources += Get-ChildItem -LiteralPath $path -Recurse -File
        }
    }
    foreach ($file in 'web.html', 'vite.web.config.ts', 'tsconfig.json', 'package.json') {
        $path = Join-Path $RepoRoot $file
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $Sources += Get-Item -LiteralPath $path
        }
    }
    $Newer = $Sources | Where-Object { $_.LastWriteTimeUtc -gt $Built } | Select-Object -First 1
    $ConsoleStale = [bool]$Newer
}
if ($ForceWebBuild) {
    $ConsoleStale = $true
}

Push-Location $RepoRoot
try {
    if ($ConsoleStale) {
        if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
            Die 'npm is needed to build the web console'
        }
        if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot 'node_modules') -PathType Container)) {
            Say 'Installing frontend dependencies'
            Invoke-Native 'npm ci' { npm ci }
        }
        Say 'Building the web console'
        Invoke-Native 'npm run build:web' { npm run build:web }
    }
    else {
        Say 'Web console is up to date'
    }

    # --- 3. Build the binary -------------------------------------------------
    Say "Building $Binary ($BuildProfile)"
    if ($BuildProfile -eq 'release') {
        Invoke-Native 'cargo build' { cargo build --release -p $Binary }
    }
    else {
        Invoke-Native 'cargo build' { cargo build -p $Binary }
    }

    # Ask cargo where the target directory is, so CARGO_TARGET_DIR and
    # .cargo/config.toml overrides are honoured.
    $MetadataJson = Invoke-Native 'cargo metadata' { cargo metadata --format-version 1 --no-deps }
    $TargetDir = ($MetadataJson | Out-String | ConvertFrom-Json).target_directory
}
finally {
    Pop-Location
}

$Exe = Join-Path $TargetDir "$BuildProfile\$Binary.exe"
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) {
    Die "no binary at $Exe"
}

# --- 4. Lay out the package -------------------------------------------------
Say "Assembling $Stage"
if (Test-Path -LiteralPath $Stage) {
    Remove-Item -LiteralPath $Stage -Recurse -Force
}
$null = New-Item -ItemType Directory -Path $Stage -Force
Copy-Item -LiteralPath $Exe -Destination (Join-Path $Stage "$Binary.exe")

# The app resolves the rootfs as <exe dir>\runtime\rstorrent-rootfs.tar.gz.
if ($SkipRuntime) {
    Write-Warning 'packaging without the WSL runtime (-SkipRuntime): the app can only connect to an existing daemon.'
}
else {
    $Runtime = Join-Path $Stage 'runtime'
    $null = New-Item -ItemType Directory -Path $Runtime -Force
    Copy-Item -LiteralPath $Rootfs -Destination (Join-Path $Runtime 'rstorrent-rootfs.tar.gz')
}

# --- 5. Zip it ---------------------------------------------------------------
# ZipFile rather than Compress-Archive: Windows PowerShell 5.1's
# Compress-Archive writes backslash entry names, which other unzip tools
# mis-handle. includeBaseDirectory keeps the rstorrent\ folder in the zip.
Say "Zipping $Zip"
Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path -LiteralPath $Zip) {
    Remove-Item -LiteralPath $Zip -Force
}
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $Stage, $Zip, [System.IO.Compression.CompressionLevel]::Optimal, $true)

$Size = '{0:N1} MB' -f ((Get-Item -LiteralPath $Zip).Length / 1MB)
Write-Host ''
Write-Host "Built $Zip ($Size)"
Write-Host "  Folder   $Stage"
if ($SkipRuntime) {
    Write-Host '  Runtime  (none; -SkipRuntime)'
}
else {
    Write-Host '  Runtime  runtime\rstorrent-rootfs.tar.gz'
}
