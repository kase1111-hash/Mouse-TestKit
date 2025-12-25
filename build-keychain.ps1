# Mouse-TestKit Keychain Builder for Windows
# PowerShell build script for creating compact Windows executables

param(
    [switch]$Release,
    [switch]$Keychain,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

# Configuration
$BinName = "mouse-testkit-gui"
$OutputDir = "dist"
$Profile = if ($Keychain) { "keychain" } else { "release" }

function Show-Banner {
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host "  Mouse-TestKit Keychain Builder" -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host ""
}

function Show-Help {
    Write-Host "Usage: .\build-keychain.ps1 [OPTIONS]"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Release     Build with release profile"
    Write-Host "  -Keychain    Build with keychain profile (smallest)"
    Write-Host "  -Help        Show this help message"
    Write-Host ""
    Write-Host "Example:"
    Write-Host "  .\build-keychain.ps1 -Keychain"
}

function Test-Requirements {
    Write-Host "Checking requirements..." -ForegroundColor Yellow

    # Check for Rust
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "Error: cargo not found. Install Rust from https://rustup.rs" -ForegroundColor Red
        exit 1
    }

    # Check Rust version
    $rustVersion = rustc --version
    Write-Host "Found: $rustVersion" -ForegroundColor Green

    Write-Host "Requirements OK" -ForegroundColor Green
    Write-Host ""
}

function Build-Executable {
    Write-Host "Building with profile: $Profile" -ForegroundColor Cyan

    # Create output directory
    New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

    # Set environment for static CRT
    $env:RUSTFLAGS = "-C target-feature=+crt-static"

    # Build
    cargo build --profile $Profile --bin $BinName

    if ($LASTEXITCODE -ne 0) {
        Write-Host "Build failed!" -ForegroundColor Red
        exit 1
    }

    # Determine source path based on profile
    $ProfileDir = if ($Profile -eq "keychain") { "keychain" } else { "release" }
    $SrcPath = "target\$ProfileDir\$BinName.exe"
    $DstPath = "$OutputDir\MouseTestKit.exe"

    if (Test-Path $SrcPath) {
        Copy-Item $SrcPath $DstPath -Force
        $size = (Get-Item $DstPath).Length / 1MB
        Write-Host "Built: $DstPath ($([math]::Round($size, 2)) MB)" -ForegroundColor Green
    } else {
        Write-Host "Error: Build output not found at $SrcPath" -ForegroundColor Red
        exit 1
    }
}

function Compress-WithUpx {
    if (Get-Command upx -ErrorAction SilentlyContinue) {
        Write-Host "Compressing with UPX..." -ForegroundColor Yellow
        upx --best --lzma "$OutputDir\MouseTestKit.exe"
        $size = (Get-Item "$OutputDir\MouseTestKit.exe").Length / 1MB
        Write-Host "Compressed: $([math]::Round($size, 2)) MB" -ForegroundColor Green
    } else {
        Write-Host "UPX not found. Download from https://upx.github.io for additional compression" -ForegroundColor Yellow
    }
}

function Show-Results {
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host "Build complete!" -ForegroundColor Green
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Output:" -ForegroundColor White
    Get-ChildItem $OutputDir | Format-Table Name, @{N='Size (MB)';E={[math]::Round($_.Length/1MB, 2)}}
}

# Main
Show-Banner

if ($Help) {
    Show-Help
    exit 0
}

Test-Requirements
Build-Executable
Compress-WithUpx
Show-Results

Write-Host ""
Write-Host "Your portable MouseTestKit.exe is ready in the 'dist' folder!" -ForegroundColor Green
Write-Host "Copy it to your USB drive for on-the-go mouse testing." -ForegroundColor Cyan
