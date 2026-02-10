# Building Mouse-TestKit

## Prerequisites

### All Platforms
- [Rust](https://rustup.rs/) (1.70 or later)

### Linux
```bash
# Debian/Ubuntu
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev libgtk-3-dev

# Fedora
sudo dnf install libxcb-devel libxkbcommon-devel openssl-devel gtk3-devel

# Arch
sudo pacman -S libxcb libxkbcommon openssl gtk3
```

### Windows
- Visual Studio Build Tools with C++ workload
- Or: `winget install Microsoft.VisualStudio.2022.BuildTools`

### macOS
```bash
xcode-select --install
```

---

## Build Commands

### Debug Build (fast compile, slower runtime)
```bash
cargo build --bin mouse-testkit-gui
```

### Release Build (optimized)
```bash
cargo build --release --bin mouse-testkit-gui
```

### CLI Build (Linux/Windows)
```bash
cargo build --release --bin mouse-testkit
```

### Ultra-Compact Build
```bash
cargo build --profile keychain --bin mouse-testkit-gui
```

---

## Output Locations

| Build Type | Binary Location |
|------------|-----------------|
| Debug (GUI) | `target/debug/mouse-testkit-gui` |
| Release (GUI) | `target/release/mouse-testkit-gui` |
| Release (CLI) | `target/release/mouse-testkit` |
| Keychain (GUI) | `target/keychain/mouse-testkit-gui` |

---

## Running

```bash
# GUI (Debug)
cargo run --bin mouse-testkit-gui

# GUI (Release)
cargo run --release --bin mouse-testkit-gui

# CLI (Linux/Windows, requires raw input access)
cargo run --release --bin mouse-testkit

# Or run binaries directly
./target/release/mouse-testkit-gui
./target/release/mouse-testkit
```

---

## Running Tests

```bash
cargo test
```

---

## Troubleshooting

### Linux: "error: failed to run custom build command for `wayland-sys`"
Install Wayland development libraries:
```bash
sudo apt install libwayland-dev
```

### Linux: "error: linker `cc` not found"
Install build essentials:
```bash
sudo apt install build-essential
```

### Windows: "LINK : fatal error LNK1181"
Install Visual Studio Build Tools with C++ workload.

### macOS: "xcrun: error: invalid active developer path"
Run:
```bash
xcode-select --install
```

---

## Cross-Compilation

### Linux to Windows
```bash
rustup target add x86_64-pc-windows-gnu
sudo apt install mingw-w64
cargo build --release --target x86_64-pc-windows-gnu --bin mouse-testkit-gui
```

Output: `target/x86_64-pc-windows-gnu/release/mouse-testkit-gui.exe`
