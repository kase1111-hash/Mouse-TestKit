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
| Detects | Phantom clicks/scroll at rest, and movement at rest that recurs in 3+ separate seconds (a single bump is not a fault); stutters (time lost to reports later than 2.5x the poll period and at least 2 ms), unstable polling, a polling rate under 60 Hz (60-110 Hz is only noted: normal for Bluetooth, low for USB), Bluetooth batching (reports arriving two or more at a time on schedule, not after a gap; deliveries 5+ ms apart, at most 4 reports each on average (a delivery after missed connection events, which carries theirs too, is left out); noted, rate, stutters and stability judged per delivery, and no USB rate named), freezes (0.1-1 s gaps while the mouse kept moving, after which the held-back counts arrive in one report, which can point another way when circling; or, at 1 kHz and up, 0.1-1 s gaps with two full-speed reports on each side, which no hand produces; 1 = info, 2+ = warning), fast reports pinned at a report format limit of 127, 2047 or 32767 counts with none larger (warning: movement lost at high speed), timed only between same-direction reports with 2+ counts on an axis (fast enough to report every poll) that are not the small reports of a hand slowing to a stop, as in test 1; switch bounce (press within 25 ms of the previous release or 50 ms of the previous press; 1-2 = warning, 3+ = fault, as in test 10), presses under 3 ms, holds over 250 ms, buttons still down at the end; scroll notches reversed within 60 ms, not counting the notch that carries on the original way (1-2 = warning, 3+ = fault, as in the Scroll Wheel test); lift-off jumps as in test 6 (1 = info, 2+ that no hand could make = warning; 2+ that could also be the hand, or with the DPI unknown, = inconclusive; smooth hand movements are not judged) |
| Output | Verdict (Healthy / Needs attention / Fault detected / Inconclusive), findings with severity and explanation, raw measurements; included in JSON/CSV export. A button or scroll direction that registered nothing, too little fast movement, or an aborted run gives Inconclusive, never Healthy |
| Availability | GUI panel and CLI menu option 13 (device timestamps per report). Without raw input (macOS) the Movement and Lift steps are left out and bounce, short-press and scroll-skip checks are not judged |

### 1. Stutter Detection & Graphing

| Specification | Description |
|---------------|-------------|
| Function | Detects mouse movement stutter and irregularities |
| Method | Judges report intervals only during fast, steady movement (slow movement, reversals and pauses legitimately skip polls; the small reports of a hand slowing to a brief stop are left out); stutter = more than the chosen multiple (default 2.5x) of the median interval and at least 2.5 ms; a freeze = a stutter of 0.1-1 s between full-size reports after which the held-back counts (3+ median reports' worth) arrive in one report, or (at 1 kHz and up, where a hand's slow-down of 4+ ms spans 3+ polls) with two full-speed reports on each side and the movement lost; 2+ rate the session at least Fair. Batched (Bluetooth) delivery is judged per delivery; reports pinned at a report format limit are shown as clipping, not stutters |
| Output | Visual graph representation of stutter events |
| Use Case | Identifying inconsistent tracking or frame drops |

### 2. Polling Rate Monitor

| Specification | Description |
|---------------|-------------|
| Function | Displays real-time mouse polling rate |
| Measurement | Hz (polls per second), from the median interval of fast movement only (slow movement and reversals skip polls); deliveries per second when a Bluetooth mouse delivers reports in batches; GUI and CLI |
| Common Values | 125Hz, 250Hz, 500Hz, 1000Hz, 4000Hz, 8000Hz; under 110 Hz no USB rate is named (normal for Bluetooth), nor more than 4% above a standard rate (133 Hz: Bluetooth at 7.5 ms) |
| Use Case | Verifying advertised polling rates and consistency |

### 3. USB Conflict Detection

| Specification | Description |
|---------------|-------------|
| Function | Shows each mouse's USB bus, hubs, link speed and what shares the bus (device types from interface descriptors) |
| Purpose | Explain polling limits (low speed: about 125 Hz) and flag a mouse sharing a 12 Mbit/s hub with audio, video or storage devices; sharing a bus is reported as information |
| Use Case | Checking how a mouse is connected when other tests show low polling or stutter |

### 4. Click Response Test

| Specification | Description |
|---------------|-------------|
| Function | Measures click reaction time (human reaction plus display and input delay, not the mouse's own latency) |
| Measurement | Milliseconds (ms) |
| Use Case | Checking that clicks register and gauging overall reaction time |

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
| Detection | Motion that starts after the mouse has been still for 100 ms is summed over its first 100 ms; carrying the cursor more than 2 mm makes it jump-sized. Counts are converted with the mouse's DPI (800 assumed, and reported as assumed, when not given). Movement that continues more than 2 mm after that window is sliding and is not judged. A jump-sized movement is judged by its shape: from its report times and counts, the least acceleration it needed to start from rest, to come to rest and to turn back is worked out (allowing for counts a mouse holds back, a quarter report interval of timing error, and a waking sensor putting extra motion into its first report). Above 30 m/s^2 (about 3 g, more than a hand gives the mouse) at both ends, or to turn back, with 3+ steadily timed reports (a resting hand's stray counts just before or after it aside) = jump; straight, slow at both ends compared with its top speed, 5+ reports over 25+ ms and needing no more than 30 m/s^2 anywhere = the hand, not judged; anything else (the mouse landing while it slid, 1-2 reports, 125 Hz reports too coarse to show the shape, bunched Bluetooth or host timing, motion cut off by the end of the test) = possible jump |
| Verdict | 2+ jumps = problem; 2+ jumps and possible jumps with fewer than 2 jumps = possible problem (repeat without sliding the mouse); 1 of either = possible problem (repeat); with the DPI assumed, 2+ jumps = inconclusive (the GUI panel says "Possible jumps - DPI not set"; they may be ordinary set-down shifts at a higher DPI); "no jumps" needs 5 lifts reported with SPACE, otherwise inconclusive. The GUI panel stops with Enter or Esc; when Stop is clicked, motion that ended within 0.6 s of the click (the trip to the button) is not judged. The Auto Diagnostics lift step uses the same jump rules (possible jumps make it inconclusive), but cannot count lifts: one movement after a pause is enough for its "no jumps" note, which says when no lift or set-down motion was seen at all |
| Availability | CLI menu option 6 and GUI panel; needs raw input |
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
| Method | Fits a line to slightly tilted near-horizontal/vertical strokes; perfectly straight on-axis strokes indicate snapping |
| Detection | Identifies deviation from natural hand movement |
| Use Case | Verifying raw input for competitive gaming |

### 9. Acceleration Detection

| Specification | Description |
|---------------|-------------|
| Function | Tests for unwanted acceleration curves in the mouse's sensor/firmware |
| Method | Compares the median counts of slow and fast passes over the same distance; strokes that turn back on themselves are left out |
| Output | Fast/slow count factor and verdict (acceleration above 1.15x, lost counts below 0.85x), with the input measured |
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
| Function | Counts wheel notches and checks for encoder faults |
| Measurement | Notch count each way, speed (not rated), direction changes |
| Detection | Notches reversed within 60 ms of the previous one (skipping, raw input only), a direction that never registers; lost notches are checked by the user counting detents |
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
| Rust | 1.92 or later |
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
