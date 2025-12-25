#!/bin/bash
# Mouse-TestKit Keychain Build Script
# Creates a compact, standalone Windows executable

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  Mouse-TestKit Keychain Builder${NC}"
echo -e "${BLUE}========================================${NC}"
echo

# Configuration
TARGET_WIN64="x86_64-pc-windows-gnu"
TARGET_WIN32="i686-pc-windows-gnu"
TARGET_LINUX="x86_64-unknown-linux-gnu"
PROFILE="keychain"
BIN_NAME="mouse-testkit-gui"
OUTPUT_DIR="dist"

# Parse arguments
BUILD_WINDOWS=false
BUILD_LINUX=false
BUILD_ALL=false

show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo
    echo "Options:"
    echo "  --windows, -w    Build Windows 64-bit executable"
    echo "  --windows32      Build Windows 32-bit executable"
    echo "  --linux, -l      Build Linux executable"
    echo "  --all, -a        Build all targets"
    echo "  --help, -h       Show this help message"
    echo
    echo "Example:"
    echo "  $0 --windows     # Build Windows exe"
    echo "  $0 --all         # Build all platforms"
}

if [ $# -eq 0 ]; then
    show_help
    exit 0
fi

while [[ $# -gt 0 ]]; do
    case $1 in
        --windows|-w)
            BUILD_WINDOWS=true
            shift
            ;;
        --windows32)
            BUILD_WIN32=true
            shift
            ;;
        --linux|-l)
            BUILD_LINUX=true
            shift
            ;;
        --all|-a)
            BUILD_ALL=true
            shift
            ;;
        --help|-h)
            show_help
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            show_help
            exit 1
            ;;
    esac
done

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Check for required tools
check_requirements() {
    echo -e "${YELLOW}Checking requirements...${NC}"

    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}Error: cargo not found. Install Rust first.${NC}"
        exit 1
    fi

    if [ "$BUILD_WINDOWS" = true ] || [ "$BUILD_ALL" = true ]; then
        if ! rustup target list --installed | grep -q "$TARGET_WIN64"; then
            echo -e "${YELLOW}Installing Windows target...${NC}"
            rustup target add "$TARGET_WIN64"
        fi

        if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
            echo -e "${RED}Warning: mingw-w64 not found. Install with:${NC}"
            echo -e "  Ubuntu/Debian: sudo apt install mingw-w64"
            echo -e "  Fedora: sudo dnf install mingw64-gcc"
            echo -e "  Arch: sudo pacman -S mingw-w64-gcc"
        fi
    fi

    echo -e "${GREEN}Requirements OK${NC}"
    echo
}

# Build for Windows 64-bit
build_windows64() {
    echo -e "${BLUE}Building Windows 64-bit executable...${NC}"

    # Set environment for static linking
    export RUSTFLAGS="-C target-feature=+crt-static -C link-args=-static"

    cargo build \
        --profile "$PROFILE" \
        --bin "$BIN_NAME" \
        --target "$TARGET_WIN64"

    # Copy and rename output
    local SRC="target/$TARGET_WIN64/$PROFILE/$BIN_NAME.exe"
    local DST="$OUTPUT_DIR/MouseTestKit-win64.exe"

    if [ -f "$SRC" ]; then
        cp "$SRC" "$DST"
        local SIZE=$(du -h "$DST" | cut -f1)
        echo -e "${GREEN}Built: $DST ($SIZE)${NC}"
    else
        echo -e "${RED}Build failed: $SRC not found${NC}"
        exit 1
    fi
}

# Build for Windows 32-bit
build_windows32() {
    echo -e "${BLUE}Building Windows 32-bit executable...${NC}"

    rustup target add "$TARGET_WIN32" 2>/dev/null || true

    export RUSTFLAGS="-C target-feature=+crt-static -C link-args=-static"

    cargo build \
        --profile "$PROFILE" \
        --bin "$BIN_NAME" \
        --target "$TARGET_WIN32"

    local SRC="target/$TARGET_WIN32/$PROFILE/$BIN_NAME.exe"
    local DST="$OUTPUT_DIR/MouseTestKit-win32.exe"

    if [ -f "$SRC" ]; then
        cp "$SRC" "$DST"
        local SIZE=$(du -h "$DST" | cut -f1)
        echo -e "${GREEN}Built: $DST ($SIZE)${NC}"
    fi
}

# Build for Linux
build_linux() {
    echo -e "${BLUE}Building Linux executable...${NC}"

    cargo build \
        --profile "$PROFILE" \
        --bin "$BIN_NAME"

    local SRC="target/$PROFILE/$BIN_NAME"
    local DST="$OUTPUT_DIR/MouseTestKit-linux"

    if [ -f "$SRC" ]; then
        cp "$SRC" "$DST"
        chmod +x "$DST"
        local SIZE=$(du -h "$DST" | cut -f1)
        echo -e "${GREEN}Built: $DST ($SIZE)${NC}"
    fi
}

# UPX compression (optional, if installed)
compress_with_upx() {
    if command -v upx &> /dev/null; then
        echo -e "${YELLOW}Compressing with UPX...${NC}"
        for file in "$OUTPUT_DIR"/*; do
            if [ -f "$file" ]; then
                upx --best --lzma "$file" 2>/dev/null || true
                local SIZE=$(du -h "$file" | cut -f1)
                echo -e "${GREEN}Compressed: $file ($SIZE)${NC}"
            fi
        done
    else
        echo -e "${YELLOW}UPX not found. Install for additional compression:${NC}"
        echo "  sudo apt install upx-ucl"
    fi
}

# Main build process
check_requirements

if [ "$BUILD_ALL" = true ]; then
    build_linux
    build_windows64
    [ "$BUILD_WIN32" = true ] && build_windows32
elif [ "$BUILD_WINDOWS" = true ]; then
    build_windows64
elif [ "$BUILD_WIN32" = true ]; then
    build_windows32
elif [ "$BUILD_LINUX" = true ]; then
    build_linux
fi

echo
echo -e "${BLUE}========================================${NC}"
echo -e "${GREEN}Build complete!${NC}"
echo -e "${BLUE}========================================${NC}"
echo
echo "Output files in: $OUTPUT_DIR/"
ls -lh "$OUTPUT_DIR/"
echo
echo -e "${YELLOW}Tip: Use UPX for additional 50-70% size reduction${NC}"
