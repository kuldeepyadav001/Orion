# Orion Windows Distribution Packager (PowerShell)
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$NsisDir = Join-Path $Root "src-tauri\target\release\bundle\nsis"
$DistDir = Join-Path $Root "src-tauri\target\release\bundle\dist_package"

Write-Host "=========================================================" -ForegroundColor Cyan
Write-Host "       Orion Windows Packaging & Distribution Tool       " -ForegroundColor Cyan
Write-Host "=========================================================" -ForegroundColor Cyan

if (-not (Test-Path $NsisDir)) {
    Write-Host "Error: NSIS folder not found at: $NsisDir" -ForegroundColor Red
    Write-Host "Please run 'npm run tauri build' first." -ForegroundColor Yellow
    exit 1
}

$Exe = Get-ChildItem -Path $NsisDir -Filter "*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $Exe) {
    Write-Host "Error: No installer executable found in $NsisDir" -ForegroundColor Red
    exit 1
}

Write-Host "Found Installer: $($Exe.Name)" -ForegroundColor Green

if (Test-Path $DistDir) { Remove-Item -Recurse -Force $DistDir }
New-Item -ItemType Directory -Path $DistDir | Out-Null

Copy-Item $Exe.FullName -Destination (Join-Path $DistDir $Exe.Name)

# Create 1-click installer launcher
$BatContent = @"
@echo off
title Installing Orion Local AI...
echo ===================================================
echo   Orion Private AI Assistant - Setup Launcher
echo ===================================================
echo.
echo [1/2] Unblocking installer permissions...
powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-ChildItem -Path '%~dp0' -Filter '*.exe' | Unblock-File"
echo.
echo [2/2] Launching installer...
start "" "%~dp0$($Exe.Name)"
echo.
echo Setup initiated. You may close this window.
"@
Set-Content -Path (Join-Path $DistDir "Install-Orion.bat") -Value $BatContent -Encoding ASCII

# Create Friendly README
$ReadmeContent = @"
=====================================================================
               Orion - Offline Personal AI Assistant
=====================================================================

QUICK INSTALLATION:
1. Double-click "Install-Orion.bat" to start setup without Windows warnings.
   OR double-click "$($Exe.Name)" directly.

IF MICROSOFT EDGE OR WINDOWS SHOWS A WARNING:
- In Edge: Click the three dots (...) -> Click "Keep" -> Click "Keep anyway".
- In Windows: Click "More info" -> Click "Run anyway".
(This appears because Orion is an independent, offline-first application
running entirely on your computer without commercial Microsoft cloud certificates.)

REQUIREMENTS:
- 8 GB RAM or higher
- Windows 10 or 11 (64-bit)
- 100% offline, zero data leaves your PC.
=====================================================================
"@
Set-Content -Path (Join-Path $DistDir "HOW-TO-INSTALL.txt") -Value $ReadmeContent -Encoding UTF8

# Compress to Zip
$ZipPath = Join-Path $Root "src-tauri\target\release\bundle\Orion_v0.1.0_Windows_x64.zip"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path "$DistDir\*" -DestinationPath $ZipPath -Force

Write-Host "`nSUCCESS!" -ForegroundColor Green
Write-Host "Shareable ZIP file created at:" -ForegroundColor Yellow
Write-Host "$ZipPath" -ForegroundColor White
Write-Host "`nSend this .zip file to your friend. Browsers will NOT block it!" -ForegroundColor Cyan
