# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Auto Diagnostics**: a guided one-minute check-up (GUI panel and CLI menu
  option 13) that looks for faults by itself across five steps (rest, movement,
  clicks, scroll, lift) and reports a verdict with explained findings: phantom
  movement/clicks/scrolls at rest, stutters and unstable or very low polling
  rate, switch bounce (accidental double clicks), micro-presses, sticky and
  stuck buttons, scroll-wheel skips or a dead direction, and lift-off jumps.
  The report is included in JSON/CSV exports. The engine lives in
  `analysis::diagnostics` and is covered by synthetic-fault unit tests
- Shared stroke segmentation (`analysis::strokes`) used by the acceleration,
  angle-snapping and diagnostics tests
- Headless egui smoke tests that render every GUI panel with synthetic input
- Polling panels show the nearest nominal rate (125/250/500/1000/... Hz)
- Windows raw input now reports side/extra buttons
- Comprehensive project documentation

### Fixed
- **Acceleration test (GUI)** reported acceleration on every mouse: it compared
  the size of individual reports, which grows with speed by definition. It now
  compares the counts produced by whole passes over the same distance at slow
  and fast speeds, and says so when the speeds were too similar to judge
- **Angle-snapping test (GUI)** flagged normal mice because integer sensor
  counts make most slow reports exactly axis-aligned. It now judges whole
  strokes: a slightly tilted freehand stroke that comes out perfectly straight
  and on-axis is the signature of snapping
- **Double-click test** used the widget's `clicked()`, which merges several
  presses in one frame into a single click and therefore could not see a
  bouncing switch. It now counts raw button presses (and egui pointer events
  as a fallback) and also flags a press within 25 ms of the previous release
- **Polling rate** was the number of reports in the last second, so the first
  fraction of a second of movement produced tiny readings that stuck in "Min".
  It is now derived from the median report interval (GUI and CLI)
- **Scroll wheel** counted every notch twice and totals were inflated 120x on
  kernels that emit `REL_WHEEL_HI_RES`; only the notch event is used now
- **CLI tests stalled** until the mouse moved because the evdev device was
  opened in blocking mode: the quit key and timed samples (jitter, click
  response) only worked while the mouse was producing events
- The raw-input channel grew without bound while the pointer was outside the
  window and no test was running (the GUI never repainted, so never drained
  it). The channel is now bounded and the app repaints periodically when idle
- Config was written to disk on every frame while a slider was dragged; it is
  now debounced and flushed on exit
- CSV/JSON export contained `f64::MAX` for the minimum hold time of a button
  that was never clicked
- CLI angle-snapping average angle was an arithmetic mean, which broke for
  movements straddling +/-180 degrees; it is now a circular mean
- Dashboard shows whether raw input is active and how to enable it on Linux
- Documented Rust requirement corrected to 1.92 (required by eframe 0.35)

### Changed
- Updated `eframe`/`egui` from 0.29 to 0.35 and `egui_plot` from 0.29 to 0.36,
  migrating to the new `eframe::App::ui` entry point, `egui::Panel` API,
  `CornerRadius` styling, and named plot-item constructors
- Updated `evdev` from 0.12 to 0.13, migrating to the `KeyCode`,
  `RelativeAxisCode`, and `EventSummary` event API
- Updated `crossterm` from 0.27 to 0.29 and `rfd` from 0.15 to 0.17
- Formatted the entire codebase with `rustfmt` so the CI formatting check passes

## [0.1.0] - 2025-01-01

### Added
- Initial release of Mouse TRAP
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
- **Scroll Wheel Test** - Scroll functionality validation (GUI only)
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
