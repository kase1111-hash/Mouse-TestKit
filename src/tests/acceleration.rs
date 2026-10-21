//! Acceleration Detection
//! Tests for unwanted acceleration in the mouse's sensor/firmware
//!
//! The raw counts read here come before the OS applies pointer acceleration,
//! so this test covers sensor/firmware acceleration only and says so.

use crate::terminal;
use crossterm::event::{self, Event, KeyCode};
use mouse_testkit::analysis::acceleration::{
    ACCEL_TOLERANCE, MIN_SPEED_RATIO, MIN_STROKES_PER_GROUP,
};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

/// A recorded pass needs at least this many counts to be a deliberate movement.
const MIN_PASS_COUNTS: f64 = 100.0;

pub fn run() {
    println!("\n=== Acceleration Detection ===");
    println!("This test detects acceleration in the mouse's sensor or firmware.");
    println!("It reads raw counts, before the OS applies pointer acceleration, so OS");
    println!("acceleration (Windows 'Enhance pointer precision', libinput) is NOT covered.");
    println!("\nInstructions:");
    println!("  1. Move mouse SLOWLY across the same distance");
    println!("  2. Press SPACE to record slow movement");
    println!("  3. Move mouse QUICKLY across the same distance");
    println!("  4. Press SPACE to record fast movement");
    println!("  5. Record at least {} of each\n", MIN_STROKES_PER_GROUP);
    println!("With acceleration: fast movement = more counts");
    println!("Without acceleration: same distance = same counts\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return;
        }
    };

    #[cfg(target_os = "linux")]
    device.grab().ok();

    let mut pass = Pass::default();
    let mut slow_samples: Vec<AccelSample> = Vec::new();
    let mut fast_samples: Vec<AccelSample> = Vec::new();
    let mut last_print = Instant::now();
    let mut recording_slow = true;

    let _guard = terminal::TerminalGuard::new();

    println!("Recording SLOW movements first. Press 's' for slow, 'f' for fast mode.");
    println!("Press SPACE to record, 'r' to reset, 'q' to finish.\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('s') => {
                        recording_slow = true;
                        pass = Pass::default();
                        println!("\r\x1B[KMode: SLOW - Move slowly and press SPACE");
                    }
                    KeyCode::Char('f') => {
                        recording_slow = false;
                        pass = Pass::default();
                        println!("\r\x1B[KMode: FAST - Move quickly and press SPACE");
                    }
                    KeyCode::Char('r') => {
                        pass = Pass::default();
                        println!("\r\x1B[KReset. Move and press SPACE to record.");
                    }
                    KeyCode::Char(' ') => {
                        if let Some(sample) = pass.sample() {
                            let label = if recording_slow { "Slow" } else { "Fast" };
                            let list = if recording_slow {
                                &mut slow_samples
                            } else {
                                &mut fast_samples
                            };
                            list.push(sample);
                            println!(
                                "\r\x1B[K{} #{}: {:.0} counts in {:.2}s ({:.0} counts/s)",
                                label,
                                list.len(),
                                sample.counts,
                                sample.secs,
                                sample.speed
                            );
                            pass = Pass::default();
                        } else {
                            println!("\r\x1B[KNot enough movement. Try again.");
                        }
                    }
                    _ => {}
                }
            }
        }

        // One Move per report, stamped when the device sent it, so the pass
        // duration is first to last movement (not until SPACE is pressed).
        for timed in input::fetch_timed_events(&mut device) {
            if let MouseEvent::Move { dx, dy } = timed.event {
                pass.add(timed.time, dx, dy);
            }
        }

        if last_print.elapsed() >= Duration::from_millis(100) {
            print!("\r\x1B[K");
            print!(
                "[{}] Counts: {:.0} | Slow: {} | Fast: {} | ",
                if recording_slow { "SLOW" } else { "FAST" },
                pass.counts,
                slow_samples.len(),
                fast_samples.len()
            );
            io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    drop(_guard);

    println!("\n\n=== Acceleration Detection Complete ===\n");

    if !slow_samples.is_empty() && !fast_samples.is_empty() {
        let avg_slow_counts: f64 =
            slow_samples.iter().map(|s| s.counts).sum::<f64>() / slow_samples.len() as f64;
        let avg_fast_counts: f64 =
            fast_samples.iter().map(|s| s.counts).sum::<f64>() / fast_samples.len() as f64;

        let avg_slow_speed: f64 =
            slow_samples.iter().map(|s| s.speed).sum::<f64>() / slow_samples.len() as f64;
        let avg_fast_speed: f64 =
            fast_samples.iter().map(|s| s.speed).sum::<f64>() / fast_samples.len() as f64;

        // If acceleration exists, fast counts will be higher than slow counts
        // for the same physical distance
        let count_ratio = avg_fast_counts / avg_slow_counts;
        let speed_ratio = avg_fast_speed / avg_slow_speed.max(f64::MIN_POSITIVE);

        println!("Slow movements: {} samples", slow_samples.len());
        println!("  Average counts: {:.0}", avg_slow_counts);
        println!("  Average speed: {:.0} counts/s\n", avg_slow_speed);

        println!("Fast movements: {} samples", fast_samples.len());
        println!("  Average counts: {:.0}", avg_fast_counts);
        println!("  Average speed: {:.0} counts/s\n", avg_fast_speed);

        println!("Analysis:");
        println!("  Count ratio (fast/slow): {:.2}", count_ratio);
        println!("  Speed ratio (fast/slow): {:.2}", speed_ratio);

        match verdict(
            slow_samples.len(),
            fast_samples.len(),
            count_ratio,
            speed_ratio,
        ) {
            Verdict::TooFewPasses => {
                println!(
                    "\nInconclusive: record at least {} slow and {} fast passes.",
                    MIN_STROKES_PER_GROUP, MIN_STROKES_PER_GROUP
                );
            }
            Verdict::SpeedsTooSimilar => {
                println!(
                    "\nInconclusive: the fast passes were not at least {:.1}x faster than the slow ones.",
                    MIN_SPEED_RATIO
                );
                println!("  Make the slow passes slower and the fast passes faster.");
            }
            Verdict::Acceleration => {
                println!("\n⚠ SENSOR/FIRMWARE ACCELERATION DETECTED");
                println!(
                    "  Fast movements register {:.0}% more counts.",
                    (count_ratio - 1.0) * 100.0
                );
                println!("  Check the mouse's own software or on-board settings.");
            }
            Verdict::Deceleration => {
                println!("\n⚠ Negative acceleration (deceleration) detected");
                println!(
                    "  Fast movements register {:.0}% fewer counts.",
                    (1.0 - count_ratio) * 100.0
                );
                println!("  The sensor may be losing tracking at speed (surface, or above its speed limit).");
            }
            Verdict::None => {
                println!("\n✓ No significant sensor/firmware acceleration detected");
                println!("  The mouse reports the same counts for the same distance at any speed.");
            }
        }
        println!("\nNote: OS pointer acceleration acts after these raw counts and is not");
        println!("covered by this result; check that setting separately.");
    } else {
        println!("Need both slow and fast samples for comparison.");
        println!(
            "Slow samples: {}, Fast samples: {}",
            slow_samples.len(),
            fast_samples.len()
        );
    }

    terminal::wait_for_enter();
}

/// Movement accumulated since the last SPACE press.
#[derive(Default)]
struct Pass {
    counts: f64,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Pass {
    fn add(&mut self, t: Instant, dx: i32, dy: i32) {
        if dx == 0 && dy == 0 {
            return;
        }
        self.counts += ((dx as f64).powi(2) + (dy as f64).powi(2)).sqrt();
        self.first.get_or_insert(t);
        self.last = Some(t);
    }

    /// The pass as a sample, or `None` when it is too short to judge.
    fn sample(&self) -> Option<AccelSample> {
        if self.counts < MIN_PASS_COUNTS {
            return None;
        }
        let secs = match (self.first, self.last) {
            (Some(first), Some(last)) => last.saturating_duration_since(first).as_secs_f64(),
            _ => 0.0,
        }
        // A pass shorter than a millisecond counts as one, to avoid dividing by zero
        .max(0.001);
        Some(AccelSample {
            counts: self.counts,
            secs,
            speed: self.counts / secs,
        })
    }
}

#[derive(Clone, Copy)]
struct AccelSample {
    counts: f64,
    secs: f64,
    speed: f64,
}

#[derive(Debug, PartialEq)]
enum Verdict {
    TooFewPasses,
    SpeedsTooSimilar,
    Acceleration,
    Deceleration,
    None,
}

/// Same rules as the GUI (`analysis::acceleration`): enough passes, a real
/// speed contrast, and a count ratio outside 1 +/- ACCEL_TOLERANCE.
fn verdict(slow: usize, fast: usize, count_ratio: f64, speed_ratio: f64) -> Verdict {
    if slow < MIN_STROKES_PER_GROUP || fast < MIN_STROKES_PER_GROUP {
        Verdict::TooFewPasses
    } else if speed_ratio < MIN_SPEED_RATIO {
        Verdict::SpeedsTooSimilar
    } else if count_ratio > 1.0 + ACCEL_TOLERANCE {
        Verdict::Acceleration
    } else if count_ratio < 1.0 - ACCEL_TOLERANCE {
        Verdict::Deceleration
    } else {
        Verdict::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_duration_is_first_to_last_movement() {
        let t0 = Instant::now();
        let mut pass = Pass::default();
        for i in 0..100u64 {
            pass.add(t0 + Duration::from_millis(2 * i), 3, 4);
        }
        let s = pass.sample().unwrap();
        assert!((s.counts - 500.0).abs() < 1e-9);
        assert!((s.secs - 0.198).abs() < 1e-6);
        assert!(Pass::default().sample().is_none());
    }

    #[test]
    fn verdict_needs_passes_and_speed_contrast() {
        assert_eq!(verdict(1, 1, 1.5, 4.0), Verdict::TooFewPasses);
        // Fast passes no faster than slow ones: equal counts prove nothing
        assert_eq!(verdict(3, 3, 1.0, 1.1), Verdict::SpeedsTooSimilar);
        assert_eq!(verdict(3, 3, 1.02, 3.0), Verdict::None);
        assert_eq!(verdict(3, 3, 1.3, 3.0), Verdict::Acceleration);
        assert_eq!(verdict(3, 3, 0.7, 3.0), Verdict::Deceleration);
    }
}
