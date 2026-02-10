# Mouse-TestKit

A comprehensive mouse diagnostics and testing utility for precision performance analysis.

[![CI](https://github.com/kase1111-hash/Mouse-TestKit/actions/workflows/ci.yml/badge.svg)](https://github.com/kase1111-hash/Mouse-TestKit/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

## Overview

Mouse-TestKit is a cross-platform mouse testing utility designed to analyze and diagnose mouse performance, connectivity, and reliability. Whether you're a competitive gamer validating your equipment, a QA engineer testing peripherals, or troubleshooting mouse issues, this tool provides comprehensive diagnostics.

## Features

- **Polling Rate Monitor** - Real-time Hz measurement (125Hz to 8000Hz+)
- **Stutter Detection & Graphing** - Visual detection of movement irregularities
- **USB Conflict Detection** - Identifies competing devices on the same controller
- **Click Response Test** - Measures button registration latency
- **Click Stickiness Test** - Detects stuck or delayed click releases
- **Lift-Off Distance Test** - Detects unwanted cursor jumps when lifting
- **DPI Accuracy Test** - Verifies actual vs advertised DPI
- **Angle Snapping Detection** - Identifies artificial movement straightening
- **Acceleration Detection** - Tests for unwanted acceleration curves
- **Double-Click Test** - Detects switch failures causing unintended double-clicks
- **Jitter Test** - Measures sensor noise when stationary
- **Scroll Wheel Test** - Scroll functionality validation

## Installation

### Pre-built Binaries

Download the latest release for your platform from the [Releases](https://github.com/kase1111-hash/Mouse-TestKit/releases) page.

### Building from Source

#### Prerequisites

- [Rust](https://rustup.rs/) 1.70 or later

#### Platform-specific Dependencies

**Linux (Debian/Ubuntu):**
```bash
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev libgtk-3-dev
```

**Windows:**
- Visual Studio Build Tools with C++ workload

**macOS:**
```bash
xcode-select --install
```

#### Build

```bash
# Clone the repository
git clone https://github.com/kase1111-hash/Mouse-TestKit.git
cd Mouse-TestKit

# Build release version
cargo build --release --bin mouse-testkit-gui

# Run
./target/release/mouse-testkit-gui
```

For detailed build instructions, see [docs/BUILD.md](docs/BUILD.md).

## Usage

### GUI Application

```bash
# Run the GUI application
cargo run --release --bin mouse-testkit-gui
```

### CLI Application (Linux only)

```bash
# Run the CLI application
cargo run --release --bin mouse-testkit
```

For detailed usage instructions, see the [User Manual](docs/USER_MANUAL.md).

## Documentation

- [Build Instructions](docs/BUILD.md) - Detailed build guide for all platforms
- [User Manual](docs/USER_MANUAL.md) - Step-by-step guide for each test
- [Specification Sheet](SPEC_SHEET.md) - Technical specifications for all features

## Platform Support

| Platform | GUI | CLI | Notes |
|----------|-----|-----|-------|
| Linux (x64) | Yes | Yes | X11 and Wayland supported |
| Windows (x64) | Yes | No | Windows 10+ |
| macOS (ARM64) | Yes | No | Apple Silicon |
| macOS (x64) | Yes | No | Intel Macs |

## Contributing

Contributions are welcome! Please read our [Contributing Guidelines](CONTRIBUTING.md) before submitting pull requests.

## Security

For information about reporting security vulnerabilities, please see our [Security Policy](SECURITY.md).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with [egui](https://github.com/emilk/egui) - An easy-to-use immediate mode GUI library for Rust
- Input handling via [evdev](https://github.com/cmr/evdev) (Linux) and [winapi](https://github.com/retep998/winapi-rs) (Windows)
