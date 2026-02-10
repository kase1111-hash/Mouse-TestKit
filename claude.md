# Claude.md - Mouse-TestKit

## Project Overview

Mouse-TestKit is a cross-platform mouse diagnostics and testing utility written in Rust. It provides both GUI and CLI interfaces for testing mouse hardware performance, including polling rate, click response, jitter, DPI accuracy, and more.

## Tech Stack

- **Language**: Rust (Edition 2021, requires 1.70+)
- **GUI Framework**: eframe/egui with egui_plot for graphs
- **Platform Input**: evdev (Linux), winapi Raw Input (Windows)
- **Serialization**: serde/serde_json for config and export
- **Terminal**: crossterm for CLI raw input handling

## Build Commands

```bash
# Debug build (GUI)
cargo build --bin mouse-testkit-gui

# Release build (GUI)
cargo build --release --bin mouse-testkit-gui

# CLI build (Linux only)
cargo build --release --bin mouse-testkit

# Ultra-compact build
cargo build --profile keychain --bin mouse-testkit-gui
```

## Test and Lint Commands

```bash
cargo test --verbose          # Run tests
cargo fmt --check             # Check formatting
cargo fmt                     # Apply formatting
cargo clippy -- -D warnings   # Lint with strict warnings
```

## Project Structure

```
src/
├── main.rs                   # CLI entry point (Linux/Windows)
├── lib.rs                    # Library re-exports (types, analysis)
├── types.rs                  # Shared MouseEvent and MouseButton types
├── input.rs                  # Linux input via evdev
├── input_windows.rs          # Windows input via Raw Input API
├── terminal.rs               # Terminal utilities
├── analysis/                 # Shared analysis logic
│   ├── mod.rs
│   ├── polling.rs            # Polling rate statistics (PollingStats)
│   └── stutter.rs            # Stutter detection algorithm
├── gui/
│   ├── main.rs               # GUI entry point
│   ├── app.rs                # Main application state
│   ├── config.rs             # Configuration persistence (JSON)
│   ├── theme.rs              # Dark theme styling
│   ├── input_bridge.rs       # Raw input bridge for GUI
│   ├── panels/               # Individual test UI panels
│   │   ├── mod.rs
│   │   ├── polling.rs        # Polling rate monitor
│   │   ├── stutter.rs        # Movement irregularity detection
│   │   ├── click.rs          # Click response, stickiness, and lift-off tests
│   │   ├── dpi.rs            # DPI verification
│   │   ├── accel.rs          # Acceleration and angle snapping detection
│   │   ├── double_click.rs   # Double-click detection
│   │   ├── jitter.rs         # Sensor noise measurement
│   │   └── scroll.rs         # Scroll wheel testing (GUI only)
│   └── export/
│       └── mod.rs            # JSON/CSV export
├── tests/                    # Test implementations (CLI)
│   ├── mod.rs
│   ├── polling.rs            # Real-time Hz measurement
│   ├── stutter.rs            # Stutter detection
│   ├── click_response.rs     # Button latency
│   ├── click_sticky.rs       # Stuck click detection
│   ├── liftoff.rs            # Lift-off distance testing
│   ├── dpi.rs                # DPI accuracy verification
│   ├── angle_snap.rs         # Angle snapping detection
│   ├── acceleration.rs       # Acceleration detection
│   ├── double_click.rs       # Switch failure detection
│   ├── jitter.rs             # Sensor noise testing
│   └── standard.rs           # Batch test runner
├── usb/
│   ├── mod.rs
│   └── conflicts.rs          # USB conflict detection (Linux only)
└── display/
    ├── mod.rs
    └── graph.rs              # ASCII graph rendering
```

## Coding Conventions

### Cross-Platform Code

Use conditional compilation for platform-specific code:

```rust
#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};
```

### GUI Panels

Each test panel follows this pattern:
- Struct with internal state (is_running, results, settings)
- `ui(&mut self, ui: &mut egui::Ui)` method for rendering
- State management within the panel

### Test Modules

CLI tests follow this pattern:
- Separate module with `pub fn run()` entry point
- Test-specific data structures for results
- Clear user prompts and output

### Error Handling

- Use `Option<T>` for optional values
- Use `Result<T, E>` for recoverable errors
- Provide graceful fallbacks for missing devices
- Clear error messages for user guidance

### Documentation

- Module-level doc comments (`//!`) for all modules
- Public API documentation on structs and functions
- Test modules with `#[cfg(test)]` attributes

## Platform Support

| Platform | GUI | CLI |
|----------|-----|-----|
| Linux x64 | Yes | Yes |
| Windows x64 | Yes | No |
| macOS ARM64 | Yes | No |
| macOS x64 | Yes | No |

## Configuration

Config is stored as JSON at platform-specific locations:
- **Windows**: `%APPDATA%/mouse-trap/config.json`
- **macOS**: `~/Library/Application Support/mouse-trap/config.json`
- **Linux**: `~/.config/mouse-trap/config.json`

## Key Files

- `Cargo.toml` - Project manifest with two binary targets
- `.cargo/config.toml` - Cross-compilation and build aliases
- `docs/BUILD.md` - Detailed build instructions
- `docs/USER_MANUAL.md` - Usage guide
- `SPEC_SHEET.md` - Technical specifications
