//! Double-Click Test
//! Detects faulty switches causing unintended double-clicks
//!
//! A worn switch "bounces": as the contact opens it closes again for a
//! moment, so one click arrives as two. That shows either as two presses of
//! the same button closer together than any finger can manage, or as a press
//! arriving right after the previous release. Both checks use the Auto
//! Diagnostics thresholds (`analysis::diagnostics::Config`) and the device's
//! own timestamps.

use super::standard::TestOutcome;
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::diagnostics::{bounce_verdict, BounceVerdict, Config};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseButton, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseButton, MouseEvent};

/// Clicks needed before "no bounce" means anything (as in Auto Diagnostics).
fn min_clicks() -> usize {
    Config::default().clicks_min_left
}

pub fn run() -> TestOutcome {
    let cfg = Config::default();
    println!("\n=== Double-Click Detection Test ===");
    println!("This test detects faulty switches causing unintended double-clicks.");
    println!("\nInstructions:");
    println!("  1. Click normally at your regular pace - about 20 clicks per button");
    println!("  2. The test will detect any unintended rapid clicks");
    println!("  3. Faulty switches often register multiple clicks from one press\n");
    println!(
        "A press within {:.0} ms of the previous press, or within {:.0} ms of the previous",
        cfg.bounce_threshold_ms, cfg.bounce_gap_ms
    );
    println!("release, of the same button is flagged (a finger cannot do either).\n");

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
    // times: a stale double-click would read as a 0 ms bounce.
    terminal::discard_queued(|| input::fetch_timed_events(&mut device));
    let start = Instant::now();
    let mut detector = BounceDetector::new(&cfg);
    let mut last_print = Instant::now();

    let _guard = terminal::TerminalGuard::new();

    println!("Click at normal pace... Press 'q' to finish.\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            })) = event::read()
            {
                break;
            }
        }

        for timed in input::fetch_timed_events(&mut device) {
            let Some(t) = terminal::secs_after(start, timed.time) else {
                continue;
            };
            if let Some(dc) = detector.feed(t, &timed.event) {
                println!("\r\x1B[K⚠ DOUBLE-CLICK: {}", dc.describe());
            }
        }

        if last_print.elapsed() >= Duration::from_millis(500) {
            print!("\r\x1B[K");
            print!(
                "Clicks: {} | Double-clicks detected: {} | ",
                detector.presses,
                detector.events.len()
            );
            print!("Press 'q' to finish");
            io::stdout().flush().ok();
            last_print = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    drop(_guard);

    println!("\n\n=== Double-Click Test Complete ===\n");

    let double_clicks = &detector.events;
    println!("Total clicks: {}", detector.presses);
    println!(
        "By button: {}",
        super::click_sticky::per_button(&detector.presses_by_button()).0
    );
    println!("Double-clicks detected: {}\n", double_clicks.len());

    if !double_clicks.is_empty() {
        println!("By button:");
        for button in [
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Side,
            MouseButton::Extra,
        ] {
            let n = double_clicks.iter().filter(|d| d.button == button).count();
            if n > 0 {
                println!("  {:<7} {} occurrence(s)", format!("{:?}:", button), n);
            }
        }

        println!("\nRecent double-click events:");
        for (i, dc) in double_clicks.iter().rev().take(10).enumerate() {
            println!("  #{}: {}", i + 1, dc.describe());
        }
        println!();
    }

    let (outcome, mut lines) = verdict(double_clicks.len(), detector.presses, min_clicks());
    if outcome == TestOutcome::NoIssues {
        lines.extend(super::click_sticky::per_button(&detector.presses_by_button()).1);
    }
    for line in lines {
        println!("{}", line);
    }

    // Show click interval histogram (same button only: alternating buttons
    // quickly is not suspicious).
    if !detector.press_intervals_ms.is_empty() {
        let intervals = &detector.press_intervals_ms;
        println!("\nInterval between presses of the same button:");
        let count = |lo: f64, hi: f64| intervals.iter().filter(|i| **i >= lo && **i < hi).count();
        println!(
            "  <{:.0}ms:     {} (suspicious)",
            cfg.bounce_threshold_ms,
            count(0.0, cfg.bounce_threshold_ms)
        );
        println!(
            "  {:.0}-100ms:  {} (fast clicking)",
            cfg.bounce_threshold_ms,
            count(cfg.bounce_threshold_ms, 100.0)
        );
        println!("  100-200ms: {} (fast)", count(100.0, 200.0));
        println!("  >200ms:    {} (normal)", count(200.0, f64::INFINITY));
    }

    terminal::wait_for_enter();
    outcome
}

/// Overall verdict for `bounces` accidental double-clicks in `presses`.
pub fn verdict(bounces: usize, presses: usize, min_presses: usize) -> (TestOutcome, Vec<String>) {
    let rate = if presses > 0 {
        bounces as f64 / presses as f64 * 100.0
    } else {
        0.0
    };
    let judged = bounce_verdict(bounces);
    if judged == BounceVerdict::Failing {
        (
            TestOutcome::IssueFound,
            vec![
                format!("Double-click rate: {:.1}% of presses", rate),
                "\n⚠ SWITCH BOUNCE (accidental double clicks)".to_string(),
                "  Your mouse switches are failing.".to_string(),
                "  Consider switch replacement or mouse replacement.".to_string(),
            ],
        )
    } else if judged == BounceVerdict::Possible {
        (
            TestOutcome::PossibleIssue,
            vec![
                format!("Double-click rate: {:.1}% of presses", rate),
                "\n⚠ Possible switch bounce".to_string(),
                "  Re-run the test; if it happens again the switch is starting to fail."
                    .to_string(),
            ],
        )
    } else if presses < min_presses {
        (
            TestOutcome::Inconclusive,
            vec![format!(
                "Only {} click(s) recorded - click at least {} times (20 or more is better) for a verdict.",
                presses, min_presses
            )],
        )
    } else {
        (
            TestOutcome::NoIssues,
            vec![
                format!("✓ No double-click issues detected in {} clicks", presses),
                "  The switches of the buttons clicked appear healthy. An intermittent fault"
                    .to_string(),
                "  can need 50+ clicks to show up.".to_string(),
            ],
        )
    }
}

/// An accidental double-click.
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleClickEvent {
    pub button: MouseButton,
    /// Time since the previous press of the button (ms).
    pub interval_ms: Option<f64>,
    /// Time since the previous release of the button (ms).
    pub release_gap_ms: Option<f64>,
}

impl DoubleClickEvent {
    fn describe(&self) -> String {
        let mut parts = vec![format!("{:?}", self.button)];
        if let Some(gap) = self.release_gap_ms {
            parts.push(format!("{:.1} ms after release", gap));
        }
        if let Some(interval) = self.interval_ms {
            parts.push(format!("{:.1} ms after previous press", interval));
        }
        parts.join(" - ")
    }
}

#[derive(Default, Clone, Copy)]
struct ButtonState {
    last_press: Option<f64>,
    last_release: Option<f64>,
    presses: usize,
}

/// Flags presses that come too soon after the previous press or release of
/// the same button.
pub struct BounceDetector {
    press_threshold_ms: f64,
    release_gap_ms: f64,
    buttons: Vec<(MouseButton, ButtonState)>,
    pub presses: usize,
    pub events: Vec<DoubleClickEvent>,
    /// Press-to-press intervals of the same button (ms).
    pub press_intervals_ms: Vec<f64>,
}

impl BounceDetector {
    pub fn new(cfg: &Config) -> Self {
        Self {
            press_threshold_ms: cfg.bounce_threshold_ms,
            release_gap_ms: cfg.bounce_gap_ms,
            buttons: Vec::new(),
            presses: 0,
            events: Vec::new(),
            press_intervals_ms: Vec::new(),
        }
    }

    fn state(&mut self, button: MouseButton) -> &mut ButtonState {
        let pos = match self.buttons.iter().position(|(b, _)| *b == button) {
            Some(pos) => pos,
            None => {
                self.buttons.push((button, ButtonState::default()));
                self.buttons.len() - 1
            }
        };
        &mut self.buttons[pos].1
    }

    /// Presses per button, in the order the buttons first appeared.
    pub fn presses_by_button(&self) -> Vec<(String, usize)> {
        self.buttons
            .iter()
            .filter(|(_, s)| s.presses > 0)
            .map(|(b, s)| (format!("{:?}", b), s.presses))
            .collect()
    }

    /// Feed one event at `t` seconds. Returns the double-click it reveals.
    pub fn feed(&mut self, t: f64, event: &MouseEvent) -> Option<&DoubleClickEvent> {
        match event {
            MouseEvent::ButtonPress(button) => {
                let (press_threshold, gap_threshold) =
                    (self.press_threshold_ms, self.release_gap_ms);
                let state = self.state(*button);
                let interval = state.last_press.map(|p| (t - p) * 1000.0);
                let gap = state.last_release.map(|r| (t - r) * 1000.0);
                state.last_press = Some(t);
                state.presses += 1;
                self.presses += 1;
                if let Some(i) = interval {
                    self.press_intervals_ms.push(i);
                }

                let bounced = interval.is_some_and(|i| i < press_threshold)
                    || gap.is_some_and(|g| g < gap_threshold);
                if bounced {
                    self.events.push(DoubleClickEvent {
                        button: *button,
                        interval_ms: interval,
                        release_gap_ms: gap,
                    });
                    return self.events.last();
                }
                None
            }
            MouseEvent::ButtonRelease(button) => {
                self.state(*button).last_release = Some(t);
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(d: &mut BounceDetector, t_ms: f64, b: MouseButton) -> bool {
        d.feed(t_ms / 1000.0, &MouseEvent::ButtonPress(b)).is_some()
    }

    fn release(d: &mut BounceDetector, t_ms: f64, b: MouseButton) {
        d.feed(t_ms / 1000.0, &MouseEvent::ButtonRelease(b));
    }

    #[test]
    fn test_bounce_on_release_is_detected() {
        // 80 ms hold, release, phantom press 8 ms later: 88 ms press-to-press
        // passed the old press-only check as "fast clicking".
        let mut d = BounceDetector::new(&Config::default());
        assert!(!press(&mut d, 0.0, MouseButton::Left));
        release(&mut d, 80.0, MouseButton::Left);
        assert!(press(&mut d, 88.0, MouseButton::Left));
        let ev = &d.events[0];
        assert!((ev.release_gap_ms.unwrap() - 8.0).abs() < 1e-6);
        assert!((ev.interval_ms.unwrap() - 88.0).abs() < 1e-6);
    }

    #[test]
    fn test_two_presses_close_together_are_detected() {
        let mut d = BounceDetector::new(&Config::default());
        press(&mut d, 0.0, MouseButton::Left);
        // Release lost or merged; second press 20 ms after the first.
        assert!(press(&mut d, 20.0, MouseButton::Left));
    }

    #[test]
    fn test_fast_human_double_click_is_not_flagged() {
        // A quick deliberate double-click: 60 ms hold, 60 ms gap.
        let mut d = BounceDetector::new(&Config::default());
        press(&mut d, 0.0, MouseButton::Left);
        release(&mut d, 60.0, MouseButton::Left);
        assert!(!press(&mut d, 120.0, MouseButton::Left));
        release(&mut d, 175.0, MouseButton::Left);
        // Very fast clicking: 40 ms hold, 35 ms gap.
        assert!(!press(&mut d, 210.0, MouseButton::Left));
        release(&mut d, 250.0, MouseButton::Left);
        assert!(!press(&mut d, 285.0, MouseButton::Left));
        assert!(d.events.is_empty());
    }

    #[test]
    fn test_different_buttons_are_not_compared() {
        let mut d = BounceDetector::new(&Config::default());
        press(&mut d, 0.0, MouseButton::Left);
        release(&mut d, 5.0, MouseButton::Left);
        assert!(!press(&mut d, 10.0, MouseButton::Right));
        assert!(d.events.is_empty());
    }

    #[test]
    fn test_normal_session_verdict_is_healthy() {
        let mut d = BounceDetector::new(&Config::default());
        let mut t = 0.0;
        for i in 0..25 {
            // Irregular human pace: 250-450 ms between clicks, 70-130 ms holds.
            let hold = 70.0 + (i * 37 % 60) as f64;
            press(&mut d, t, MouseButton::Left);
            release(&mut d, t + hold, MouseButton::Left);
            t += 250.0 + (i * 53 % 200) as f64;
        }
        assert!(d.events.is_empty());
        let (outcome, _) = verdict(d.events.len(), d.presses, min_clicks());
        assert_eq!(outcome, TestOutcome::NoIssues);
    }

    #[test]
    fn test_presses_are_counted_per_button() {
        let mut d = BounceDetector::new(&Config::default());
        for i in 0..6 {
            press(&mut d, 400.0 * i as f64, MouseButton::Left);
            release(&mut d, 400.0 * i as f64 + 90.0, MouseButton::Left);
        }
        press(&mut d, 5000.0, MouseButton::Right);
        assert_eq!(
            d.presses_by_button(),
            vec![("Left".to_string(), 6), ("Right".to_string(), 1)]
        );
    }

    #[test]
    fn test_verdict_levels() {
        assert_eq!(verdict(3, 40, 5).0, TestOutcome::IssueFound);
        assert_eq!(verdict(1, 40, 5).0, TestOutcome::PossibleIssue);
        assert_eq!(verdict(2, 40, 5).0, TestOutcome::PossibleIssue);
        assert_eq!(verdict(0, 40, 5).0, TestOutcome::NoIssues);
        assert_eq!(verdict(0, 2, 5).0, TestOutcome::Inconclusive);
    }

    #[test]
    fn test_thresholds_come_from_auto_diagnostics() {
        let cfg = Config::default();
        let d = BounceDetector::new(&cfg);
        assert_eq!(d.press_threshold_ms, cfg.bounce_threshold_ms);
        assert_eq!(d.release_gap_ms, cfg.bounce_gap_ms);
    }
}
