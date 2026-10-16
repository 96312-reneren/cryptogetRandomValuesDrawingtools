@echo off
rem ============================================================
rem  Random Picker - native Windows GUI (Win32 + GDI)
rem  Build output: target\x86_64-pc-windows-msvc\release\*.exe
rem ============================================================
setlocal
cd /d "%~dp0"

echo [1/2] Building (release) ...
cargo build --release
if errorlevel 1 (
    echo     Online build failed, retrying from local cache ^(--offline^) ...
    cargo build --release --offline
    if errorlevel 1 (
        echo.
        echo Build failed. Make sure Rust ^(rustup^) and the MSVC toolchain are installed.
        exit /b 1
    )
)

set "EXE="
for %%F in ("target\x86_64-pc-windows-msvc\release\*.exe") do if not defined EXE set "EXE=%%F"
if not defined EXE for %%F in ("target\release\*.exe") do if not defined EXE set "EXE=%%F"

echo.
if defined EXE (
    echo [2/2] Done: %EXE%
    for %%A in ("%EXE%") do echo     size: %%~zA bytes
) else (
    echo [2/2] Build finished, but no .exe was found.
)
echo.
echo Double-click the exe to run. No Python or extra runtime required.
endlocal
