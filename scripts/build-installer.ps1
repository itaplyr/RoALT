<#
.SYNOPSIS
  Builds the Windows installer: dist-installer\RoALT-Setup-<version>.exe.

.DESCRIPTION
  Release-builds the app (npm run tauri build -- --no-bundle), then compiles installer\RoALT.iss
  with Inno Setup 6. Prints the installer's path and SHA-256. Needs Rust (with the Build Tools'
  C++ workload), Node.js and Inno Setup 6 (https://jrsoftware.org/isdl.php).

.PARAMETER Dev
  Also keeps a copy as installers\RoALT-Setup-<version>-dev-<yyyyMMdd-HHmm>.exe: one installer
  per change, earlier ones never overwritten. The build never offers a RoALT update (ROALT_DEV,
  self_update.rs): the published release would replace it with older code.

.PARAMETER Qa
  The QA team's build: as -Dev (no RoALT updates, symbols kept), named
  installers\RoALT-Setup-<version>-qa-<yyyyMMdd-HHmm>.exe, and the first-run setup replays every time
  RoALT starts (VITE_ROALT_QA, features/onboarding/qa.ts). Never published: release.yml passes neither.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1 -Dev
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1 -Qa
#>
param([switch]$Dev, [switch]$Qa)
$indev = $Dev -or $Qa

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Stop-Build([string]$What, [string]$Fix) {
    Write-Host "build-installer: $What" -ForegroundColor Red
    Write-Host "fix: $Fix"
    exit 1
}

# Runs a native command; its exit code, not its stderr, says whether it worked.
function Invoke-Native([string]$Exe, [string[]]$Arguments, [string]$Fix) {
    $ErrorActionPreference = 'Continue'
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) {
        Stop-Build "$Exe $($Arguments -join ' ') failed (exit $LASTEXITCODE)" $Fix
    }
}

# tauri.conf.json carries the version; package.json and Cargo.toml must agree.
$version = (Get-Content -Raw "$root\src-tauri\tauri.conf.json" | ConvertFrom-Json).version
$npmVersion = (Get-Content -Raw "$root\package.json" | ConvertFrom-Json).version
$cargoVersion = [regex]::Match((Get-Content -Raw "$root\src-tauri\Cargo.toml"), '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $version) { Stop-Build 'src-tauri\tauri.conf.json has no version' 'add "version": "x.y.z" to it' }
if ($npmVersion -ne $version -or $cargoVersion -ne $version) {
    Stop-Build "versions differ: src-tauri\tauri.conf.json $version, package.json $npmVersion, src-tauri\Cargo.toml $cargoVersion" 'set all three to the same version'
}

# Inno Setup first, so a missing compiler fails before the long build.
$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { Stop-Build 'Inno Setup 6.6 or newer (ISCC.exe) was not found' 'install it from https://jrsoftware.org/isdl.php (or: winget install JRSoftware.InnoSetup), then run this again' }

# The encrypted database (SQLCipher) builds OpenSSL from source, which needs a native Perl:
# Git's MSYS Perl doesn't work for MSVC builds.
# A portable Strawberry Perl already on PATH counts too.
$pathPerl = Get-Command perl.exe -All -ErrorAction SilentlyContinue | Where-Object { $_.Source -match 'strawberry' } | Select-Object -First 1
$perl = @("C:\Strawberry\perl\bin", "$env:ProgramFiles\Strawberry\perl\bin", $(if ($pathPerl) { Split-Path $pathPerl.Source })) |
    Where-Object { $_ -and (Test-Path "$_\perl.exe") } | Select-Object -First 1
if (-not $perl) { Stop-Build 'Strawberry Perl was not found (OpenSSL, for the encrypted database, needs it to build)' 'winget install StrawberryPerl.StrawberryPerl, then run this again' }
$env:PATH = "$perl;$env:PATH"

Write-Host "Building RoALT $version (release$(if ($Qa) { ', QA' }))"
# Set or cleared for every build: a release must never inherit them from the shell.
$env:ROALT_DEV = if ($indev) { '1' } else { $null }
$env:VITE_ROALT_QA = if ($Qa) { '1' } else { $null }
# An indev build's PDB has RoALT's function names (debug info goes in the PDB, not the exe), so a
# stuck main thread's stack in the log (`watchdog`) can be named. LTO makes them internal: public
# symbols alone don't name them.
$env:CARGO_PROFILE_RELEASE_STRIP = if ($indev) { 'none' } else { $null }
$env:CARGO_PROFILE_RELEASE_DEBUG = if ($indev) { 'line-tables-only' } else { $null }
Push-Location $root
try {
    if (-not (Test-Path node_modules)) {
        Invoke-Native npm.cmd @('ci') 'check your network connection; if npm says package-lock.json is out of sync, run npm install. Then run this again'
    }
    Invoke-Native npm.cmd @('run', 'tauri', 'build', '--', '--no-bundle', '--', '--locked') 'fix the build error above, then run this again'
} finally {
    Pop-Location
}

# Where cargo put the build (CARGO_TARGET_DIR may move it).
$meta = & cargo metadata --no-deps --format-version 1 --manifest-path "$root\src-tauri\Cargo.toml" | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { Stop-Build 'cargo metadata failed' 'run it yourself to see why' }
$exe = Join-Path $meta.target_directory 'release\roalt.exe'

Write-Host 'Compiling the installer'
$isccArgs = @('/Q', "/DAppVersion=$version", "/DAppNumVersion=$($version -replace '[-+].*$','')", "/DAppExe=$exe", "$root\installer\RoALT.iss")
# Antivirus scanning the fresh setup.exe can make ISCC's resource update fail once (error 110).
foreach ($try in 1..2) {
    $ErrorActionPreference = 'Continue'
    & $iscc @isccArgs
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -eq 0) { break }
    Start-Sleep -Seconds 3
}
if ($LASTEXITCODE -ne 0) { Stop-Build "ISCC failed (exit $LASTEXITCODE)" 'fix the error ISCC names above, then run this again' }

$setup = Get-Item "$root\dist-installer\RoALT-Setup-$version.exe"
if ($indev) {
    New-Item -ItemType Directory -Force "$root\installers" | Out-Null
    $kind = if ($Qa) { 'qa' } else { 'dev' }
    $setup = Copy-Item $setup.FullName "$root\installers\RoALT-Setup-$version-$kind-$(Get-Date -Format 'yyyyMMdd-HHmm').exe" -PassThru
    # This build's exe and PDB beside it: what names its frames in a watchdog log line.
    $syms = New-Item -ItemType Directory -Force ($setup.FullName -replace '\.exe$', '-symbols')
    Copy-Item $exe, ($exe -replace '\.exe$', '.pdb') $syms.FullName
}
Write-Host ''
Write-Host "Installer: $($setup.FullName)" -ForegroundColor Green
Write-Host "SHA-256:   $((Get-FileHash -Algorithm SHA256 $setup.FullName).Hash.ToLower())"
