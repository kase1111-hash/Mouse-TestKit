# Vibe-Code Detection Audit v2.0 — Mouse-TestKit

**Audit Date:** 2026-02-24
**Auditor:** Claude Code (Opus 4.6)
**Repository:** kase1111-hash/Mouse-TestKit
**Commit Range:** Full history (81 commits across main + feature branches)

---

## Executive Summary

Mouse-TestKit is a cross-platform mouse diagnostics utility written in Rust with dual CLI/GUI binaries. The code is **technically competent and functionally complete**, with correct concurrency patterns, deep platform API usage, and a polished GUI. However, surface provenance signals overwhelmingly indicate AI generation: **65% of commits are authored by `Claude <noreply@anthropic.com>`**, human contributions are limited to PR merges, and the codebase exhibits unnaturally uniform documentation and naming conventions.

**Final Vibe-Code Confidence: 15.2% — Borderline Human-Authored / AI-Assisted**

The code works. The architecture is sound. But a human didn't write most of it.

---

## Domain A: Surface Provenance (Weight: 20%)

### A1: Commit History & Iteration Patterns — Score: 1/3 (Weak)

| Author | Commits | Role |
|--------|---------|------|
| Claude (noreply@anthropic.com) | 53 | All implementation work |
| Kase / Kase Branham | 28 | PR merges only |

**Evidence:**
- Every implementation commit is authored by Claude. All branch names follow the pattern `claude/<task>-<id>` (e.g., `claude/code-review-assessment-nyxLP`, `claude/repo-review-evaluation-O1dok`).
- Human commits are exclusively `Merge pull request #N from kase1111-hash/claude/...` — no direct code contributions, no WIP commits, no fixup/squash from a human developer.
- Commit messages are uniformly well-structured: "Add comprehensive code documentation", "Phase 2: consolidate shared types and analysis logic", "Apply clippy suggestions for improved code quality". No human-style terse messages like "fix bug", "wip", or "oops".
- The development pattern is: Claude writes entire feature → PR created → human merges. No iterative back-and-forth visible in the commit graph.

**Remediation:** This is an observation, not a defect. If provenance matters for the project's context, the human maintainer should make direct commits for features they personally architect.

---

### A2: Comment & Documentation Archaeology — Score: 1/3 (Weak)

**Evidence:**
- Every source file opens with a `//!` module-level doc comment following an identical template pattern:
  ```rust
  //! <Module name>
  //!
  //! <One-sentence description>. <Architecture notes>.
  ```
  Files: `src/input.rs:1-12`, `src/terminal.rs:1-4`, `src/gui/app.rs:1-4`, `src/gui/config.rs:1-3`, `src/gui/theme.rs:1-4`, `src/gui/export/mod.rs:1-5`, `src/gui/input_bridge.rs:1-9`, `src/gui/panels/mod.rs:1-15`, etc.

- Zero instances of `TODO`, `HACK`, `FIXME`, `XXX`, or `NOTE` across the entire codebase. No debugging artifacts. No commented-out code. No "I don't understand why this works" notes. This is atypical for active human development.

- Inline comments are tutorial-style explanations rather than rationale:
  ```rust
  // Only check event devices          (src/input.rs:44)
  // Check if device has mouse-like capabilities  (src/input.rs:50)
  // Warn user if we encountered permission issues (src/input.rs:83)
  ```
  These describe *what* the code does (which is visible from the code itself), not *why* a particular approach was chosen.

- One authentic human signal found: `src/gui/app.rs:656` — a hidden message `"MERRY CHRISTMAS // TO: XANDER FROM: DAD"` embedded in the About dialog. This is a genuine personal touch that an AI would not independently produce.

**Remediation:** None needed — the code is well-documented. This criterion measures authorship signal, not documentation quality.

---

### A3: Test Quality & Coverage — Score: 2/3 (Moderate)

**Evidence — Tests that exist:**
- `src/analysis/polling.rs:39-149` — 9 unit tests covering `PollingStats`: initialization, single/multiple updates, zero handling, min/max tracking, running average, large values, consistent samples. These are **genuine and correct**.
- `src/analysis/stutter.rs:45-135` — 8 unit tests covering `analyze_stutter()`: empty input, single element, uniform deltas, minor/moderate/severe deviation classification, event field verification, threshold boundaries. **Well-designed with explicit deviation calculations in comments.**
- `src/gui/config.rs:125-158` — 2 tests for config default values and JSON round-trip serialization. **Functional.**

**Evidence — Tests that are missing:**
- No tests for any GUI panel logic (polling, stutter, click, jitter, DPI, accel, double-click, scroll)
- No tests for `input.rs` device enumeration or event parsing
- No tests for `input_bridge.rs` event coalescing logic (the REL_X/REL_Y merge in `linux_event_loop`)
- No tests for `export/mod.rs` CSV generation or `csv_escape()`
- No tests for `usb/conflicts.rs` device parsing
- No integration tests (`tests/` directory at crate root)
- No property-based or fuzz testing

Total: **19 unit tests** across 3 files. The tests that exist are well-written, but coverage is narrow — concentrated in the shared analysis layer with zero coverage of the GUI panels, input handling, or export logic.

**Remediation:**
1. Add unit tests for `csv_escape()` — it handles commas and quotes but should be validated against RFC 4180 edge cases (embedded newlines, empty strings, values starting with `=`/`+`/`-`/`@` for CSV injection).
2. Add tests for `DoubleClickPanel::calculate_consistency()` and `DoubleClickPanel::register_click()` — these contain non-trivial statistical logic.
3. Add tests for the Linux evdev event coalescing in `InputBridge::linux_event_loop()` — the REL_X/REL_Y merge by kernel timestamp is critical correctness logic.

---

### A4: Import & Dependency Hygiene — Score: 3/3 (Strong)

**Evidence:**
- `Cargo.toml` declares 10 dependencies, all purposeful:
  - `eframe`/`egui`/`egui_plot` — GUI framework (core requirement)
  - `evdev` — Linux input (core requirement, cfg-gated)
  - `winapi` — Windows Raw Input API (core requirement, cfg-gated)
  - `serde`/`serde_json` — Config persistence and export
  - `chrono` — Export timestamps
  - `crossterm` — Terminal mode for CLI
  - `rfd` — Native file dialogs for export
  - `raw-window-handle` — Cross-platform window handle bridging

- No unused dependencies detected. No kitchen-sink utility crates (no `itertools`, `anyhow`, `thiserror`, `lazy_static` without justification).
- Platform-gating is correct: `evdev` is `[target.'cfg(target_os = "linux")'.dependencies]`, `winapi` is `[target.'cfg(target_os = "windows")'.dependencies]`.

---

### A5: Naming Consistency — Score: 1/3 (Weak)

**Evidence:**
- Naming is **mechanically uniform** across the entire codebase:
  - All panels: `{Name}Panel` with identical method signatures `fn ui(&mut self, ui, ctx, raw_events, has_bridge)`
  - All export structs: `{Name}Export`
  - All state fields: `is_running`, `last_move_time`, `total_*_count`
  - All reset methods follow identical patterns

- This level of uniformity is atypical for human development, where naming conventions drift over time as different features are added. Here, code written in PR #14 uses identical conventions to code written in PR #27.

- Project identity inconsistency: The CLI binary is `mouse-testkit` but the GUI calls itself "Mouse TRAP" (Test Response And Positioning). The config directory is `mouse-trap`. The README says "Mouse-TestKit". This suggests the rebrand happened mid-development without full reconciliation.

---

### A6: Documentation Alignment — Score: 2/3 (Moderate)

**Evidence:**
- Documentation is comprehensive: `readme.md`, `SPEC_SHEET.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`, `docs/BUILD.md`, `docs/USER_MANUAL.md`, plus the self-referential `AUDIT_REPORT.md`, `EVALUATION_REPORT.md`, `REFOCUS_PLAN.md`, `claude.md`.
- PR #27 explicitly updated documentation to match the actual codebase state, indicating awareness of doc drift.
- Module-level `//!` comments accurately describe each module's purpose and scope.
- Minor misalignment: `dark_mode` is stored in config but light mode is hardcoded as always-dark (`src/gui/app.rs:686`: `ctx.set_visuals(egui::Visuals::dark())` with comment "Always dark — light mode theme not yet implemented"). The config field exists but the toggle is dead.

---

### A7: Dependency Utilization Depth — Score: 3/3 (Strong)

**Evidence:**
- **egui/eframe**: Deep — custom style system (`theme.rs`), procedural icon rendering, custom frames (glass/card), `egui_plot` with multiple line types, `Sense::hover()` for test areas, `ScrollArea`, conditional repainting, `SidePanel`/`CentralPanel` layout.
- **evdev**: Deep — device enumeration by capability (`supported_relative_axes`, `supported_keys`), event kind matching, kernel timestamp extraction, device grabbing.
- **winapi**: Deep — Hidden window creation, `RegisterRawInputDevices` with `RIDEV_INPUTSINK`, `GetRawInputData` buffer management, `RAWINPUT` struct field access, button flag bitmask parsing. This is non-trivial Win32 API usage.
- **serde**: Standard but complete — `Serialize`/`Deserialize` derives, `serde_json::to_string_pretty`, `from_str`, `env!("CARGO_PKG_VERSION")` in export metadata.

---

### Domain A Summary

| Criterion | Score | Max |
|-----------|-------|-----|
| A1: Commit History | 1 | 3 |
| A2: Comment Archaeology | 1 | 3 |
| A3: Test Quality | 2 | 3 |
| A4: Import Hygiene | 3 | 3 |
| A5: Naming Consistency | 1 | 3 |
| A6: Documentation Alignment | 2 | 3 |
| A7: Dependency Depth | 3 | 3 |
| **Domain A Total** | **13** | **21 (61.9%)** |

---

## Domain B: Behavioral Integrity (Weight: 50%)

### B1: Error Handling Specificity — Score: 2/3 (Moderate)

**Evidence — Strong error handling:**
- `src/input.rs:68-92` — Permission denied errors are counted, and when all devices fail, a detailed remediation box is displayed with three possible causes and fixes.
- `src/input.rs:102-118` — "No Mouse Devices Found" shows a structured box with numbered remediation steps.
- `src/gui/config.rs:49-51` — Config parse failure falls back to defaults with warning.
- `src/terminal.rs:14-51` — All terminal operations return bool with descriptive error messages on failure.
- `src/terminal.rs:82-100` — RAII `TerminalGuard` ensures terminal state cleanup even on panic.

**Evidence — Gaps:**
- `src/gui/input_bridge.rs:184-195, 212-219, 243-250, 260-269` — Channel `send()` failures are handled by returning (which is correct for thread shutdown), but `let _ = sender.send(...)` in the Windows path (`src/gui/input_bridge.rs:450-507`) silently discards errors without even a return.
- `src/usb/conflicts.rs:17-18` — Uses `.ok()` to discard read_line errors rather than handling them.
- No `Result` types in GUI panel methods — everything is infallible by design, which works but means errors are absorbed rather than surfaced.

**Remediation:** The `let _ = sender.send(...)` pattern in the Windows input bridge should at minimum return on error (matching the Linux path), to avoid sending events on a dead channel.

---

### B2: Configuration Consumption — Score: 3/3 (Strong)

**Evidence — All config fields are consumed:**

| Config Field | Set In | Consumed By |
|---|---|---|
| `dark_mode` | `Config::default()` | `MouseTestKitApp::new()` → `self.dark_mode` (loaded) |
| `stutter_threshold_multiplier` | `Config::default()` (2.0) | `StutterPanel::set_threshold_multiplier()` → used in `record_delta()` |
| `dpi_target` | `Config::default()` (800) | `DpiPanel::set_target_dpi()` → used in DPI calculation |
| `dpi_distance_inches` | `Config::default()` (2.0) | `DpiPanel::set_target_distance()` → used in DPI calculation |
| `double_click_threshold_ms` | `Config::default()` (50.0) | `DoubleClickPanel::set_threshold_ms()` → used in `register_click()` |

- Config save/load cycle is complete: `Config::load()` → panel setters → `settings_changed()` → `config_dirty` flag → `save_config_if_needed()` → `update_config()` → `Config::save()`.
- Platform-aware config paths: XDG on Linux, `%APPDATA%` on Windows, `~/Library/Application Support` on macOS, with fallback to `./mouse-trap-config`.

---

### B3: Call Chain Completeness — Score: 3/3 (Strong)

**Evidence — Traced call chains:**

1. **GUI Input Pipeline:**
   `eframe::run_native()` → `MouseTestKitApp::update()` (`app.rs:665`) → `input_bridge.poll()` → `frame_events` → passed to `panel.ui(ui, ctx, raw_events, has_bridge)` → `process_raw_events()` / `process_egui_fallback()` → per-panel analysis → UI rendering. **Complete.**

2. **CLI Test Pipeline:**
   `main()` → menu selection → `tests::polling::run()` (etc.) → `input::select_mouse()` → `Device::open()` → event loop → `parse_event()` → analysis → terminal display. **Complete.**

3. **Export Pipeline:**
   `export_json()` / `export_csv()` → `collect_results()` → each `panel.export()` returns `Option<*Export>` → `TestResultsExport::to_json()` / `to_csv()` → `rfd::FileDialog::save_file()` → `fs::write()`. **Complete.**

4. **Config Persistence:**
   `Config::load()` → `MouseTestKitApp::new()` → panel setters → `settings_changed()` per frame → `config_dirty = true` → `save_config_if_needed()` → `update_config()` → `Config::save()`. **Complete.**

**One dead path identified:** `dark_mode` is loaded from config and stored in `self.dark_mode`, but `app.rs:686` unconditionally sets `ctx.set_visuals(egui::Visuals::dark())`. The light mode toggle is incomplete. This is acknowledged in a code comment.

---

### B4: Async/Concurrency Correctness — Score: 3/3 (Strong)

**Evidence:**
- **Linux InputBridge** (`input_bridge.rs:86-159`): Named thread (`input-bridge-evdev`) spawned via `thread::Builder`. Uses `mpsc::channel` for event delivery. Thread exits cleanly when `sender.send()` returns `Err` (receiver dropped). Device is NOT grabbed — correctly avoids stealing pointer input from the GUI.

- **Windows InputBridge** (`input_bridge.rs:297-508`): Named thread (`input-bridge-rawinput`). Uses blocking `GetMessageW` (correct — no CPU-spinning). `RIDEV_INPUTSINK` flag allows receiving input even when window is not focused. Hidden window class registered with `DefWindowProcW`.

- **Event Coalescing** (`input_bridge.rs:166-285`): Linux evdev emits separate REL_X and REL_Y events for a single hardware poll. The code correctly merges them by kernel timestamp — pending dx/dy are accumulated and flushed when the timestamp changes. This prevents double-counting in polling rate measurement.

- **No shared mutable state** between threads — all communication is via mpsc channels. No `Arc<Mutex<_>>`, no lock contention.

---

### B5: State Management Consistency — Score: 2/3 (Moderate)

**Evidence — Well-managed state:**
- Each panel has explicit `start()` / `reset()` methods that clear all accumulated state.
- Settings dirty-checking uses comparison against `last_saved_*` values (`stutter_panel.rs:62-68`, `double_click.rs:55-61`).
- Conditional repaint: `app.rs:724-733` only calls `ctx.request_repaint()` when a test is actively running, avoiding unnecessary CPU usage when idle.

**Evidence — Unused state:**
- `StutterPanel::window_stutter_count` (`stutter.rs:23`) is computed in `record_delta()` at line 356 but **never read or displayed**. Dead state.
- `ScrollPanel::missed_events` (`scroll.rs:27`) is declared and initialized to 0 in `reset()` but **never incremented or displayed**. Dead state.
- `ClickPanel::response_button` / `sticky_button` selectors exist but when using raw input bridge, **both** left and right clicks are always processed regardless of which button is selected — the selector only affects which stats are *displayed*, not which are *collected*. This could be confusing but is arguably a feature (collect all, display selected).

**Remediation:**
1. Remove `window_stutter_count` from `StutterPanel` or display it in the UI.
2. Remove `missed_events` from `ScrollPanel` or implement the missed-event detection logic.

---

### B6: Security Implementation — Score: 3/3 (Strong)

**Evidence:**
- No network calls anywhere in the codebase. No telemetry, no update checks, no remote data collection.
- CSV export uses `csv_escape()` (`export/mod.rs:202-208`) which handles commas, quotes, and newlines per RFC 4180.
- File paths are user-controlled via `rfd::FileDialog` — no arbitrary path construction from untrusted input.
- Device access uses OS-provided APIs (`evdev::Device::open`, WinAPI `RegisterRawInputDevices`) — no raw `/dev` file manipulation.
- No `unsafe` blocks in application code except the Windows input bridge, where `unsafe` is required for WinAPI FFI calls and is correctly scoped.

---

### B7: Resource Lifecycle Management — Score: 3/3 (Strong)

**Evidence:**
- `TerminalGuard` (`terminal.rs:82-100`) — RAII pattern ensures `disable_raw_mode()` is called even if a test panics.
- Bounded collections throughout:
  - `PollingPanel::history` — capped at 200 entries
  - `PollingPanel::event_times` — capped at 2000, pruned by 1-second window
  - `StutterPanel::deltas` — capped at 200
  - `ScrollPanel::scroll_events` — capped at 500
  - `ScrollPanel::speed_samples` — capped at 100
  - `ClickPanel::response_hold_times` — capped at 100
- Background thread lifetime: `InputBridge._thread: JoinHandle<()>` keeps the thread alive. When `InputBridge` is dropped, the channel sender in the thread gets a send error and exits.
- No file handles leaked — config read/write uses `fs::read_to_string` / `fs::write` (no manual file handle management).

---

### Domain B Summary

| Criterion | Score | Max |
|-----------|-------|-----|
| B1: Error Handling | 2 | 3 |
| B2: Config Consumption | 3 | 3 |
| B3: Call Chain Completeness | 3 | 3 |
| B4: Concurrency Correctness | 3 | 3 |
| B5: State Management | 2 | 3 |
| B6: Security | 3 | 3 |
| B7: Resource Lifecycle | 3 | 3 |
| **Domain B Total** | **19** | **21 (90.5%)** |

---

## Domain C: Interface Authenticity (Weight: 30%)

### C1: API Design Consistency — Score: 2/3 (Moderate)

**Evidence:**
- All 8 GUI panels expose identical signatures: `fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, raw_events: &[RawInputEvent], has_bridge: bool)` (except `DoubleClickPanel` which omits raw_events since it uses egui button clicks).
- All panels expose `fn export(&self) -> Option<*Export>`.
- All panels expose `fn is_running(&self) -> bool` and `fn new() -> Self`.
- This is **good API design** but the mechanical uniformity is itself an AI signature — a human team would likely have some variation (one panel might take a `&Config` reference, another might use a builder pattern, etc.).

---

### C2: UI Implementation Depth — Score: 3/3 (Strong)

**Evidence:**
- **Custom theme system** (`theme.rs`): 20+ color definitions organized by purpose (accent, background layers, sidebar, text hierarchy, borders, status). Glass-effect frames with translucent fills and subtle shadows.
- **Procedural icon**: Mouse head drawn with 3 circles (head + 2 ears) in both the sidebar (`app.rs:200-225`) and About dialog (`app.rs:569-594`), not a static asset.
- **Real-time graphs**: `egui_plot` with colored lines, horizontal threshold markers, dashed style for stutter threshold (`stutter.rs:206-208`).
- **Interactive test areas**: Color-changing click zones that respond to button state (`click.rs:177-188`), animated scroll direction indicators (`scroll.rs:90-136`).
- **Conditional repainting**: Only requests continuous repaint when a test is actively running (`app.rs:724-733`), conserving CPU when idle.
- **Easter egg**: `app.rs:656` — "MERRY CHRISTMAS // TO: XANDER FROM: DAD" in near-invisible text in the About dialog. This is the strongest human-authenticity signal in the entire codebase.

---

### C3: Frontend State Management — Score: 3/3 (Strong)

**Evidence:**
- `ActiveTest` enum (`app.rs:18-32`) cleanly drives navigation — single source of truth for which panel is displayed.
- Config dirty-flag pattern prevents unnecessary disk writes while ensuring changes are persisted.
- Per-panel state isolation — each panel owns its data with no shared mutable state between panels.
- Input bridge events are polled once per frame into a local `Vec`, then passed as immutable references to panels — clean ownership model.

---

### C4: Security Infrastructure — Score: 3/3 (Strong)

Appropriately scoped for a local desktop utility with no network, no authentication, and no user data beyond mouse event measurements. File operations are user-initiated through native file dialogs.

---

### C5: Real-Time Feature Completeness — Score: 3/3 (Strong)

**Evidence:**
- **Three-tier input architecture**: Raw input bridge (Linux evdev / Windows Raw Input) → fallback to egui pointer delta → graceful degradation message.
- **Linux event coalescing**: Merges REL_X + REL_Y events sharing the same kernel timestamp into a single Move event (`input_bridge.rs:173-199`). This is **critical correctness** — without it, polling rate would be 2x actual.
- **Windows blocking message loop**: Uses `GetMessageW` (blocks until message arrives) rather than `PeekMessageW` polling — correct and efficient.
- **High-resolution timestamps**: `Instant::now()` used at event reception time, giving microsecond-level precision for delta calculations.
- **Reasonable delta filtering**: `stutter.rs:303` rejects deltas > 100ms (mouse was stationary) and < 0.1ms (impossible at any polling rate), preventing corrupted data.

---

### C6: Error UX Handling — Score: 3/3 (Strong)

**Evidence:**
- Permission denied: Structured box with numbered solutions including the exact `usermod` command (`input.rs:86-91`, `input.rs:104-116`).
- Raw input unavailable: Yellow warning banner in every panel header (`polling.rs:66-69`, `stutter.rs:84-89`).
- Export errors: Status message in sidebar, color-coded red for errors / green for success (`app.rs:347-358`).
- Config parse failure: Warning to stderr + graceful fallback to defaults (`config.rs:49-52`).
- InputBridge startup: Diagnostic message identifying which device is selected (`input_bridge.rs:120-124`).

---

### C7: Logging & Observability — Score: 2/3 (Moderate)

**Evidence:**
- `eprintln!` used for startup diagnostics (InputBridge device selection, permission warnings, config errors).
- No structured logging framework (no `tracing`, `log`, `env_logger`).
- No metrics collection or performance counters.
- Sufficient for a desktop utility but would need structured logging for production troubleshooting.

**Remediation:** For a v0.1.0 desktop tool, this is adequate. If the project grows, consider adding `tracing` with `tracing-subscriber` for filterable structured logs.

---

### Domain C Summary

| Criterion | Score | Max |
|-----------|-------|-----|
| C1: API Design | 2 | 3 |
| C2: UI Depth | 3 | 3 |
| C3: State Management | 3 | 3 |
| C4: Security Infrastructure | 3 | 3 |
| C5: Real-Time Completeness | 3 | 3 |
| C6: Error UX | 3 | 3 |
| C7: Observability | 2 | 3 |
| **Domain C Total** | **19** | **21 (90.5%)** |

---

## Final Score Calculation

```
Domain A (Surface Provenance):    13/21 = 61.9% × 0.20 = 12.4%
Domain B (Behavioral Integrity):  19/21 = 90.5% × 0.50 = 45.2%
Domain C (Interface Authenticity): 19/21 = 90.5% × 0.30 = 27.1%
                                                         ──────
Weighted Authenticity Score:                              84.8%

Vibe-Code Confidence = 100% - 84.8% = 15.2%
```

### Classification: **Borderline Human-Authored / AI-Assisted (15.2%)**

| Range | Classification | This Project |
|-------|---------------|--------------|
| 0–15% | Human-Authored | |
| **16–35%** | **AI-Assisted** | **15.2% (boundary)** |
| 36–60% | AI-Scaffolded | |
| 61–85% | Substantially AI-Generated | |
| 86–100% | Almost Certainly AI-Generated | |

---

## Interpretation

This score deserves context. The **code quality is genuinely high** — it scores 90.5% on both Behavioral Integrity and Interface Authenticity. The platform-specific input handling, concurrency patterns, event coalescing, and GUI implementation all demonstrate real engineering understanding. There are no toy features, no mock data, no dead endpoints.

The score is pulled toward "AI-Assisted" primarily by Domain A — the commit history is transparent about AI authorship. This is **not a deception case**. The repository openly uses Claude-authored branches and commits. The low Domain A score reflects that the code was generated rather than iterated, not that it was generated poorly.

**Key strengths:**
- Correct concurrency patterns (mpsc channels, no shared mutable state)
- Deep platform API usage (evdev, WinAPI Raw Input) with proper error handling
- Complete call chains from input to analysis to display to export
- Bounded resource management throughout
- Genuine personal touch (Christmas message Easter egg)

**Key weaknesses:**
- Limited test coverage (19 tests, only in `analysis/` and `config`)
- Dead state fields (`window_stutter_count`, `missed_events`)
- Incomplete feature (`dark_mode` config exists but light theme not implemented)
- Project identity inconsistency (Mouse-TestKit vs Mouse TRAP)

---

## Top 5 Actionable Remediations

1. **Add tests for `csv_escape()` and `calculate_consistency()`** — These contain logic that could silently produce wrong output. Priority: High.

2. **Remove dead state** — Delete `StutterPanel::window_stutter_count` and `ScrollPanel::missed_events`, or implement the features they were intended for. Priority: Medium.

3. **Resolve project naming** — Choose either "Mouse-TestKit" or "Mouse TRAP" and use it consistently across binary names, config directories, documentation, and export metadata. Priority: Medium.

4. **Remove or implement `dark_mode` toggle** — Either remove `dark_mode` from `Config` (since it's always dark) or implement the light theme. Dead config fields are confusing. Priority: Low.

5. **Add integration tests for the input bridge event coalescing** — The REL_X/REL_Y merge logic in `linux_event_loop()` is correctness-critical for polling rate measurement. Extracting it into a testable pure function would be valuable. Priority: Medium.

---

*Audit conducted using the [Vibe-Code Detection Audit v2.0](https://github.com/kase1111-hash/Claude-prompts/blob/main/vibe-checkV2.md) methodology.*
