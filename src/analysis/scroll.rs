//! Scroll wheel fault checks
//!
//! A wheel encoder reports one notch per detent, in the direction the wheel
//! turns. Two faults show up in that notch stream on their own:
//!
//! - **Skipping**: a worn or dirty encoder briefly reports a notch in the
//!   opposite direction while the wheel is turned one way, so the page jumps
//!   back. A finger cannot stop a wheel and turn it back within
//!   [`Config::scroll_reversal_ms`] (60 ms) of the previous notch, so a
//!   reversal that fast is counted as a skip. Auto Diagnostics counts skips
//!   with the same [`WheelCheck`].
//! - **Dead direction**: plenty of notches one way and none the other.
//!
//! Scrolling speed and rhythm are not judged: they describe the hand, not
//! the wheel. A notch that is lost without any report cannot be told apart
//! from the hand turning the wheel less, so it is not detected here.

use super::diagnostics::Config;

/// Repeated skips at or above this count mean the encoder is failing. One or
/// two can be a hand flicking the wheel back, so they only ask for a re-test.
/// Auto Diagnostics also needs this many before it reports skipping.
pub const SKIP_FAULT_COUNT: usize = 3;

/// What the notches seen so far say about the wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelVerdict {
    /// Not enough notches in both directions to judge yet.
    KeepScrolling,
    /// Many notches one way and none the other. `missing_up` is true when
    /// the up direction never registered.
    OneDirectionOnly { missing_up: bool },
    /// One or two notches reversed within the skip window.
    PossibleSkip,
    /// At least [`SKIP_FAULT_COUNT`] notches reversed within the skip window.
    Skipping,
    /// Enough notches both ways and none reversed.
    NoFaultFound,
}

/// Streaming scroll checker: feed it every wheel report in order.
#[derive(Debug, Clone)]
pub struct WheelCheck {
    reversal_ms: f64,
    min_each: usize,
    /// Notches reported upward (positive delta).
    pub up: usize,
    /// Notches reported downward (negative delta).
    pub down: usize,
    /// Wheel reports counted as skips.
    pub skips: usize,
    /// Time (s) and direction (+1/-1) of the previous report, and whether
    /// it was itself a skip.
    last: Option<(f64, i32, bool)>,
}

impl Default for WheelCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl WheelCheck {
    /// Checker using the Auto Diagnostics thresholds: a 60 ms skip window
    /// and 10 notches each way before the wheel is judged.
    pub fn new() -> Self {
        let cfg = Config::default();
        Self::with_thresholds(cfg.scroll_reversal_ms, cfg.scroll_target_each)
    }

    pub fn with_thresholds(reversal_ms: f64, min_each: usize) -> Self {
        Self {
            reversal_ms,
            min_each,
            up: 0,
            down: 0,
            skips: 0,
            last: None,
        }
    }

    /// Notches needed in each direction before the wheel is judged.
    pub fn min_each(&self) -> usize {
        self.min_each
    }

    /// Feed one wheel report at time `t` (seconds) moving `delta` notches
    /// (positive = up). Returns true when the report is counted as a skip.
    ///
    /// A skip is a report in the opposite direction to the previous one,
    /// within the skip window. The report that follows a skip and goes back
    /// the way the wheel was turning is the hand, not a second skip.
    pub fn feed(&mut self, t: f64, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let sign = delta.signum();
        let notches = delta.unsigned_abs() as usize;
        if sign > 0 {
            self.up += notches;
        } else {
            self.down += notches;
        }
        let skip = match self.last {
            Some((last_t, last_sign, last_was_skip)) => {
                sign != last_sign && !last_was_skip && (t - last_t) * 1000.0 < self.reversal_ms
            }
            None => false,
        };
        if skip {
            self.skips += 1;
        }
        self.last = Some((t, sign, skip));
        skip
    }

    pub fn total(&self) -> usize {
        self.up + self.down
    }

    pub fn verdict(&self) -> WheelVerdict {
        if self.skips >= SKIP_FAULT_COUNT {
            return WheelVerdict::Skipping;
        }
        if self.up == 0 && self.down >= self.min_each {
            return WheelVerdict::OneDirectionOnly { missing_up: true };
        }
        if self.down == 0 && self.up >= self.min_each {
            return WheelVerdict::OneDirectionOnly { missing_up: false };
        }
        if self.skips > 0 {
            return WheelVerdict::PossibleSkip;
        }
        if self.up < self.min_each || self.down < self.min_each {
            return WheelVerdict::KeepScrolling;
        }
        WheelVerdict::NoFaultFound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `(ms, delta)` reports.
    fn check(reports: &[(f64, i32)]) -> WheelCheck {
        let mut w = WheelCheck::new();
        for &(ms, delta) in reports {
            w.feed(ms / 1000.0, delta);
        }
        w
    }

    /// Steady scrolling: `n` notches `dir`, `gap` ms apart, from `start` ms.
    fn steady(start: f64, n: usize, dir: i32, gap: f64) -> Vec<(f64, i32)> {
        (1..=n).map(|i| (start + gap * i as f64, dir)).collect()
    }

    /// How people actually scroll: strokes of 3-6 quick notches (15-40 ms
    /// apart), a 150-400 ms pause to reposition the finger, the odd slow
    /// single notch, down first and then up after a pause.
    fn human_scrolling() -> Vec<(f64, i32)> {
        let mut out = Vec::new();
        let mut t = 0.0;
        for dir in [-1, 1] {
            for (stroke, &(notches, gap, pause)) in [
                (5, 25.0, 250.0),
                (3, 40.0, 400.0),
                (6, 15.0, 150.0),
                (1, 0.0, 600.0),
                (4, 30.0, 300.0),
            ]
            .iter()
            .enumerate()
            {
                for i in 0..notches {
                    t += if i == 0 && stroke > 0 { pause } else { gap };
                    out.push((t, dir));
                }
            }
            // Finger stops, then turns the wheel back the other way
            t += 180.0;
        }
        out
    }

    #[test]
    fn healthy_wheel_scrolled_by_a_person_has_no_fault() {
        let w = check(&human_scrolling());
        assert_eq!(w.down, 19);
        assert_eq!(w.up, 19);
        assert_eq!(w.skips, 0);
        assert_eq!(w.verdict(), WheelVerdict::NoFaultFound);
    }

    #[test]
    fn mixed_speeds_and_many_direction_changes_are_not_faults() {
        // Slow and fast scrolling, changing direction 6 times with normal
        // human reversal times (>= 120 ms): speed variation is not a fault.
        let mut reports = Vec::new();
        let mut t = 0.0;
        for (i, gap) in [200.0, 10.0, 120.0, 30.0, 300.0, 8.0].iter().enumerate() {
            let dir = if i % 2 == 0 { -1 } else { 1 };
            reports.extend(steady(t, 5, dir, *gap));
            t = reports.last().unwrap().0 + 120.0;
        }
        let w = check(&reports);
        assert_eq!(w.skips, 0);
        assert_eq!(w.verdict(), WheelVerdict::NoFaultFound);
    }

    #[test]
    fn worn_encoder_jumping_back_is_skipping() {
        // Scrolling down 30 ms per notch; every 6th report comes out as up
        let mut reports = Vec::new();
        for i in 1..=30 {
            let dir = if i % 6 == 0 { 1 } else { -1 };
            reports.push((i as f64 * 30.0, dir));
        }
        reports.extend(steady(1200.0, 12, 1, 30.0));
        let w = check(&reports);
        // Only the 5 wrong-way reports count, not the notch after each
        // one that goes back the way the wheel was turning
        assert_eq!(w.skips, 5);
        assert_eq!(w.verdict(), WheelVerdict::Skipping);
    }

    #[test]
    fn contact_bounce_reverse_notch_is_one_skip() {
        // Contact bounce: a reverse notch 2 ms after the real one
        let mut reports = steady(0.0, 10, -1, 40.0);
        reports.push((402.0, 1));
        reports.extend(steady(402.0, 10, -1, 40.0));
        reports.extend(steady(1500.0, 10, 1, 40.0));
        let w = check(&reports);
        assert_eq!(w.skips, 1);
        assert_eq!(w.verdict(), WheelVerdict::PossibleSkip);
    }

    #[test]
    fn one_quick_reversal_is_only_possible() {
        // A person flicking the wheel back once, just under the window,
        // then scrolling on the other way: at most one skip, never a fault.
        // Last down notch at 240 ms, first up notch 50 ms later.
        let mut reports = steady(0.0, 12, -1, 20.0);
        reports.extend(steady(270.0, 12, 1, 20.0));
        let w = check(&reports);
        assert_eq!(w.skips, 1);
        assert_eq!(w.verdict(), WheelVerdict::PossibleSkip);
    }

    #[test]
    fn dead_direction_is_reported_once_the_other_has_enough_notches() {
        let w = check(&steady(0.0, 9, -1, 50.0));
        assert_eq!(w.verdict(), WheelVerdict::KeepScrolling);
        let w = check(&steady(0.0, 15, -1, 50.0));
        assert_eq!(
            w.verdict(),
            WheelVerdict::OneDirectionOnly { missing_up: true }
        );
        let w = check(&steady(0.0, 15, 1, 50.0));
        assert_eq!(
            w.verdict(),
            WheelVerdict::OneDirectionOnly { missing_up: false }
        );
    }

    #[test]
    fn too_little_scrolling_is_inconclusive() {
        let mut reports = steady(0.0, 12, -1, 50.0);
        reports.extend(steady(1000.0, 4, 1, 50.0));
        assert_eq!(check(&reports).verdict(), WheelVerdict::KeepScrolling);
        assert_eq!(check(&[]).verdict(), WheelVerdict::KeepScrolling);
    }

    #[test]
    fn multi_notch_reports_count_every_notch() {
        let w = check(&[(0.0, -3), (10.0, -2), (500.0, 4)]);
        assert_eq!(w.down, 5);
        assert_eq!(w.up, 4);
        assert_eq!(w.total(), 9);
        assert_eq!(w.skips, 0);
    }

    #[test]
    fn uses_the_auto_diagnostics_window() {
        let cfg = Config::default();
        let w = WheelCheck::new();
        assert_eq!(w.reversal_ms, cfg.scroll_reversal_ms);
        assert_eq!(w.min_each(), cfg.scroll_target_each);
    }
}
