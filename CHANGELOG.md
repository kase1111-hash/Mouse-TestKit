# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Comprehensive project documentation

## [0.1.0] - 2025-01-01

### Added
- Initial release of Mouse-TestKit
- GUI application with egui framework
- CLI application for Linux
- **Polling Rate Monitor** - Real-time Hz measurement with graph visualization
- **Stutter Detection** - Movement irregularity detection with visual graphing
- **USB Conflict Detection** - Shows devices sharing USB controller/hub
- **Click Response Test** - Button latency measurement
- **Click Stickiness Test** - Stuck click detection
- **Lift-Off Distance Test** - Cursor jump detection during lift
- **DPI Accuracy Test** - Actual vs configured DPI verification
- **Angle Snapping Detection** - Artificial movement straightening detection
- **Acceleration Detection** - Unwanted acceleration curve testing
- **Double-Click Test** - Switch failure detection
- **Jitter Test** - Sensor noise measurement at rest
- **Button Durability Test** - Click endurance testing
- **Scroll Wheel Test** - Scroll functionality validation
- **Run All Tests** - Complete test suite execution
- Cross-platform support (Linux, Windows, macOS)
- Test result export functionality
- Configurable test parameters
- Dark theme UI

### Platform Support
- Linux x64 (GUI + CLI) with X11/Wayland
- Windows x64 (GUI only)
- macOS ARM64 (GUI only)
- macOS x64 (GUI only)
