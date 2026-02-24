# Remediation Plan — Vibe-Code Detection Audit v2.0 Findings

**Date:** 2026-02-24
**Source:** [VIBE_CHECK_AUDIT.md](./VIBE_CHECK_AUDIT.md)
**Phases:** 5 (ordered by dependency and priority)

---

## Phase 1: Add Missing Unit Tests
**Priority:** High | **Complexity:** Low | **Risk:** None (additive only)
**Dependencies:** None — can be implemented first

### 1A: Tests for `csv_escape()` in `src/gui/export/mod.rs`

The `csv_escape()` function (line 202) handles commas, quotes, and newlines but has no tests. This function gates all CSV export output.

**File:** `src/gui/export/mod.rs` — add `#[cfg(test)] mod tests` block at end of file

**Tests to add:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_escape_plain_string() {
        assert_eq!(csv_escape("hello"), "hello");
    }

    #[test]
    fn csv_escape_with_comma() {
        assert_eq!(csv_escape("hello,world"), "\"hello,world\"");
    }

    #[test]
    fn csv_escape_with_quotes() {
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn csv_escape_with_newline() {
        assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn csv_escape_empty_string() {
        assert_eq!(csv_escape(""), "");
    }

    #[test]
    fn csv_escape_comma_and_quotes() {
        assert_eq!(csv_escape("a,\"b\""), "\"a,\"\"b\"\"\"");
    }
}
```

### 1B: Tests for `DoubleClickPanel::calculate_consistency()` in `src/gui/panels/double_click.rs`

The consistency calculation (line 239) uses coefficient of variation, which is non-trivial statistics. Currently zero test coverage.

**Problem:** `calculate_consistency()` and `register_click()` use `self.intervals` and `self.avg_interval` which are private struct fields. Tests must either:
- (a) Use `register_click()` to build state, but it depends on `Instant::now()` — not deterministic
- (b) Extract the consistency math into a standalone pure function

**Recommended approach:** Extract a pure function:

**File:** `src/gui/panels/double_click.rs`

1. Extract from `calculate_consistency()` (line 239-252) into a standalone function:
```rust
/// Calculate consistency score from a set of intervals.
/// Returns 0-100 where 100 = perfectly consistent.
fn consistency_score(intervals: &[f64]) -> f64 {
    if intervals.len() < 2 {
        return 0.0;
    }
    let mean: f64 = intervals.iter().sum::<f64>() / intervals.len() as f64;
    let variance: f64 = intervals.iter()
        .map(|x| (x - mean).powi(2))
        .sum::<f64>() / intervals.len() as f64;
    let std_dev = variance.sqrt();
    let coefficient_of_variation = std_dev / mean.max(1.0);
    (1.0 - coefficient_of_variation.min(1.0)) * 100.0
}
```

2. Make `calculate_consistency()` call the extracted function:
```rust
fn calculate_consistency(&self) -> f64 {
    consistency_score(&self.intervals)
}
```

3. Add tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consistency_perfect() {
        // Identical intervals = 100% consistency
        let intervals = vec![200.0, 200.0, 200.0, 200.0];
        assert_eq!(consistency_score(&intervals), 100.0);
    }

    #[test]
    fn consistency_empty() {
        assert_eq!(consistency_score(&[]), 0.0);
    }

    #[test]
    fn consistency_single() {
        assert_eq!(consistency_score(&[100.0]), 0.0);
    }

    #[test]
    fn consistency_moderate_variation() {
        // Some variation should give moderate score
        let intervals = vec![200.0, 250.0, 180.0, 220.0];
        let score = consistency_score(&intervals);
        assert!(score > 50.0 && score < 100.0,
            "Expected moderate consistency, got {}", score);
    }

    #[test]
    fn consistency_high_variation() {
        // Wild variation should give low score
        let intervals = vec![50.0, 500.0, 100.0, 800.0];
        let score = consistency_score(&intervals);
        assert!(score < 50.0, "Expected low consistency, got {}", score);
    }
}
```

### 1C: Tests for input bridge event coalescing logic

The REL_X/REL_Y merge in `src/gui/input_bridge.rs` `linux_event_loop()` (line 166-285) is correctness-critical — without it, polling rate reads 2x actual. However, this function takes a `Device` and `Sender` and runs an infinite loop, making it untestable directly.

**Recommended approach:** Extract the coalescing logic into a testable pure function:

**File:** `src/gui/input_bridge.rs`

1. Add a new function that takes a batch of parsed (axis, value, timestamp) tuples and returns coalesced `RawInputKind::Move` events:

```rust
/// Coalesce raw evdev-style axis events sharing the same timestamp into
/// combined Move events. This is the core algorithm extracted for testability.
///
/// Input: sequence of (axis: 'x'|'y', value: i32, timestamp_id: u64)
/// Output: Vec of coalesced (dx, dy) moves
#[cfg(test)]
fn coalesce_moves(events: &[(char, i32, u64)]) -> Vec<(i32, i32)> {
    let mut result = Vec::new();
    let mut pending_dx: i32 = 0;
    let mut pending_dy: i32 = 0;
    let mut last_ts: Option<u64> = None;

    for &(axis, value, ts) in events {
        if let Some(prev_ts) = last_ts {
            if ts != prev_ts && (pending_dx != 0 || pending_dy != 0) {
                result.push((pending_dx, pending_dy));
                pending_dx = 0;
                pending_dy = 0;
            }
        }
        last_ts = Some(ts);

        match axis {
            'x' => pending_dx += value,
            'y' => pending_dy += value,
            _ => {}
        }
    }

    // Flush remaining
    if pending_dx != 0 || pending_dy != 0 {
        result.push((pending_dx, pending_dy));
    }

    result
}
```

2. Add tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_xy_same_timestamp() {
        // X and Y at same timestamp -> single move
        let events = vec![('x', 5, 1), ('y', 3, 1)];
        assert_eq!(coalesce_moves(&events), vec![(5, 3)]);
    }

    #[test]
    fn coalesce_different_timestamps() {
        // Different timestamps -> separate moves
        let events = vec![('x', 5, 1), ('y', 3, 1), ('x', 2, 2), ('y', 4, 2)];
        assert_eq!(coalesce_moves(&events), vec![(5, 3), (2, 4)]);
    }

    #[test]
    fn coalesce_x_only() {
        let events = vec![('x', 10, 1)];
        assert_eq!(coalesce_moves(&events), vec![(10, 0)]);
    }

    #[test]
    fn coalesce_empty() {
        let events: Vec<(char, i32, u64)> = vec![];
        assert_eq!(coalesce_moves(&events), vec![]);
    }

    #[test]
    fn coalesce_multiple_x_same_timestamp() {
        // Multiple X events at same timestamp (rare but possible)
        let events = vec![('x', 3, 1), ('x', 2, 1), ('y', 1, 1)];
        assert_eq!(coalesce_moves(&events), vec![(5, 1)]);
    }
}
```

**Verification:** Run `cargo test --verbose` after implementation. All 19 existing tests plus ~16 new tests should pass.

---

## Phase 2: Remove Dead State
**Priority:** Medium | **Complexity:** Very Low | **Risk:** None (removals only)
**Dependencies:** None

### 2A: Remove `window_stutter_count` from `StutterPanel`

**File:** `src/gui/panels/stutter.rs`

| Line | Action |
|------|--------|
| 23 | Delete field declaration: `window_stutter_count: usize,` |
| 39 | Delete initialization: `window_stutter_count: 0,` |
| 356-358 | Delete computation block: `self.window_stutter_count = self.deltas.iter().filter(...)...` |
| 366 | Delete reset: `self.window_stutter_count = 0;` |
| 379 | Delete reset: `self.window_stutter_count = 0;` |

### 2B: Remove `missed_events` from `ScrollPanel`

**File:** `src/gui/panels/scroll.rs`

| Line | Action |
|------|--------|
| 27 | Delete field declaration: `missed_events: usize,` |
| 49 | Delete initialization: `missed_events: 0,` |
| 421 | Delete reset: `self.missed_events = 0;` |

**Verification:** `cargo clippy --all-targets --all-features -- -D warnings` should pass with no new warnings. `cargo build` should succeed.

---

## Phase 3: Resolve Project Naming
**Priority:** Medium | **Complexity:** Medium | **Risk:** Low (user-visible strings change)
**Dependencies:** None

The project has two names:
- **"Mouse-TestKit"** — Cargo package name, CLI binary, lib crate, README title
- **"Mouse TRAP"** — GUI window title, About dialog, config directory, export metadata, doc comments

**Decision required:** Choose one name. Recommendation: **Keep "Mouse TRAP"** as the user-facing brand (it's a clever acronym — Test Response And Positioning) and treat `mouse-testkit` as the internal crate/package name. The inconsistency is in the CLI banner and doc comments.

### 3A: Unify CLI banner to match brand

**File:** `src/main.rs`

| Line | Current | Change To |
|------|---------|-----------|
| 37 | `"║        Mouse-TestKit v0.1.0        ║"` | `"║         Mouse TRAP v0.1.0          ║"` |
| 38 | `"║     Mouse Testing Utility          ║"` | `"║   Test Response And Positioning    ║"` |
| 61 | `"Exiting Mouse-TestKit. Goodbye!"` | `"Exiting Mouse TRAP. Goodbye!"` |
| 73 | `"║        Mouse-TestKit v0.1.0        ║"` | `"║         Mouse TRAP v0.1.0          ║"` |
| 74 | `"║     Mouse Testing Utility          ║"` | `"║   Test Response And Positioning    ║"` |

### 3B: Unify doc comments

**File:** `src/main.rs` — line 1: change `//! Mouse-TestKit CLI Application` to `//! Mouse TRAP CLI Application`
**File:** `src/lib.rs` — line 1: change `//! Mouse-TestKit shared library` to `//! Mouse TRAP shared library`

### 3C: Keep `Cargo.toml` package name as `mouse-testkit`

The crate name `mouse-testkit` is used in `use mouse_testkit::types::...` throughout the codebase. Changing it would require updating all imports. **Leave it as-is** — package names don't need to match brand names.

**Files that already correctly use "Mouse TRAP"** (no changes needed):
- `src/gui/main.rs` — window title, `eframe::run_native` name
- `src/gui/app.rs` — dashboard heading, About dialog
- `src/gui/config.rs` — config directory (`mouse-trap`)
- `src/gui/export/mod.rs` — export metadata (`app_name: "Mouse TRAP"`)
- `src/gui/theme.rs`, `src/gui/panels/mod.rs` — doc comments

**Verification:** `cargo build --bin mouse-testkit` and `cargo build --bin mouse-testkit-gui` should succeed. Visually confirm CLI banner says "Mouse TRAP".

---

## Phase 4: Fix `dark_mode` Config Dead Path
**Priority:** Low | **Complexity:** Low | **Risk:** None
**Dependencies:** None

The `Config` struct has `dark_mode: bool` (default `true`) which is loaded and stored in `MouseTestKitApp.dark_mode`, but `app.rs:686` unconditionally sets dark visuals:

```rust
// Always dark — light mode theme not yet implemented.
ctx.set_visuals(egui::Visuals::dark());
```

**Two options:**

### Option A: Remove `dark_mode` entirely (Recommended — simpler)

Since light mode isn't implemented and the theme system (`theme.rs`) is built entirely for dark mode, remove the dead config field.

| File | Line | Action |
|------|------|--------|
| `src/gui/config.rs` | 13 | Delete `pub dark_mode: bool,` |
| `src/gui/config.rs` | 31 | Delete `dark_mode: true,` |
| `src/gui/app.rs` | 58 | Delete `dark_mode: bool,` |
| `src/gui/app.rs` | 106 | Delete `dark_mode: config.dark_mode,` |
| `src/gui/app.rs` | 117 | Delete `self.config.dark_mode = self.dark_mode;` |

Also update `test_config_default` in `src/gui/config.rs:130-136` to remove the `assert!(config.dark_mode)` line, and update `test_config_serialization` at line 141-157 to remove the `dark_mode` field.

**Note on backwards compatibility:** Existing `config.json` files on disk will have a `dark_mode` field. With `serde`, unknown fields are silently ignored by default, so removing the field from the struct will not cause parse failures. However, to be safe, add `#[serde(default)]` to the `Config` struct derive or ensure `serde(deny_unknown_fields)` is NOT present (it isn't).

### Option B: Implement light mode toggle

This is significantly more work (requires a full light theme color palette in `theme.rs`) and is not recommended as a remediation — it's a feature, not a fix.

**Verification:** `cargo test` should pass (config tests updated). `cargo build --bin mouse-testkit-gui` should succeed. Existing config files should load without error.

---

## Phase 5: Code Quality Polish (Optional)
**Priority:** Low | **Complexity:** Low | **Risk:** None
**Dependencies:** Phases 1-4

These are minor quality improvements found during the audit but not part of the top 5 remediations:

### 5A: Deduplicate scroll input processing

`src/gui/panels/scroll.rs` has nearly identical code blocks for the raw input path (lines 296-351) and the egui fallback path (lines 352-408). Extract shared logic into a `record_scroll_event(&mut self, direction_up: bool, abs_delta: f32, timestamp: Instant)` method.

### 5B: Fix Windows `let _ = sender.send(...)` pattern

In `src/gui/input_bridge.rs:450-507`, the Windows path uses `let _ = sender.send(...)` which silently discards send failures. The Linux path (lines 184-195, 212-219) correctly returns on error. Add `if sender.send(...).is_err() { return; }` for consistency.

### 5C: Deduplicate click panel code

`src/gui/panels/click.rs` has substantial duplication between left/right click handling in both the raw input and egui fallback paths. Consider extracting a helper method parameterized by button side.

---

## Execution Order Summary

```
Phase 1 (Tests)     ─┐
Phase 2 (Dead state) ─┼─ Independent, can be done in parallel
Phase 3 (Naming)     ─┤
Phase 4 (dark_mode)  ─┘
         │
         ▼
Phase 5 (Polish)     ── Optional, after core fixes
```

**Total files modified:** 7-8
**Total lines changed:** ~200 (mostly additions from tests)
**Estimated test count after:** 19 existing + ~16 new = ~35 tests
