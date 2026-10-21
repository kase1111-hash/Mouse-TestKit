//! Click Stickiness Test
//! Tests for stuck, late-releasing or bouncing buttons.
//!
//! Hold times come from the device's own timestamps. Thresholds are the
//! Auto Diagnostics ones (`analysis::diagnostics::Config`), so the CLI, the
//! GUI and the check-up agree on what counts as sticky: a deliberate normal
//! click is held for roughly 80-150 ms, so only holds well beyond that count,
//! and a single long hold is treated as the hand, not the switch.

use super::standard::{self, TestOutcome};
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::diagnostics::Config;
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseButton, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseButton, MouseEvent};

/// A click held longer than this (ms) is a slow/sticky release.
pub fn sticky_threshold_ms() -> f64 {
    Config::default().sticky_threshold_ms
}

pub fn run() -> TestOutcome {
    let cfg = Config::default();
    println!("\n=== Click Stickiness Test ===");
    println!("Click normally - short clicks, as when clicking a link - about 20 times");
    println!("with each button you want to check. The test measures how long each");
    println!(
        "click is held; holds over {:.0} ms are slow releases.",
        cfg.sticky_threshold_ms
    );
    println!("Press 'q' to quit.\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return TestOutcome::NoDevice;
        }
    };

    terminal::grab_device(&mut device);

    // Clicks made while the device list waited are queued with their own
    // times: they are not part of the test.
    terminal::discard_queued(|wait| input::read_events(&mut device, wait));
    let start = Instant::now();
    let mut tracker = HoldTracker::new(&cfg);
    let mut last_print = Instant::now();
    let mut gaps = 0usize;
    let mut device_lost = false;

    let _guard = terminal::TerminalGuard::new();

    println!("\nMonitoring... (press 'q' to quit)\n");

    loop {
        // The wait happens on the mouse below
        if event::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            })) = event::read()
            {
                break;
            }
        }

        let Ok(batch) = input::read_events(&mut device, terminal::READ_WAIT) else {
            // A button held when the mouse went away is no stuck switch
            device_lost = true;
            tracker.input_gap();
            break;
        };
        if batch.lost_data {
            gaps += 1;
            tracker.input_gap();
        }
        for timed in batch.events {
            let Some(t) = terminal::secs_after(start, timed.time) else {
                continue;
            };
            if let Some(hold) = tracker.feed(t, &timed.event) {
                if hold.is_sticky {
                    println!(
                        "\r\x1B[K⚠ Slow release: {} held for {:.0} ms",
                        hold.button, hold.duration_ms
                    );
                } else if hold.is_micro {
                    println!(
                        "\r\x1B[K⚠ {} press lasted only {:.1} ms (too short for a finger)",
                        hold.button, hold.duration_ms
                    );
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(500) {
            print!("\r\x1B[K");
            let holds = tracker.holds();
            let total = holds.len();
            let sticky_count = holds.iter().filter(|h| h.is_sticky).count();

            if total > 0 {
                let avg = holds.iter().map(|h| h.duration_ms).sum::<f64>() / total as f64;
                let max = holds.iter().map(|h| h.duration_ms).fold(f64::MIN, f64::max);
                print!(
                    "Clicks: {} | Slow (>{:.0} ms): {} | Avg hold: {:.0} ms | Max: {:.0} ms",
                    total, cfg.sticky_threshold_ms, sticky_count, avg, max
                );
            } else {
                print!("Waiting for clicks... (Press 'q' to quit)");
            }

            use std::io::Write;
            std::io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    drop(_guard);

    let now = Instant::now()
        .saturating_duration_since(start)
        .as_secs_f64();
    let mut report = tracker.report(now);
    if device_lost {
        report.outcome = standard::after_device_lost(report.outcome);
    }

    println!("\n\n=== Click Stickiness Test Complete ===\n");

    let holds = tracker.holds();
    if !holds.is_empty() {
        let total = holds.len();
        let avg = holds.iter().map(|h| h.duration_ms).sum::<f64>() / total as f64;

        let mut durations: Vec<f64> = holds.iter().map(|h| h.duration_ms).collect();
        durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        println!("Total clicks analyzed: {}", total);
        println!("By button: {}", per_button(&tracker.clicks_by_button()).0);
        println!(
            "Slow releases (>{:.0} ms): {}",
            cfg.sticky_threshold_ms, report.sticky
        );
        println!(
            "Too-short presses (<{:.0} ms, contact bounce): {}",
            cfg.micro_press_ms, report.micro
        );
        println!("\nHold duration statistics:");
        println!("  Average:  {:.1} ms", avg);
        println!("  Minimum:  {:.1} ms", durations.first().unwrap_or(&0.0));
        println!("  Maximum:  {:.1} ms", durations.last().unwrap_or(&0.0));
        if durations.len() >= 2 {
            println!("  Median:   {:.1} ms", durations[durations.len() / 2]);
        }
        println!();
    } else if report.held_at_exit.is_empty() {
        println!("No clicks recorded.");
    }

    for line in &report.lines {
        println!("{}", line);
    }

    terminal::print_lost_data_note(gaps);
    if device_lost {
        terminal::print_device_lost();
    }
    terminal::wait_for_enter();
    report.outcome
}

pub struct ClickHold {
    pub button: String,
    pub duration_ms: f64,
    pub is_sticky: bool,
    /// Shorter than a finger can press: contact bounce inside the switch.
    pub is_micro: bool,
}

/// Verdict at the end of the test.
pub struct StickyReport {
    pub outcome: TestOutcome,
    pub sticky: usize,
    pub micro: usize,
    /// Buttons still pressed when the test ended, with how long (ms).
    pub held_at_exit: Vec<(MouseButton, f64)>,
    /// Lines to print, most important first.
    pub lines: Vec<String>,
}

/// Pairs presses with releases and judges the hold times.
pub struct HoldTracker {
    sticky_ms: f64,
    stuck_ms: f64,
    micro_ms: f64,
    min_clicks: usize,
    pressed: Vec<(MouseButton, f64)>,
    holds: Vec<ClickHold>,
}

impl HoldTracker {
    pub fn new(cfg: &Config) -> Self {
        Self {
            sticky_ms: cfg.sticky_threshold_ms,
            stuck_ms: cfg.stuck_threshold_ms,
            micro_ms: cfg.micro_press_ms,
            min_clicks: cfg.clicks_min_left,
            pressed: Vec::new(),
            holds: Vec::new(),
        }
    }

    /// Feed one event at `t` seconds. Returns the finished click on release.
    pub fn feed(&mut self, t: f64, event: &MouseEvent) -> Option<&ClickHold> {
        match event {
            MouseEvent::ButtonPress(button) => {
                // A repeated press without a release keeps the first time.
                if !self.pressed.iter().any(|(b, _)| b == button) {
                    self.pressed.push((*button, t));
                }
                None
            }
            MouseEvent::ButtonRelease(button) => {
                // A release with no recorded press (e.g. the button was down
                // when the test started) is ignored.
                let pos = self.pressed.iter().position(|(b, _)| b == button)?;
                let (_, press_t) = self.pressed.remove(pos);
                let duration_ms = (t - press_t).max(0.0) * 1000.0;
                self.holds.push(ClickHold {
                    button: format!("{:?}", button),
                    duration_ms,
                    is_sticky: duration_ms > self.sticky_ms,
                    is_micro: duration_ms < self.micro_ms,
                });
                self.holds.last()
            }
            _ => None,
        }
    }

    /// Input was lost (reports dropped, or the mouse disconnected): a
    /// press still waiting for its release cannot be timed, so it is
    /// forgotten. A release that follows is then ignored.
    pub fn input_gap(&mut self) {
        self.pressed.clear();
    }

    pub fn holds(&self) -> &[ClickHold] {
        &self.holds
    }

    /// Clicks recorded per button, in the order the buttons first appeared.
    pub fn clicks_by_button(&self) -> Vec<(String, usize)> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for hold in &self.holds {
            match counts.iter_mut().find(|(b, _)| *b == hold.button) {
                Some((_, n)) => *n += 1,
                None => counts.push((hold.button.clone(), 1)),
            }
        }
        counts
    }

    /// Judge everything recorded, with `now` the time the test ended.
    pub fn report(&self, now: f64) -> StickyReport {
        let sticky = self.holds.iter().filter(|h| h.is_sticky).count();
        let micro = self.holds.iter().filter(|h| h.is_micro).count();
        let held_at_exit: Vec<(MouseButton, f64)> = self
            .pressed
            .iter()
            .map(|(b, t)| (*b, (now - t).max(0.0) * 1000.0))
            .collect();
        let stuck: Vec<&(MouseButton, f64)> = held_at_exit
            .iter()
            .filter(|(_, ms)| *ms > self.stuck_ms)
            .collect();
        let longest = self.holds.iter().map(|h| h.duration_ms).fold(0.0, f64::max);

        let mut lines = Vec::new();
        let mut problem = false;

        for (button, ms) in &stuck {
            problem = true;
            lines.push(format!(
                "⚠ {:?} button was still pressed when the test ended ({:.1} s). If you were not holding it, the switch is not releasing.",
                button,
                ms / 1000.0
            ));
        }
        for (button, ms) in held_at_exit.iter().filter(|(_, ms)| *ms <= self.stuck_ms) {
            lines.push(format!(
                "{:?} button was pressed when the test ended ({:.0} ms); that click was not judged.",
                button, ms
            ));
        }
        if micro > 0 {
            problem = true;
            lines.push(format!(
                "⚠ {} press(es) lasted under {:.0} ms, which a finger cannot do. This is contact bounce inside the switch.",
                micro, self.micro_ms
            ));
        }
        if sticky >= 2 {
            problem = true;
            lines.push(format!(
                "⚠ {} clicks were held longer than {:.0} ms (longest {:.0} ms). If you were clicking normally, the switch is releasing late.",
                sticky, self.sticky_ms, longest
            ));
        } else if sticky == 1 {
            lines.push(format!(
                "One click was held for {:.0} ms. A single long hold is usually the hand, not the switch.",
                longest
            ));
        }

        let outcome = if problem {
            TestOutcome::IssueFound
        } else if self.holds.len() < self.min_clicks {
            lines.push(format!(
                "Only {} click(s) recorded - click at least {} times (20 is better) for a verdict.",
                self.holds.len(),
                self.min_clicks
            ));
            TestOutcome::Inconclusive
        } else {
            // Only the buttons that registered were checked: a dead one
            // registers nothing.
            let (by_button, note) = per_button(&self.clicks_by_button());
            lines.push(format!(
                "✓ No sticky or bouncing clicks in {} clicks ({}). Those switches appear healthy.",
                self.holds.len(),
                by_button
            ));
            lines.extend(note);
            TestOutcome::NoIssues
        };

        StickyReport {
            outcome,
            sticky,
            micro,
            held_at_exit,
            lines,
        }
    }
}

/// "Left 20, Right 18" for the clicks each button registered, and a note
/// when only one button registered: a verdict on "the switches" covers only
/// the buttons listed, and a dead button registers nothing at all.
pub fn per_button(counts: &[(String, usize)]) -> (String, Option<String>) {
    let summary = counts
        .iter()
        .map(|(b, n)| format!("{} {}", b, n))
        .collect::<Vec<_>>()
        .join(", ");
    let note = match counts {
        [(only, _)] => Some(format!(
            "  Only {} clicks registered. If you also clicked other buttons, they did not register (a dead switch?).",
            only
        )),
        _ => None,
    };
    (summary, note)
}

#[allow(dead_code)]
pub fn is_sticky(duration_ms: f64) -> bool {
    duration_ms > sticky_threshold_ms()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `n` clicks of the left button with the given hold times, one
    /// every 400 ms.
    fn clicks(tracker: &mut HoldTracker, holds_ms: &[f64]) -> f64 {
        let mut t = 0.0;
        for hold in holds_ms {
            tracker.feed(t, &MouseEvent::ButtonPress(MouseButton::Left));
            tracker.feed(
                t + hold / 1000.0,
                &MouseEvent::ButtonRelease(MouseButton::Left),
            );
            t += 0.4;
        }
        t
    }

    #[test]
    fn test_verdict_names_the_buttons_that_registered() {
        // Left clicked 20 times; the right switch is dead and sends nothing
        let mut tracker = HoldTracker::new(&Config::default());
        let end = clicks(&mut tracker, &[90.0; 20]);
        let report = tracker.report(end);
        assert_eq!(report.outcome, TestOutcome::NoIssues);
        let text = report.lines.join("\n");
        assert!(text.contains("(Left 20)"), "{}", text);
        assert!(text.contains("Only Left clicks registered"), "{}", text);

        // Both buttons registered: no note
        let mut t = end;
        for _ in 0..10 {
            tracker.feed(t, &MouseEvent::ButtonPress(MouseButton::Right));
            tracker.feed(t + 0.09, &MouseEvent::ButtonRelease(MouseButton::Right));
            t += 0.4;
        }
        let text = tracker.report(t).lines.join("\n");
        assert!(text.contains("(Left 20, Right 10)"), "{}", text);
        assert!(!text.contains("Only"), "{}", text);
    }

    #[test]
    fn test_threshold_matches_auto_diagnostics() {
        assert_eq!(sticky_threshold_ms(), Config::default().sticky_threshold_ms);
        assert!(!is_sticky(110.0));
        assert!(is_sticky(400.0));
    }

    #[test]
    fn test_normal_deliberate_clicks_are_not_sticky() {
        // Normal clicks are held ~80-150 ms; the old 100 ms limit flagged them.
        let mut tracker = HoldTracker::new(&Config::default());
        let end = clicks(
            &mut tracker,
            &[85.0, 110.0, 140.0, 95.0, 150.0, 120.0, 105.0, 130.0],
        );
        let r = tracker.report(end);
        assert_eq!(r.sticky, 0);
        assert_eq!(r.outcome, TestOutcome::NoIssues);
    }

    #[test]
    fn test_one_long_hold_is_not_a_fault() {
        let mut tracker = HoldTracker::new(&Config::default());
        let end = clicks(&mut tracker, &[100.0, 120.0, 400.0, 110.0, 95.0, 105.0]);
        let r = tracker.report(end);
        assert_eq!(r.sticky, 1);
        assert_eq!(r.outcome, TestOutcome::NoIssues);
        assert!(r.lines.iter().any(|l| l.contains("usually the hand")));
    }

    #[test]
    fn test_repeated_late_releases_are_flagged() {
        let mut tracker = HoldTracker::new(&Config::default());
        let end = clicks(&mut tracker, &[100.0, 380.0, 120.0, 420.0, 110.0, 390.0]);
        let r = tracker.report(end);
        assert_eq!(r.sticky, 3);
        assert_eq!(r.outcome, TestOutcome::IssueFound);
    }

    #[test]
    fn test_button_still_held_at_exit_is_reported_stuck() {
        // A press that never releases used to vanish from the report.
        let mut tracker = HoldTracker::new(&Config::default());
        let t = clicks(&mut tracker, &[100.0, 110.0, 120.0, 105.0, 95.0]);
        tracker.feed(t, &MouseEvent::ButtonPress(MouseButton::Right));
        let r = tracker.report(t + 5.0);
        assert_eq!(r.held_at_exit.len(), 1);
        assert_eq!(r.held_at_exit[0].0, MouseButton::Right);
        assert_eq!(r.outcome, TestOutcome::IssueFound);
        assert!(r.lines[0].contains("still pressed"));
    }

    #[test]
    fn test_button_pressed_just_before_quitting_is_not_stuck() {
        let mut tracker = HoldTracker::new(&Config::default());
        let t = clicks(&mut tracker, &[100.0, 110.0, 120.0, 105.0, 95.0]);
        tracker.feed(t, &MouseEvent::ButtonPress(MouseButton::Left));
        let r = tracker.report(t + 0.2);
        assert_eq!(r.held_at_exit.len(), 1);
        assert_eq!(r.outcome, TestOutcome::NoIssues);
        assert!(r.lines.iter().any(|l| l.contains("not judged")));
    }

    #[test]
    fn test_micro_press_is_contact_bounce() {
        let mut tracker = HoldTracker::new(&Config::default());
        let end = clicks(&mut tracker, &[100.0, 1.0, 120.0, 105.0, 95.0]);
        let r = tracker.report(end);
        assert_eq!(r.micro, 1);
        assert_eq!(r.outcome, TestOutcome::IssueFound);
    }

    #[test]
    fn test_too_few_clicks_is_inconclusive() {
        let mut tracker = HoldTracker::new(&Config::default());
        let end = clicks(&mut tracker, &[100.0, 120.0]);
        assert_eq!(tracker.report(end).outcome, TestOutcome::Inconclusive);
    }

    #[test]
    fn test_input_lost_mid_click_is_not_judged() {
        // Left pressed, then reports are dropped (its release among them):
        // the late resynchronised release is not a slow release, and a
        // button held when the mouse disconnects is not a stuck switch
        let mut tracker = HoldTracker::new(&Config::default());
        let t = clicks(&mut tracker, &[100.0, 110.0, 120.0, 105.0, 95.0]);
        tracker.feed(t, &MouseEvent::ButtonPress(MouseButton::Left));
        tracker.input_gap();
        assert!(tracker
            .feed(t + 0.6, &MouseEvent::ButtonRelease(MouseButton::Left))
            .is_none());
        tracker.feed(t + 1.0, &MouseEvent::ButtonPress(MouseButton::Right));
        tracker.input_gap();
        let r = tracker.report(t + 5.0);
        assert_eq!(r.sticky, 0);
        assert!(r.held_at_exit.is_empty());
        assert_eq!(r.outcome, TestOutcome::NoIssues);

        // Without the gap, the same input is a slow release and a stuck
        // button: the checks still work
        let mut tracker = HoldTracker::new(&Config::default());
        let t = clicks(&mut tracker, &[100.0, 110.0, 120.0, 105.0, 95.0]);
        tracker.feed(t, &MouseEvent::ButtonPress(MouseButton::Right));
        assert_eq!(tracker.report(t + 5.0).outcome, TestOutcome::IssueFound);
    }

    #[test]
    fn test_release_without_press_is_ignored() {
        let mut tracker = HoldTracker::new(&Config::default());
        assert!(tracker
            .feed(0.1, &MouseEvent::ButtonRelease(MouseButton::Left))
            .is_none());
        assert!(tracker.holds().is_empty());
    }

    #[test]
    fn test_buttons_are_tracked_separately() {
        let mut tracker = HoldTracker::new(&Config::default());
        tracker.feed(0.0, &MouseEvent::ButtonPress(MouseButton::Left));
        tracker.feed(0.05, &MouseEvent::ButtonPress(MouseButton::Right));
        let right = tracker
            .feed(0.15, &MouseEvent::ButtonRelease(MouseButton::Right))
            .map(|h| h.duration_ms)
            .unwrap();
        assert!((right - 100.0).abs() < 1e-6);
        let left = tracker
            .feed(0.2, &MouseEvent::ButtonRelease(MouseButton::Left))
            .map(|h| h.duration_ms)
            .unwrap();
        assert!((left - 200.0).abs() < 1e-6);
    }
}
