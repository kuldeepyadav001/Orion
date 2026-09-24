@echo off
setlocal enabledelayedexpansion

echo ====================================================================
echo        Orion Windows Distribution Packaging Utility
echo ====================================================================

set "ROOT=%~dp0.."
set "NSIS_DIR=%ROOT%\src-tauri\target\release\bundle\nsis"
set "DIST_DIR=%ROOT%\src-tauri\target\release\bundle\dist_package"

if not exist "%NSIS_DIR%" (
    echo [ERROR] NSIS installer directory not found at:
    echo         %NSIS_DIR%
    echo Please run "npm run tauri build" first.
    pause
    exit /b 1
)

echo Finding latest Orion installer .exe...
for /f "delims=" %%F in ('dir /b /o:-d "%NSIS_DIR%\*.exe" 2^>nul') do (
    set "EXE_NAME=%%F"
    goto :found_exe
)

:found_exe
if "%EXE_NAME%"=="" (
    echo [ERROR] No .exe installer found in %NSIS_DIR%.
    pause
    exit /b 1
)

echo Found installer: %EXE_NAME%
echo Preparing distribution bundle in: %DIST_DIR%

if exist "%DIST_DIR%" rmdir /s /q "%DIST_DIR%"
mkdir "%DIST_DIR%"

copy /y "%NSIS_DIR%\%EXE_NAME%" "%DIST_DIR%\%EXE_NAME%" >nul

:: Create 1-click installer launcher with auto-unblock
(
echo @echo off
echo title Installing Orion Local AI...
echo ===================================================
echo   Orion Private AI Assistant - Setup Launcher
echo ===================================================
echo.
echo [1/2] Unblocking installer permissions...
echo powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-ChildItem -Path '%%~dp0' -Filter '*.exe' | Unblock-File"
powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-ChildItem -Path '%%~dp0' -Filter '*.exe' | Unblock-File"
echo.
echo [2/2] Launching installer...
start "" "%%~dp0%EXE_NAME%"
echo.
echo Setup initiated. You may close this window.
) > "%DIST_DIR%\Install-Orion.bat"

:: Create friendly instructions file
(
echo =====================================================================
echo                Orion - Offline Personal AI Assistant
echo =====================================================================
echo.
echo QUICK INSTALLATION:
echo 1. Double-click "Install-Orion.bat" to start setup without Windows warnings.
echo    OR double-click "%EXE_NAME%" directly.
echo.
echo IF WINDOWS DEFENDER / SMARTSCREEN SHOWS A BLUE NOTICE:
echo - Click "More info"
echo - Click "Run anyway"
echo (This occurs because Orion is newly compiled and runs 100%% locally without cloud tracking^)
echo.
echo REQUIREMENTS:
echo - 8 GB RAM or higher
echo - 64-bit Windows 10 or 11
echo - Runs 100%% offline on your computer. Zero telemetry.
echo =====================================================================
) > "%DIST_DIR%\HOW-TO-INSTALL.txt"

echo Creating distribution ZIP archive...
set "ZIP_OUT=%ROOT%\src-tauri\target\release\bundle\Orion_v0.1.0_Windows_x64.zip"
powershell -NoProfile -Command "Compress-Archive -Path '%DIST_DIR%\*' -DestinationPath '%ZIP_OUT%' -Force"

echo.
echo ====================================================================
echo SUCCESS!
echo Shareable ZIP package created at:
echo %ZIP_OUT%
echo.
echo Send this .zip file to your friend/testers.
echo Browsers will NOT block the .zip file!
echo ====================================================================
pause
