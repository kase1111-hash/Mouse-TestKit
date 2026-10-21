# Mouse TRAP Specification Sheet

## Overview

Mouse TRAP is a comprehensive mouse testing utility designed to analyze and diagnose mouse performance, connectivity, and reliability.

---

## Features & Specifications

### 0. Auto Diagnostics

| Specification | Description |
|---------------|-------------|
| Function | Guided one-minute check-up that looks for faults automatically |
| Steps | Rest (2 s settle + 5 s), Movement (up to 12 s), Clicks (up to 20 s), Scroll (up to 12 s), Lift (10 s); each ends on its own once enough data is collected or its time is up |
| Detects | Phantom clicks/scroll at rest, and movement at rest that recurs in 3+ separate seconds (a single bump is not a fault); stutters (time lost to reports later than 2.5x the poll period and at least 2 ms), unstable or very low polling rate, dropouts, timed only between reports with 2+ counts on an axis (fast enough to report every poll); switch bounce (press within 25 ms of the previous release or 50 ms of the previous press; 1 = warning, 2+ = fault), presses under 3 ms, holds over 250 ms, buttons still down at the end; scroll notches reversed within 60 ms (1-2 = warning, 3+ = fault); cursor jumps over 50 counts after an 80 ms pause |
| Output | Verdict (Healthy / Needs attention / Fault detected / Inconclusive), findings with severity and explanation, raw measurements; included in JSON/CSV export. A button or scroll direction that registered nothing, too little fast movement, or an aborted run gives Inconclusive, never Healthy |
| Availability | GUI panel and CLI menu option 13 (device timestamps per report). Without raw input (macOS) the Movement and Lift steps are left out and bounce, short-press and scroll-skip checks are not judged |

### 1. Stutter Detection & Graphing

| Specification | Description |
|---------------|-------------|
| Function | Detects mouse movement stutter and irregularities |
| Method | Judges report intervals only during fast, steady movement (slow movement, reversals and pauses legitimately skip polls); stutter = more than the chosen multiple of the median interval and at least 2.5 ms |
| Output | Visual graph representation of stutter events |
| Use Case | Identifying inconsistent tracking or frame drops |

### 2. Polling Rate Monitor

| Specification | Description |
|---------------|-------------|
| Function | Displays real-time mouse polling rate |
| Measurement | Hz (polls per second) |
| Common Values | 125Hz, 250Hz, 500Hz, 1000Hz, 4000Hz, 8000Hz |
| Use Case | Verifying advertised polling rates and consistency |

### 3. USB Conflict Detection

| Specification | Description |
|---------------|-------------|
| Function | Shows other devices connected to the same USB controller/hub |
| Purpose | Identify potential bandwidth conflicts |
| Use Case | Diagnosing performance issues caused by USB bus contention |

### 4. Click Response Test

| Specification | Description |
|---------------|-------------|
| Function | Measures click latency and response time |
| Measurement | Milliseconds (ms) |
| Use Case | Evaluating switch responsiveness and debounce timing |

### 5. Click Stickiness Test

| Specification | Description |
|---------------|-------------|
| Function | Tests for stuck or delayed click release |
| Detection | Identifies switches that fail to release properly |
| Use Case | Diagnosing worn or faulty mouse switches |

### 6. Lift-Off Distance (LOD) Jump Test

| Specification | Description |
|---------------|-------------|
| Function | Tests for cursor jump during mouse lift |
| Detection | Identifies unwanted cursor movement when lifting mouse |
| Use Case | Evaluating sensor lift-off behavior and calibration |

### 7. DPI Accuracy Test

| Specification | Description |
|---------------|-------------|
| Function | Verifies mouse DPI matches advertised/configured value |
| Method | Measures actual counts per physical distance moved |
| Output | Actual DPI vs expected, accuracy percentage |
| Use Case | Validating sensor accuracy and DPI calibration |

### 8. Angle Snapping Detection

| Specification | Description |
|---------------|-------------|
| Function | Detects if mouse has angle snapping/prediction enabled |
| Method | Analyzes diagonal movement for artificial straightening |
| Detection | Identifies deviation from natural hand movement |
| Use Case | Verifying raw input for competitive gaming |

### 9. Acceleration Detection

| Specification | Description |
|---------------|-------------|
| Function | Tests for unwanted acceleration curves in the mouse's sensor/firmware |
| Method | Compares movement distance at different speeds |
| Output | Acceleration coefficient and consistency |
| Scope | Raw counts are read before OS pointer acceleration, which is not covered |
| Use Case | Ensuring the mouse itself reports 1:1 for precision tasks |

### 10. Double-Click Test

| Specification | Description |
|---------------|-------------|
| Function | Detects faulty switches causing unintended double-clicks |
| Method | Monitors for rapid unintended click events |
| Threshold | Configurable debounce detection window |
| Use Case | Diagnosing switch degradation or defects |

### 11. Jitter Test

| Specification | Description |
|---------------|-------------|
| Function | Measures micro-movements when mouse is stationary |
| Method | Records position variance over time at rest |
| Output | Jitter magnitude in raw counts, recorded after a 2 s settle period so the click that started it is not counted |
| Use Case | Evaluating sensor stability and noise floor |

### 12. Scroll Wheel Test

| Specification | Description |
|---------------|-------------|
| Function | Tests scroll wheel functionality and consistency |
| Measurement | Step count, speed, direction changes |
| Detection | Identifies missed scroll steps and inconsistent behavior |
| Availability | GUI only |
| Use Case | Validating scroll wheel reliability and encoder quality |

### 13. Run All Standard Tests

| Specification | Description |
|---------------|-------------|
| Function | Executes complete test suite sequentially |
| Coverage | All tests with comprehensive final report |
| Use Case | Full mouse performance validation |

---

## System Requirements

| Component | Requirement |
|-----------|-------------|
| Platform | Linux x64, Windows x64, macOS ARM64/x64 |
| Rust | 1.70 or later |
| USB | USB 2.0 or higher |
| Input | HID-compliant mouse device |

---

## Target Use Cases

- Gaming mouse performance validation
- Quality assurance testing for mouse manufacturers
- Troubleshooting mouse performance issues
- Comparing mouse specifications and real-world performance
- Identifying hardware defects or wear

---

## Version

**Version:** 0.1.0
