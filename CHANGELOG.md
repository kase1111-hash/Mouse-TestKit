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
- **GUI device selection**: the raw-input bridge reads every mouse-like device,
  locks onto the one you actually move (shown by name on the Dashboard, with a
  picker to choose another) and picks a mouse up again after it is unplugged
  and replugged; Windows filters raw input by device the same way
- Shared, unit-tested analysis modules for lift-off jumps (`analysis::liftoff`,
  used by the CLI test, the GUI Lift-Off test and Auto Diagnostics) and scroll
  wheel faults (`analysis::scroll`)
- `input::fetch_timed_events`: CLI tests get one event per hardware report with
  its device timestamp (kernel time on Linux, raw-input thread time on Windows)

### Fixed
- **Timing used read time, not report time**: CLI tests and the GUI bridge
  stamped events when the queue was drained, so a batch of reports looked
  simultaneous and 2-8 kHz mice read as millions of Hz (CLI polling) or as
  ~476 Hz (CLI Auto Diagnostics). Kernel/raw-input timestamps are used now
- **Healthy mice flagged for stutter**: mice only report when counts build up,
  so slow movement, reversals and brief stops leave normal 2-20 ms gaps.
  Polling and stutter (GUI, CLI, Auto Diagnostics) are now judged only between
  reports fast enough to prove the mouse reported every poll, against the
  median interval with a 2.5 ms floor; with too little fast movement the result
  is "move faster" / inconclusive. The CLI stutter totals also re-counted the
  same window several times a second
- **Auto Diagnostics verdicts**: a stuck button, a dead right button or a dead
  wheel could still end "Healthy"; untested or skipped parts now make the
  verdict Inconclusive and say what was not tested. One bump, or a hand that
  lets go late, during the rest step no longer reads as sensor drift (movement
  must recur in 3 separate seconds after a 2 s settle). Bounce severity depends
  on the count (1 = warning, 2+ = fault) rather than on how many clicks were
  made. Without raw input the timing-based checks are skipped instead of being
  judged on frame times, and wheel deltas are summed into whole notches. Abort
  no longer sits under the click target, and export is disabled mid-run
- **Lift-off** (CLI, GUI and Auto Diagnostics) checked one axis of one report:
  real jumps spread over several reports were missed and ordinary starts at
  high DPI were flagged. Motion is now summed over 100 ms after a pause and
  compared with a physical distance (2 mm) at the mouse's DPI; tests need real
  lifts before saying "no jumps", and an assumed DPI is stated
- **Sticky clicks**: a single normal 100-150 ms click was called sticky (CLI
  and GUI). The Auto Diagnostics rule (250 ms, at least 2 long holds) is used
  everywhere, a button still held at the end is reported, and clicks on the
  panel's own Stop button are no longer recorded
- **CLI double-click test** missed a bounce on release (press within 25 ms of
  the previous release)
- **CLI angle-snapping test** asked for diagonal lines (which snapping never
  changes) and measured X and Y events separately, so it could never detect
  snapping; it now uses the GUI's stroke analysis with near-axis lines
- **DPI test**: the GUI summed path length (overshoot and wobble read high) and
  the CLI counted only X and started counting before the mouse was on the
  start mark. Both now use net displacement, distance in inches or cm, and a
  wider DPI range; the CLI uses the median of samples
- **Jitter (GUI)** recorded the click on "Take Sample" and the hand letting go;
  it now waits 2 s and discards samples where a button was touched
- **Scroll wheel (GUI)** graded the user's scrolling rhythm ("Poor") and
  claimed missed-step detection it did not have. It now detects reversed
  notches and a direction that never registers; high-resolution wheel deltas
  add up on Windows instead of being dropped
- **Acceleration tests** claimed to verify OS acceleration and "1:1" input,
  although raw counts are read before OS acceleration; they now cover
  sensor/firmware acceleration only. The CLI no longer counts the return trip
  as part of a pass, and lost counts at speed are not called acceleration
- **USB conflict scan** read only device-level classes, so mice were "Other"
  and it could never report anything. It now reads interface classes, link
  speed and hubs, and treats a shared bus as information, not a conflict
- **CLI click reaction test** recorded clicks made before the cue as ~0 ms; it
  now rejects false starts, randomises the delay and says it measures human
  reaction time, not mouse latency (GUI text no longer claims latency either)
- **CLI standard runner** reported every test as successful even when it was
  quit or no mouse was found
- **Without raw input** (macOS, Linux outside the `input` group) the polling,
  stutter and DPI panels showed the display frame rate and OS-accelerated
  pointer movement as mouse measurements; they now explain that raw input is
  required
- Windows CLI merged every mouse and touchpad into the selected device and
  ignored side buttons; it now filters by device and ignores absolute reports
- Loaded GUI config values are clamped to the ranges the UI allows
- CI: the retired `macos-13` runner left the Intel macOS job queued until it
  was cancelled (it is now cross-compiled on `macos-latest`), and new clippy
  lints are fixed
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
- A measured polling rate of 60-110 Hz is noted as normal for Bluetooth
  instead of flagged; only rates under 60 Hz are a warning
- Exports keep their existing JSON keys and add fields that say what the
  numbers are (units, long-hold threshold and verdicts, scroll skips, lift-off
  distances in mm); CSV labels say counts instead of px
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
