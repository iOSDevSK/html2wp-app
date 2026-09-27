@echo off
setlocal
cd /d "%~dp0"
if errorlevel 1 exit /b 1

echo html2wp Desktop - Windows x64 build
echo Prerequisites and troubleshooting: WINDOWS_BUILD.md

where node.exe >nul 2>nul
if errorlevel 1 goto missing
where npm.cmd >nul 2>nul
if errorlevel 1 goto missing
where rustup.exe >nul 2>nul
if errorlevel 1 goto missing
where cargo.exe >nul 2>nul
if errorlevel 1 goto missing

echo [1/4] Preparing the Windows MSVC target...
rustup target add x86_64-pc-windows-msvc
if errorlevel 1 goto failed

echo [2/4] Installing the locked JavaScript dependencies...
call npm.cmd ci
if errorlevel 1 goto failed

echo [3/4] The html2wp plugin is fetched by the app from GitHub; nothing to verify here.

echo [4/4] Building the application and NSIS installer...
call npm.cmd run bundle:windows
if errorlevel 1 goto failed

if not exist "src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\*-setup.exe" goto failed
echo Build complete. Installer:
dir /b "src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\*-setup.exe"
echo Folder: %CD%\src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis
exit /b 0

:missing
echo Missing Node.js, npm, Rust or Cargo. Install the prerequisites in WINDOWS_BUILD.md and reopen this terminal.
exit /b 1

:failed
echo Build stopped. See the error above and WINDOWS_BUILD.md.
exit /b 1
