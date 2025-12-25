@echo off
REM Mouse-TestKit Keychain Builder for Windows
REM Creates a compact, standalone executable

echo ========================================
echo   Mouse-TestKit Keychain Builder
echo ========================================
echo.

REM Create output directory
if not exist "dist" mkdir dist

REM Set static CRT linking
set RUSTFLAGS=-C target-feature=+crt-static

echo Building with keychain profile (optimized for size)...
cargo build --profile keychain --bin mouse-testkit-gui

if %ERRORLEVEL% neq 0 (
    echo.
    echo Build failed!
    pause
    exit /b 1
)

REM Copy output
if exist "target\keychain\mouse-testkit-gui.exe" (
    copy /Y "target\keychain\mouse-testkit-gui.exe" "dist\MouseTestKit.exe"
    echo.
    echo Build successful!
    echo Output: dist\MouseTestKit.exe
    echo.
    dir dist\MouseTestKit.exe
) else (
    echo.
    echo Error: Build output not found
    pause
    exit /b 1
)

REM Try UPX compression if available
where upx >nul 2>nul
if %ERRORLEVEL% equ 0 (
    echo.
    echo Compressing with UPX...
    upx --best --lzma dist\MouseTestKit.exe
)

echo.
echo ========================================
echo   Build Complete!
echo ========================================
echo.
echo Your portable MouseTestKit.exe is in the 'dist' folder.
echo Copy it to your USB drive for on-the-go mouse testing!
echo.
pause
