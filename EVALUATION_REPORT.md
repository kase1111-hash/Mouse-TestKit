## PROJECT EVALUATION REPORT

**Primary Classification:** Underdeveloped
**Secondary Tags:** Good Concept, Execution Gap

---

### CONCEPT ASSESSMENT

**Problem solved:** Mouse hardware diagnostics — measuring polling rate, stutter, click latency, DPI accuracy, sensor jitter, and switch health. Today this requires juggling 3-4 separate single-purpose tools, most of which are Windows-only, closed-source, and abandoned.

**User:** Competitive gamers validating equipment, QA engineers testing peripherals, and users troubleshooting mouse issues. The pain is real — a mouse polling at 500Hz instead of advertised 1000Hz, or one with micro-stutters, causes measurable competitive disadvantage.

**Competition:** MouseTester (Windows/.NET, old, unmaintained), various web-based tools (limited to browser event resolution, ~120Hz cap), manufacturer-specific software (vendor lock-in). Nothing cross-platform, open-source, and comprehensive exists in this space.

**Value prop:** "All-in-one, cross-platform, native mouse diagnostics tool with both CLI and GUI."

**Verdict:** Sound — clear user need, fragmented competitive landscape, and a well-scoped value proposition. The cross-platform native approach in Rust is the right call for a tool that needs low-level input access and sub-millisecond timing accuracy.

---

### EXECUTION ASSESSMENT

**Architecture:** Clean module separation — `src/tests/` for CLI test logic, `src/gui/panels/` for GUI panels, `src/input.rs` / `src/input_windows.rs` for platform-specific input. Conditional compilation via `#[cfg(target_os = ...)]` is handled correctly. The egui/eframe choice for GUI is appropriate for a data-heavy dashboard application.

**Critical flaw — GUI measurement accuracy:**
The single biggest technical problem is that the GUI panels do not read raw input. Every GUI panel (`src/gui/panels/polling.rs:129`, `src/gui/panels/stutter.rs:270`, `src/gui/panels/click.rs:275`) uses:

```rust
let delta = ctx.input(|i| i.pointer.delta());
```

This reads egui's processed pointer data, which is limited to the GUI frame rate (typically 60Hz on a 60fps display). A mouse polling at 1000Hz will report ~60Hz in the GUI polling rate test. **The tool's primary measurement — polling rate — is fundamentally capped by screen refresh rate in the GUI.** The CLI version correctly reads raw evdev events (`src/tests/polling.rs:49`), but the GUI — which is the primary interface for most users — cannot produce accurate results for its core feature.

This means every timing-sensitive GUI test (polling rate, stutter detection, click latency) is measuring egui's rendering loop, not the mouse hardware. The numbers it produces will mislead users.

**Code duplication:**
Test logic is reimplemented between CLI and GUI rather than shared. Stutter analysis exists in `src/tests/stutter.rs:177-194` (CLI) and is rewritten in `src/gui/panels/stutter.rs:277-303` (GUI). The `MouseEvent` and `MouseButton` enums are defined identically in both `src/input.rs:166-188` and `src/input_windows.rs:35-54`. This duplication means bug fixes must be applied twice.

**Data structure mismatches:**
`src/gui/panels/click.rs:297` and `:326` use `Vec::remove(0)` — an O(n) operation — while other panels correctly use `VecDeque`. Inconsistent choice within the same codebase.

**Windows input loop pollution:**
`src/input_windows.rs:358` sends empty `RawMouseData` heartbeat messages every 100μs to detect channel closure. These zero-data messages flood the channel and must be filtered out downstream. A `try_send` check or a separate control channel would be cleaner.

**Continuous CPU burn:**
`src/gui/app.rs:721` calls `ctx.request_repaint()` unconditionally on every frame, even when no test is running and the user is idle on the dashboard. This keeps the GPU/CPU spinning at maximum frame rate for no reason.

**Light mode is broken:**
`src/gui/app.rs:681-686` sets `Visuals::light()` then immediately calls `theme::setup_custom_style()`, which overwrites everything with hardcoded dark-mode colors (`ThemeColors::bg_dark()` = `rgb(15, 17, 23)`, `ThemeColors::text_primary()` = `rgb(240, 242, 248)`). Light mode will look nearly identical to dark mode.

**Dead code:**
`src/gui/theme.rs` has 6 items marked `#[allow(dead_code)]`. `src/usb/conflicts.rs:217-232` defines `UsbBus` with `#[allow(dead_code)]`. `src/terminal.rs:82-101` has `TerminalGuard` marked dead despite being the correct RAII pattern that should replace the manual enable/disable calls in test modules.

**CSV export doesn't escape:**
`src/gui/export/mod.rs:206-315` interpolates values directly into CSV with `format!()`. Any value containing commas, quotes, or newlines will produce malformed output.

**What's done well:**
- Linux evdev input handling (`src/input.rs`) is solid — proper device enumeration, capability checking, permission error guidance
- Windows Raw Input API usage (`src/input_windows.rs`) demonstrates competent unsafe Rust with correct buffer management
- Configuration persistence (`src/gui/config.rs`) with platform-appropriate paths and graceful fallbacks
- The export system design with optional results per test is well-structured
- Unit tests for `PollingStats` and stutter analysis cover edge cases properly
- Error handling throughout is user-friendly with actionable messages

**Verdict:** Execution does not match ambition. The architecture is competent but the core measurement mechanism in the GUI — which is the primary interface — is fundamentally inaccurate. The CLI works correctly but is Linux/Windows-only and secondary. For a tool whose entire value proposition is measurement precision, this is a critical gap.

---

### SCOPE ANALYSIS

**Core Feature:** Mouse polling rate measurement and stutter detection

**Supporting:**
- Click response/latency testing
- DPI accuracy verification
- Sensor jitter analysis
- Acceleration and angle snapping detection
- Double-click switch testing

**Nice-to-Have:**
- JSON/CSV export
- Configuration persistence
- Dark/light theme toggle
- Scroll wheel testing

**Distractions:**
- USB conflict scanner (`src/usb/conflicts.rs`) — Linux-only, reads `/sys/bus/usb/devices`, tangential to mouse performance testing. Useful but belongs in a separate "USB diagnostics" scope.

**Wrong Product:**
- None — but "Button Durability Test" is advertised in `readme.md:26` and `SPEC_SHEET.md` with no implementation anywhere in the codebase. This is a documentation lie.

**Scope Verdict:** Focused — the feature set is cohesive and correctly prioritized. The test suite covers the diagnostics that matter to the target user. No feature creep.

---

### RECOMMENDATIONS

**CUT:**
- `#[allow(dead_code)]` items in `src/gui/theme.rs` — either use them or delete them
- Empty heartbeat messages in `src/input_windows.rs:358` — replace with proper channel health check
- "Button Durability Test" from README/SPEC_SHEET until it's actually implemented
- `UsbBus` struct in `src/usb/conflicts.rs:218-232` — unused, dead code

**DEFER:**
- macOS raw input support (IOKit/HID) — the current egui-based input is effectively a placeholder
- CSV export (until proper escaping is added, JSON-only is safer)
- Light mode theming (the current implementation is broken; dark-only is fine for v0.1)

**DOUBLE DOWN:**
- **Fix GUI input to use raw input APIs.** This is the single most important change. The GUI needs a background thread reading evdev (Linux) / Raw Input (Windows) and sending events to the UI via a channel, exactly like the CLI does. Without this, the GUI's polling rate and stutter measurements are useless. The architecture for this already exists in `src/input_windows.rs:290-363` — it just needs to be wired into the GUI panels instead of reading `ctx.input()`.
- **Share test logic between CLI and GUI.** Extract the analysis functions (stutter detection, polling rate calculation, DPI math) into a shared `analysis` module. The GUI panels should be thin UI wrappers over shared measurement logic.
- **Consolidate `MouseEvent`/`MouseButton` types.** Define them once in a shared module; platform-specific code converts into the shared types.

**FINAL VERDICT:** Refocus

The concept is strong, the architecture is reasonable, and the feature scope is disciplined. But the tool cannot ship as a credible diagnostics utility when its primary interface (GUI) produces inaccurate measurements for its primary feature (polling rate). Fix the GUI input pipeline, share the test logic, and this becomes a genuinely useful tool.

**Next Step:** Create a background input thread for the GUI that reads raw evdev/Raw Input events and feeds them to the panel structs via `mpsc::channel`, bypassing egui's pointer processing entirely. Start with the polling rate panel as the proof of concept.
