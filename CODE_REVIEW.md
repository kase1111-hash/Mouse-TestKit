# Mouse-TestKit Code Review

**Reviewer:** Claude (Opus 4.6)
**Date:** 2026-02-23
**Scope:** Full codebase review — all source files, tests, configuration, and CI

---

## Executive Summary

Mouse-TestKit is a well-structured cross-platform mouse diagnostics utility in Rust with 12 tests, a CLI interface (Linux/Windows), and an egui-based GUI (all platforms). The project demonstrates solid Rust fundamentals, good separation between platform-specific and shared code, and thorough user-facing error handling.

**Overall Verdict: Solid foundation with clear areas for improvement.**

The architecture is sound for a v0.1.0 release, but there are measurable issues in code duplication, a few incorrect algorithms, and architectural patterns that will scale poorly.

| Category         | Rating    | Notes                                                     |
|------------------|-----------|-----------------------------------------------------------|
| Correctness      | 7/10      | A few algorithmic bugs in measurement logic                |
| Security         | 9/10      | Minimal attack surface; safe Rust; one unsafe block review |
| Performance      | 6/10      | Unnecessary per-frame recalculations across GUI panels     |
| Maintainability  | 5/10      | High duplication in GUI panels; some god-objects           |
| Test Coverage    | 7/10      | Good unit tests for analysis; no GUI or integration tests  |
| Documentation    | 8/10      | Excellent doc-comments and user docs                       |
| Build/CI         | 8/10      | Multi-platform CI; missing Cargo.lock caching              |

---

## 1. Correctness Issues

### 1.1 [BUG] Polling rate Hz calculation is unreliable — `src/gui/panels/polling.rs`

The GUI polling panel counts events in a `VecDeque<Instant>` and uses the count directly as the Hz value, but does not properly verify the measurement window is exactly 1 second. Events are pruned to "within 1 second," but the actual window could be anywhere from a few milliseconds to a full second, making the Hz count inaccurate — especially during the first second of measurement or when the user briefly stops moving.

**Recommended fix:** Calculate Hz from inter-event intervals (as the CLI `tests/polling.rs` already does correctly), not from a simple event count.

### 1.2 [BUG] DPI accuracy formula is asymmetric — `src/gui/panels/dpi.rs`

```rust
let raw_accuracy = measured_dpi / target_dpi * 100.0;
// Then if > 100%: 200 - raw_accuracy
```

This produces a nonlinear scale: measuring 110% DPI yields 90% accuracy, but measuring 90% DPI also yields 90% accuracy. While this *works* for a pass/fail check, it hides whether the mouse is over- or under-shooting. A simple absolute deviation (`(measured - target).abs() / target * 100.0`) would be more transparent.

### 1.3 [BUG] Angle snapping average is naive for circular data — `src/tests/angle_snap.rs:172`

```rust
let avg_angle: f64 = angles.iter().sum::<f64>() / angles.len() as f64;
```

Averaging angles directly fails near the ±180 boundary. Movements at -179 and +179 yield an average of 0 instead of ±180. The correct approach is circular mean (`atan2(sum_sin, sum_cos)`). This invalidates variance and snapping detection for movements crossing the ±180 boundary.

### 1.4 [BUG] `Graph::push()` uses `Vec::remove(0)` — O(n) — `src/display/graph.rs:25`

```rust
if self.data.len() > self.width {
    self.data.remove(0);
}
```

`Vec::remove(0)` shifts all elements left — O(n) for every push once the buffer is full. This should use `VecDeque` for O(1) push/pop, or a ring buffer. With graph widths of 60, the cost is small, but it's an anti-pattern.

### 1.5 [MINOR] `TestSuite::count_completed()` — manual boolean counting — `src/tests/standard.rs:160-173`

This could be a simple array/slice of bools with `.iter().filter(|b| **b).count()` rather than 10 manual `if` statements.

---

## 2. Security Review

### 2.1 Unsafe code audit — `src/input_windows.rs` and `src/gui/input_bridge.rs`

Both files contain `unsafe` blocks for Windows Raw Input API interop. The unsafe code is well-contained and follows correct patterns:

- Buffer sizes are queried before allocation (`GetRawInputData` two-call pattern).
- Raw pointer casts are guarded by size checks.
- No user-controlled data reaches pointer arithmetic.

**One concern:** In `input_windows.rs:371`:
```rust
let raw = &*(buffer.as_ptr() as *const RAWINPUT);
```
This cast assumes the buffer alignment matches `RAWINPUT`. Since `Vec<u8>` guarantees 1-byte alignment, this could theoretically cause UB on platforms with strict alignment. In practice, Windows allocators align to at least 8 bytes and `RAWINPUT` fits within that, but using `std::alloc::Layout`-aware allocation would be safer.

### 2.2 No injection vectors

- No network code, no SQL, no HTML rendering.
- File I/O is limited to config reads (`~/.config/mouse-trap/config.json`) and user-initiated exports.
- Config deserialization uses `serde_json` — malformed JSON fails gracefully (falls back to defaults).
- USB scanner reads from `/sys/bus/usb/devices` — read-only filesystem paths, no user-controlled path injection.

### 2.3 CSV export escaping — `src/gui/export/mod.rs:202-208`

The `csv_escape()` function correctly handles RFC 4180 escaping (quoting fields with commas, double-quotes, and newlines). This prevents CSV injection in exported files.

---

## 3. Performance Issues

### 3.1 Per-frame statistic recalculation — multiple GUI panels

Several panels recalculate aggregate statistics (mean, min, max, std dev) from the entire dataset on every UI frame (~60 FPS):

- `stutter.rs`: Recalculates avg/min/max from all deltas every sample
- `click.rs`: Recalculates click statistics from entire vector each frame
- `scroll.rs`: Recalculates speed averages each frame

**Impact:** With 100-1000 data points, this is negligible. At 8000 Hz polling rates over minutes, vectors could reach 10k+ entries, making per-frame O(n) traversals noticeable.

**Recommended fix:** Use running statistics (incremental mean/variance) as `PollingStats` in `src/analysis/polling.rs` already does correctly. Extend this pattern to other panels.

### 3.2 `ctx.request_repaint()` is called conditionally — good

The app correctly only requests continuous repaints when a test is running (`app.rs:724-733`). This saves CPU when idle. Well done.

### 3.3 No `egui` frame throttling during idle dashboard

The dashboard and non-running panel views still repaint at the default egui rate when interacted with. This is fine for a desktop app.

---

## 4. Maintainability Concerns

### 4.1 [HIGH] GUI panels combine unrelated tests into god-objects

Two panels combine logically independent tests:

- **`click.rs` (895 lines):** Combines Click Response, Click Stickiness, and Lift-Off Jump — three unrelated tests sharing one struct with 40+ fields.
- **`accel.rs` (595 lines):** Combines Acceleration Detection and Angle Snapping — two different analyses in one struct.

This causes field naming confusion, state leaking between tests, and difficulty testing in isolation. Each should be its own struct/module.

### 4.2 [HIGH] Massive code duplication between raw-input and egui-fallback paths

Every GUI panel that processes mouse input has two nearly identical code paths: one for `InputBridge` raw events and one for egui pointer deltas. Across all panels, this represents ~400-500 lines of duplicated logic.

**Recommended fix:** Create a trait or adapter that normalizes both input sources into a common `MouseDelta { dx, dy, timestamp }` type. Panels would process a single unified event stream.

### 4.3 [MEDIUM] Settings-change detection mutates state in a getter

Multiple panels use this pattern:
```rust
pub fn settings_changed(&mut self) -> bool {
    if self.threshold != self.last_saved_threshold {
        self.last_saved_threshold = self.threshold;
        return true;
    }
    false
}
```

A method named `settings_changed` should be a pure query, not mutate state. This makes the function non-idempotent — calling it twice returns different results. Extract the mutation into a separate `acknowledge_settings_change()` method.

### 4.4 [LOW] Naming inconsistency: CLI vs GUI product name

- CLI: "Mouse-TestKit"
- GUI: "Mouse TRAP" (Test Response And Positioning)
- Cargo package: `mouse-testkit`
- Config directory: `mouse-trap`
- Repository: `Mouse-TestKit`

This isn't a bug but will confuse users and contributors. Pick one name.

---

## 5. Code Duplication Analysis

### 5.1 Platform input handling duplicated between CLI and GUI

Both `src/input.rs` and `src/gui/input_bridge.rs` independently implement:
- Device enumeration via evdev
- Mouse device selection heuristics (`REL_X` + `BTN_LEFT`)
- Event parsing (REL_X/Y, Key→Button mapping)

The GUI's `InputBridge` is the more capable version (timestamp merging, non-blocking). The CLI's `input.rs` could be refactored to share the device enumeration logic.

### 5.2 Windows Raw Input duplicated between CLI and GUI

`src/input_windows.rs` and the Windows path in `src/gui/input_bridge.rs` independently implement:
- Window class registration
- Raw input device registration
- WM_INPUT message processing
- Button flag parsing (identical magic constants: `0x0001`, `0x0002`, `0x0004`, etc.)

The button flag constants should at minimum be named constants rather than raw hex.

### 5.3 Type duplication: `MouseEvent` vs `RawInputEvent`

`src/types.rs` defines `MouseEvent` and `MouseButton` for the CLI. The GUI independently defines `RawInputEvent`, `RawInputKind`, and `RawButton` in `input_bridge.rs`. These are semantically identical types with different names.

---

## 6. Test Coverage Assessment

### 6.1 What's tested (good)

| Module | Tests | Quality |
|--------|-------|---------|
| `analysis::polling` | 9 tests | Thorough — edge cases, boundary values |
| `analysis::stutter` | 7 tests | Good — severity thresholds, empty input |
| `tests::liftoff` | 11 tests | Excellent — Pythagorean checks, threshold edges |
| `tests::angle_snap` | 11 tests | Good — snapping detection, negative movements |
| `tests::jitter` | 7 tests | Solid — magnitude, averages, edge cases |
| `gui::config` | 2 tests | Basic — serialization round-trip |

### 6.2 What's NOT tested (gaps)

- **No GUI panel tests:** None of the 8 GUI panels have unit tests. State machines (start/stop/reset), statistics calculations, and export functions are untested.
- **No integration tests:** No end-to-end test feeding synthetic mouse events through the pipeline.
- **No Windows-specific tests:** All `input_windows.rs` code is untested.
- **CLI test modules:** `click_response.rs`, `click_sticky.rs`, `double_click.rs`, `acceleration.rs`, `dpi.rs`, `standard.rs` — none have unit tests (they're interactive-only).
- **Export module:** `to_csv()` and `to_json()` have no tests verifying format correctness.

### 6.3 Recommendation

Add pure-logic unit tests for:
1. Statistics calculations in each GUI panel (extract into testable functions)
2. CSV output format validation
3. The `analyze_line()` function in angle snapping (already has tests, but add circular boundary cases)

---

## 7. Architecture & Design

### 7.1 What works well

- **Shared analysis library:** `src/analysis/` contains pure functions (`analyze_stutter`, `PollingStats`) used by both CLI and GUI. This is clean separation.
- **Platform abstraction:** `#[cfg(target_os)]` gates are used correctly and consistently throughout.
- **RAII terminal guard:** `TerminalGuard` in `terminal.rs` ensures raw mode cleanup even on panic — excellent pattern.
- **Config persistence:** XDG-compliant paths, graceful fallback, serde round-tripping.
- **CI pipeline:** Multi-platform builds, clippy, fmt checks, artifact upload.

### 7.2 What could improve

- **Missing `lib.rs` re-exports:** `lib.rs` only exports `types` and `analysis`. The CLI's `main.rs` has to import them directly via `use mouse_testkit::analysis::*`. This is fine now but will get awkward as the library grows.
- **No error types:** The project uses `String` for errors (`Config::save()` returns `Result<(), String>`). A proper error enum would be more idiomatic.
- **`#[allow(dead_code)]` overuse:** Multiple structs/fields suppress dead-code warnings. This often indicates code that was written speculatively or exported types that aren't actually used. Audit and remove genuinely dead code.

---

## 8. Build & CI

### 8.1 Cargo.toml observations

- **Missing Cargo.lock caching** in CI — each build re-resolves and downloads all dependencies. Add `actions/cache@v4` for `~/.cargo` and `target/`.
- **`overflow-checks = false`** in the keychain profile — this disables integer overflow detection. For a tool that processes hardware counters, overflow is a real concern at high polling rates. Consider keeping overflow checks enabled even in release.
- **GUI deps pulled into CLI build:** Both binaries share the same `[dependencies]`, so building the CLI pulls in `eframe`, `egui_plot`, `rfd`, etc. Use feature flags or workspace members to avoid this.

### 8.2 CI gaps

- **No `cargo audit`** step — should check for known vulnerabilities in dependencies.
- **No release automation** — CI builds artifacts but has no release/tagging workflow.
- **macOS builds don't run tests** — tests only run on Linux. At minimum, `cargo test --lib` should run on all platforms.

---

## 9. Dead Code & Cleanup

| File | Item | Issue |
|------|------|-------|
| `src/display/mod.rs` | `#![allow(dead_code)]` | Module-level suppression hides real dead code |
| `src/display/graph.rs` | `#![allow(dead_code)]` | `Graph` struct is unused in the GUI |
| `src/tests/click_sticky.rs:142` | `fn is_sticky()` | Public but never called externally |
| `src/tests/liftoff.rs:148` | `fn is_jump()` | Public but only used in tests |
| `src/tests/jitter.rs:203` | `fn analyze_jitter_pub()` | Wrapper exists only for test visibility — use `#[cfg(test)]` pub instead |
| `src/tests/angle_snap.rs:206` | `fn analyze_line_pub()` | Same pattern as above |
| `src/tests/acceleration.rs:39` | `let _total_time` | Underscore-prefixed, never read |
| `src/tests/dpi.rs:56` | `let mut _sample_start` | Underscore-prefixed, only written to |
| `src/gui/panels/scroll.rs:27` | `missed_events` field | Declared, reset, but never incremented or read |

---

## 10. Positive Highlights

These deserve recognition as good Rust practices:

1. **`TerminalGuard` RAII pattern** (`terminal.rs:83-100`): Guarantees terminal state restoration. This prevents the dreaded "terminal stuck in raw mode" issue.

2. **Evdev timestamp deduplication** (`input_bridge.rs:173-199`): Merging consecutive REL_X/REL_Y events with the same kernel timestamp into a single `Move` event is crucial for accurate polling rate measurement. This shows deep understanding of the hardware.

3. **Conditional repaint requests** (`app.rs:724-733`): Only requesting continuous repaints when a test is running saves significant CPU. Many egui apps miss this optimization.

4. **Config fallback chain** (`config.rs:82-122`): Platform-appropriate config directories (`XDG_CONFIG_HOME`, `%APPDATA%`, `~/Library/Application Support`) with fallback to current directory.

5. **Unit test quality** in analysis modules: Tests cover empty input, single element, boundary values, and large data — these are well-written.

6. **Blocking `GetMessageW` loop** (`input_bridge.rs:388-401`): Using a blocking message loop instead of polling avoids CPU waste. The comment explicitly notes "no heartbeat hack needed."

---

## Summary of Actionable Items

### Must Fix (Correctness)
- [ ] Fix circular mean calculation in angle snapping detection
- [ ] Fix GUI polling rate Hz calculation to use proper time windows

### Should Fix (Architecture)
- [ ] Split `click.rs` into three separate panel modules
- [ ] Split `accel.rs` into two separate panel modules
- [ ] Unify `MouseEvent`/`RawInputEvent` type duplication
- [ ] Create common input adapter to eliminate raw-vs-egui code duplication

### Nice to Have (Quality)
- [ ] Add dependency caching to CI
- [ ] Add `cargo audit` to CI
- [ ] Add unit tests for GUI panel statistics and export functions
- [ ] Replace `String` error returns with proper error types
- [ ] Remove genuinely dead code and `#[allow(dead_code)]` suppressions
- [ ] Add named constants for Windows Raw Input button flags
- [ ] Unify product naming (Mouse-TestKit vs Mouse TRAP)
