# Mouse-TestKit Refocus Plan

Based on the [Evaluation Report](EVALUATION_REPORT.md). Four phases, strict dependency order. Phase 1 is the only one that matters — it fixes the tool's credibility. Phases 2-4 are cleanup.

---

## Phase 1: Fix GUI Input Pipeline

**Goal:** Make GUI tests produce accurate measurements by reading raw input instead of egui pointer deltas.

**Why this is blocking:** Every timing-sensitive GUI panel reads `ctx.input(|i| i.pointer.delta())`, which is capped at the screen refresh rate (~60Hz). A 1000Hz mouse shows ~60Hz. The tool's core value proposition — precision diagnostics — is broken in the primary interface.

### 1.1 Create a shared input bridge

Create `src/gui/input_bridge.rs` — a background thread that reads raw platform input and sends events to the GUI via `mpsc::channel`.

**Architecture:**

```
┌──────────────────────┐      mpsc::channel       ┌──────────────────┐
│  Background Thread   │ ──────────────────────►   │   GUI App        │
│  (evdev / Raw Input) │    RawInputEvent stream   │   (egui panels)  │
└──────────────────────┘                           └──────────────────┘
```

**New types needed:**

```rust
// src/gui/input_bridge.rs

/// Platform-agnostic raw input event with high-resolution timestamp
pub struct RawInputEvent {
    pub kind: RawInputKind,
    pub timestamp: Instant,
}

pub enum RawInputKind {
    Move { dx: i32, dy: i32 },
    ButtonPress(RawButton),
    ButtonRelease(RawButton),
    Scroll { delta: i32 },
}

pub enum RawButton { Left, Right, Middle, Side, Extra }

/// Handle to the background input thread
pub struct InputBridge {
    receiver: mpsc::Receiver<RawInputEvent>,
    _thread: thread::JoinHandle<()>,
}

impl InputBridge {
    /// Start the background input thread. Returns None on platforms
    /// where raw input isn't available (macOS until IOKit support lands).
    pub fn start() -> Option<Self> { ... }

    /// Drain all pending events (non-blocking). Call once per frame.
    pub fn poll(&self) -> Vec<RawInputEvent> { ... }
}
```

**Linux implementation** (`#[cfg(target_os = "linux")]`):
- Reuse the device discovery logic from `src/input.rs:34-95` (`find_mouse_devices`)
- Auto-select the first mouse (no interactive prompt in GUI — add a device picker to the sidebar later)
- Spawn a thread that calls `device.fetch_events()` in a loop, converts `evdev::InputEvent` → `RawInputEvent` using the existing `parse_event` logic from `src/input.rs:206-233`, and sends over the channel
- The thread should NOT grab the device exclusively (the GUI still needs normal pointer input for its own UI)

**Windows implementation** (`#[cfg(target_os = "windows")]`):
- The architecture already exists in `src/input_windows.rs:290-363` (`run_input_loop`)
- Refactor: extract the message loop into `InputBridge::start()`, convert `RawMouseData` → `RawInputEvent`
- Remove the heartbeat hack at `src/input_windows.rs:358` — use `receiver.try_recv()` in the GUI thread and let the background thread block on `GetMessageW` instead of `PeekMessageW` + sleep

**macOS** (`#[cfg(target_os = "macos")]`):
- Return `None` from `InputBridge::start()` for now
- Panels fall back to `ctx.input()` when no bridge is available (graceful degradation)
- Document this limitation clearly in the UI ("Polling rate measurement requires Linux or Windows for raw input access")

### 1.2 Wire the bridge into the GUI app

**File: `src/gui/app.rs`**

Add `InputBridge` as a field on `MouseTestKitApp`:

```rust
pub struct MouseTestKitApp {
    // ... existing fields ...
    input_bridge: Option<InputBridge>,
    /// Events polled this frame, shared with all panels
    frame_events: Vec<RawInputEvent>,
}
```

In `MouseTestKitApp::new()`:
```rust
let input_bridge = InputBridge::start();
if input_bridge.is_none() {
    eprintln!("Note: Raw input not available on this platform. GUI tests will use framework input.");
}
```

In `fn update()` (`src/gui/app.rs:669`), poll once at the top of each frame:
```rust
self.frame_events = match &self.input_bridge {
    Some(bridge) => bridge.poll(),
    None => Vec::new(),
};
```

Pass `&self.frame_events` into each panel's `ui()` method.

### 1.3 Update all 8 GUI panels

Every panel needs its signature changed from:
```rust
pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context)
```
to:
```rust
pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, raw_events: &[RawInputEvent])
```

Then replace the `ctx.input()` calls with iteration over `raw_events`. Below is the specific change for each panel:

**`src/gui/panels/polling.rs`** (lines 128-171)
- Currently: counts `ctx.input(|i| i.pointer.delta())` events per second
- Change: count `RawInputEvent::Move` events per second from `raw_events`
- Fallback: if `raw_events` is empty and panel is running, fall back to `ctx.input()` for macOS
- This is the proof-of-concept panel — get this right first

**`src/gui/panels/stutter.rs`** (lines 266-310)
- Currently: measures `Instant::now()` delta between `ctx.input()` pointer moves
- Change: use `RawInputEvent.timestamp` deltas between consecutive `Move` events
- This is the most timing-sensitive panel — raw timestamps are critical here

**`src/gui/panels/click.rs`** (lines 270-348, 487-538, 647-679)
- Three sub-tests: response, sticky, liftoff
- Response/sticky: currently reads `pointer.primary_down()` / `pointer.secondary_down()`
- Change: detect `ButtonPress`/`ButtonRelease` events from `raw_events`
- Liftoff: currently reads `pointer.delta()` for movement + idle detection
- Change: use `Move` events from `raw_events`, detect idle from timestamp gaps
- Note: the click test area bounds check (`in_test_area`) can remain using `pointer.hover_pos()` since that's a UI concern, not a measurement

**`src/gui/panels/jitter.rs`** (lines 139-159)
- Currently: accumulates `pointer.delta()` during 5s sampling
- Change: accumulate `Move { dx, dy }` from `raw_events`
- Raw input gives integer counts (not float pixels), which is more accurate for jitter measurement

**`src/gui/panels/dpi.rs`** (lines 224-237)
- Currently: accumulates `pointer.delta()` Euclidean distance
- Change: accumulate `Move { dx, dy }` integer counts from `raw_events`
- This fixes DPI measurement — raw counts are what DPI is literally defined against

**`src/gui/panels/accel.rs`** (lines 194-212, 394-452)
- Two sub-tests: angle snapping and acceleration
- Both read `pointer.delta()` for movement tracking
- Change: use `Move` events from `raw_events`
- Velocity calculation in acceleration test needs raw timestamps for accuracy

**`src/gui/panels/scroll.rs`** (lines 291-349)
- Currently: reads `ctx.input(|i| i.raw_scroll_delta)`
- Change: use `Scroll { delta }` events from `raw_events`
- Note: `raw_scroll_delta` might already be accurate since scroll events are discrete; verify before changing

**`src/gui/panels/double_click.rs`** (lines 63, 93)
- Currently: uses egui `Button::clicked()` to detect clicks
- This panel does NOT need raw input — it's measuring human click timing on a UI button, not raw hardware latency. Leave as-is.

### 1.4 Fix unconditional `request_repaint()`

**File: `src/gui/app.rs:721`**

Currently `ctx.request_repaint()` runs on every frame unconditionally. Change to:

```rust
// Only request continuous repaints when a test is actively running
let any_test_running = /* check if any panel is in running state */;
if any_test_running {
    ctx.request_repaint();
}
```

This also means removing the per-panel `ctx.request_repaint()` calls (9 occurrences across all panels) and centralizing the decision in `app.rs`. Each panel should expose an `is_running()` method.

**Panels with `request_repaint()` to remove:**
- `polling.rs` — implicit (running state)
- `stutter.rs:268`
- `click.rs:272, 490, 650`
- `jitter.rs:140`
- `accel.rs:196, 396`
- `scroll.rs:293`
- `dpi.rs:225`

---

## Phase 2: Consolidate Shared Types and Logic

**Goal:** Eliminate duplication between CLI and GUI. Extract analysis into shared modules.

### 2.1 Unify input types

**Problem:** `MouseEvent` and `MouseButton` are defined identically in `src/input.rs:166-188` and `src/input_windows.rs:35-54`.

**Fix:** Create `src/types.rs` (or `src/common.rs`) with the canonical definitions. Both `input.rs` and `input_windows.rs` import from there. The GUI's `RawInputEvent` types from Phase 1 can also live here or map to these.

**Files to change:**
- Create `src/types.rs` with `MouseEvent`, `MouseButton`
- `src/input.rs` — remove enum definitions, add `use crate::types::*`
- `src/input_windows.rs` — remove enum definitions, add `use crate::types::*`
- `src/main.rs` — add `mod types;`
- `src/gui/main.rs` — add `mod types;` (or make it a shared lib)

### 2.2 Extract analysis functions

**Problem:** Test logic is reimplemented in GUI panels instead of shared with CLI.

**Fix:** Create `src/analysis/` module with pure functions:

```
src/analysis/
├── mod.rs
├── polling.rs      — PollingStats (already exists in src/tests/polling.rs:125-156)
├── stutter.rs      — analyze_stutter() (already exists in src/tests/stutter.rs:177-194)
├── dpi.rs          — DPI calculation math
└── ratings.rs      — Shared rating thresholds (Excellent/Good/Fair/Poor)
```

**Migration path:**
1. Move `PollingStats` from `src/tests/polling.rs:125-156` → `src/analysis/polling.rs`
2. Move `analyze_stutter()` and `StutterEvent`/`StutterSeverity` from `src/tests/stutter.rs:177-208` → `src/analysis/stutter.rs`
3. CLI tests (`src/tests/*.rs`) import from `src/analysis/`
4. GUI panels (`src/gui/panels/*.rs`) import from `src/analysis/`
5. Move unit tests alongside the analysis functions

**Do NOT move:**
- UI rendering code (stays in panels)
- Device selection / terminal handling (stays in CLI)
- Platform-specific input code (stays in `input.rs` / `input_windows.rs`)

### 2.3 Consider a library crate

If the shared code grows, restructure into:
```
mouse-testkit/          (lib crate — types, analysis, input bridge)
mouse-testkit-cli/      (bin crate — CLI binary)
mouse-testkit-gui/      (bin crate — GUI binary)
```

This is optional and can be deferred until the shared surface area justifies the Cargo workspace overhead. For now, `mod analysis` in the same crate is fine.

---

## Phase 3: Bug Fixes

All independent of each other. Can be done in any order or in parallel.

### 3.1 Fix light mode theme

**File: `src/gui/theme.rs`**

**Problem:** `ThemeColors` returns hardcoded dark-mode colors. `setup_custom_style()` applies them unconditionally, overwriting `Visuals::light()`.

**Fix:** Make `ThemeColors` methods accept a `dark_mode: bool` parameter, or split into `DarkTheme` / `LightTheme`. The simplest fix:

```rust
// Option A: Pass dark_mode flag
pub fn accent(dark: bool) -> Color32 { ... }
pub fn bg_dark(dark: bool) -> Color32 { ... }

// Option B: Thread-local or static
pub fn set_dark_mode(dark: bool) { ... }
```

Option A is cleaner. Thread the `dark_mode` bool through `setup_custom_style(ctx, dark_mode)` and all `ThemeColors` calls.

**Alternatively:** just remove the theme toggle until a proper light theme is designed. Dark-only is fine for v0.1. Remove the toggle button in `src/gui/app.rs:286-298` and the `dark_mode` field.

### 3.2 Fix Vec::remove(0) → VecDeque

**Files:**
- `src/gui/panels/click.rs:297` — `self.response_hold_times.remove(0)` → change `response_hold_times` to `VecDeque<f64>`
- `src/gui/panels/click.rs:326` — `self.response_right_hold_times.remove(0)` → same
- `src/gui/panels/accel.rs:209` — `self.angle_points.remove(0)` → change `angle_points` to `VecDeque<(f64, f64)>`
- `src/gui/panels/accel.rs:445` — `self.samples.remove(0)` → change `samples` to `VecDeque<AccelSample>`
- `src/tests/stutter.rs:57,60` — `deltas.remove(0)`, `timestamps.remove(0)` → change to `VecDeque`

Each is a mechanical change: `Vec<T>` → `VecDeque<T>`, `.remove(0)` → `.pop_front()`, add `use std::collections::VecDeque;`.

Watch out for `PlotPoints` collection — `VecDeque` implements `Iterator` so `.iter().enumerate().map(...)` will still work. The `egui_plot` `Points::new()` and `Line::new()` accept `PlotPoints` which collects from iterators, so no issue.

### 3.3 Fix Windows input heartbeat

**File: `src/input_windows.rs:344-361`**

**Problem:** The background thread sends empty `RawMouseData` structs every 100μs to check if the channel is alive. This floods the channel.

**Fix:** Replace `PeekMessageW` + sleep + heartbeat with `GetMessageW` (blocking). Use a separate `mpsc::channel<()>` or `Arc<AtomicBool>` for shutdown signaling:

```rust
// Shutdown signal
let running = Arc::new(AtomicBool::new(true));
let running_clone = running.clone();

let thread = thread::spawn(move || {
    // ... setup ...
    loop {
        if !running_clone.load(Ordering::Relaxed) { break; }
        // GetMessageW blocks until a message arrives — no busy loop
        if GetMessageW(&mut msg, hwnd, 0, 0) > 0 {
            if msg.message == WM_INPUT {
                process_raw_input(msg.lParam as HRAWINPUT, &sender);
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        } else {
            break;
        }
    }
});
```

On `MouseDevice` drop, set `running` to false and post `WM_QUIT` to unblock `GetMessageW`.

### 3.4 Fix CSV escaping

**File: `src/gui/export/mod.rs:206-315`**

**Problem:** Values are interpolated directly into CSV. Commas, quotes, or newlines in values break the format.

**Fix:** Add a helper function:

```rust
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
```

Apply to string values in CSV output. Numeric values don't need escaping. Currently only `ExportInfo` fields (`app_name`, `export_time`) could realistically contain problem characters, but the fix should be applied systematically.

**Alternative:** Add the `csv` crate as a dependency and use it properly. Slight dependency cost but eliminates the class of bugs entirely.

### 3.5 Fix documentation lies

**Files:**
- `readme.md:26` — Remove "Button Durability Test" from the features list
- `SPEC_SHEET.md` — Remove any Button Durability Test specification
- `CHANGELOG.md` — No change needed (it doesn't claim the feature)

Only re-add when the feature is actually implemented.

---

## Phase 4: Dead Code Cleanup

Low priority. Do after Phases 1-3.

### 4.1 Remove unused theme items

**File: `src/gui/theme.rs`**

Six `#[allow(dead_code)]` items:
- `warning()` (line 86)
- `error()` (line 91)
- `info()` (line 96)
- `accent_card_frame()` (line 213)
- `status_frame()` (line 223)
- `status_color()` / `warning_color()` / `info_color()` (lines 234-251)
- `muted_style()` (line 271)
- `metric_style()` (line 278)

**Decision:** If Phase 1-3 panels use any of these, keep them. Otherwise delete. Don't keep code "in case we need it later" — it's one `git revert` away.

### 4.2 Remove unused UsbBus struct

**File: `src/usb/conflicts.rs:217-232`**

`UsbBus` and its `has_conflicts()` method are unused. The `scan()` function implements conflict detection inline. Delete the struct and impl.

### 4.3 Use TerminalGuard in CLI tests

**File: `src/terminal.rs:82-101`**

`TerminalGuard` is the correct RAII pattern but is marked `#[allow(dead_code)]`. The CLI tests manually call `enable_raw_mode()` / `disable_raw_mode()`. Refactor tests to use `TerminalGuard` instead, which guarantees cleanup even on panic.

### 4.4 Remove `#[allow(dead_code)]` on ScrollEvent.delta

**File: `src/gui/panels/scroll.rs:32-33`**

`ScrollEvent.delta` is stored but never read. Either use it (display in UI or export) or remove the field.

---

## Execution Order

```
Phase 1.1  (input bridge)
    │
    ├──► Phase 1.2  (wire into app)
    │        │
    │        └──► Phase 1.3  (update all panels)
    │                 │
    │                 └──► Phase 1.4  (fix request_repaint)
    │
    ▼
Phase 2.1  (unify types)  ◄── can start after 1.1
    │
    └──► Phase 2.2  (extract analysis)

Phase 3.*  (all independent, can run in parallel with Phase 2)

Phase 4.*  (after everything else)
```

**Estimated scope:**
- Phase 1: ~400-600 lines new code, ~200 lines modified across 10 files
- Phase 2: ~100 lines new, ~150 lines moved/deleted
- Phase 3: ~50 lines changed across 6 files
- Phase 4: ~80 lines deleted

**Testing strategy:**
- Phase 1: Manual testing with a real mouse on Linux and Windows. Verify GUI polling rate matches CLI polling rate on the same hardware.
- Phase 2: Existing unit tests in `src/tests/polling.rs` and `src/tests/stutter.rs` should pass after migration with no changes.
- Phase 3: Compile + `cargo clippy` + `cargo test`.
- Phase 4: `cargo clippy` should show fewer `dead_code` warnings.

---

## What NOT to Do

- **Don't add macOS raw input yet.** IOKit/HID is a significant undertaking. The egui fallback is acceptable for macOS users with a "limited accuracy" disclaimer.
- **Don't restructure into a Cargo workspace.** The current single-crate structure is fine for this project size. A workspace adds CI complexity for marginal benefit.
- **Don't add new features.** No button durability test, no new panels, no cloud export. Fix the foundation first.
- **Don't rewrite the GUI framework.** egui/eframe is the right choice. The problem is the input source, not the framework.
