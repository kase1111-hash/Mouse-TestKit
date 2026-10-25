//! Acceleration Detection
//! Tests for unwanted acceleration in the mouse's sensor/firmware
//!
//! The raw counts read here come before the OS applies pointer acceleration,
//! so this test covers sensor/firmware acceleration only and says so.

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use mouse_testkit::analysis::acceleration::{
    ACCEL_TOLERANCE, MIN_SPEED_RATIO, MIN_STROKES_PER_GROUP,
};
use mouse_testkit::analysis::polling::median_in_place;
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

/// A recorded pass needs at least this many counts to be a deliberate movement.
const MIN_PASS_COUNTS: f64 = 100.0;

/// Returns false when no mouse was selected, so the test suite does not
/// count the test as run.
pub fn run() -> bool {
    println!("\n=== Acceleration Detection ===");
    println!("This test detects acceleration in the mouse's sensor or firmware.");
    println!("It reads raw counts, before the OS applies pointer acceleration, so OS");
    println!("acceleration (Windows 'Enhance pointer precision', libinput) is NOT covered.");
    println!("\nInstructions:");
    println!("  1. Move mouse SLOWLY across the same distance");
    println!("  2. Press SPACE to record slow movement");
    println!("  3. Move mouse QUICKLY across the same distance");
    println!("  4. Press SPACE to record fast movement");
    println!("  5. Record at least {} of each", MIN_STROKES_PER_GROUP);
    println!("Press SPACE as soon as a pass reaches its end mark. Sliding the mouse");
    println!("back to the start first is fine: only the last movement is recorded.\n");
    println!("With acceleration: fast movement = more counts");
    println!("Without acceleration: same distance = same counts\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return false;
        }
    };

    #[cfg(target_os = "linux")]
    device.grab().ok();

    let mut pass = Pass::default();
    let mut slow_samples: Vec<AccelSample> = Vec::new();
    let mut fast_samples: Vec<AccelSample> = Vec::new();
    let mut last_print = Instant::now();
    let mut recording_slow = true;
    // Some of the current pass was lost: it will not be recorded
    let mut spoiled = false;
    let mut gaps = 0usize;
    let mut device_lost = false;

    let _guard = terminal::TerminalGuard::new();

    println!("Recording SLOW movements first. Press 's' for slow, 'f' for fast mode.");
    println!("Press SPACE to record, 'r' to reset, 'q' to finish.\n");

    loop {
        // One Move per report, stamped when the device sent it, so the pass
        // duration is first to last movement (not until SPACE is pressed).
        let Ok(batch) = input::read_events(&mut device, terminal::READ_WAIT) else {
            device_lost = true;
            break;
        };
        gaps += usize::from(batch.lost_data);
        if batch.lost_data && !spoiled {
            // Dropped reports took their counts with them: reports only flow
            // while the mouse moves, so this pass lost some of its movement
            // (even one with no counts yet lost its start). The rest of it
            // must not be recorded as a pass of its own, so the SPACE that
            // ends it discards it.
            spoiled = true;
            println!(
                "\r\x1B[KSome movement was lost (the computer fell behind) - this pass will not be recorded. Finish it, press SPACE, then repeat it."
            );
        }
        for timed in batch.events {
            if let MouseEvent::Move { dx, dy } = timed.event {
                pass.add(timed.time, dx, dy);
            }
        }

        // Keys after the read: motion read just before SPACE belongs to the
        // pass it ends, as does a gap found there (the wait is on the mouse)
        if event::poll(Duration::ZERO).unwrap_or(false) {
            if let Some(code) = event::read().ok().and_then(|e| pressed_key(&e)) {
                match code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('s') => {
                        recording_slow = true;
                        pass = Pass::default();
                        spoiled = false;
                        println!("\r\x1B[KMode: SLOW - Move slowly and press SPACE");
                    }
                    KeyCode::Char('f') => {
                        recording_slow = false;
                        pass = Pass::default();
                        spoiled = false;
                        println!("\r\x1B[KMode: FAST - Move quickly and press SPACE");
                    }
                    KeyCode::Char('r') => {
                        pass = Pass::default();
                        spoiled = false;
                        println!("\r\x1B[KReset. Move and press SPACE to record.");
                    }
                    KeyCode::Char(' ') if spoiled => {
                        spoiled = false;
                        pass = Pass::default();
                        println!("\r\x1B[KPass not recorded. Move and press SPACE to record.");
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

        if last_print.elapsed() >= Duration::from_millis(100) {
            print!("\r\x1B[K");
            print!(
                "[{}] Counts: {:.0} | Slow: {} | Fast: {} | ",
                if recording_slow { "SLOW" } else { "FAST" },
                pass.counts(),
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
        // Medians, as in the GUI: one pass that went wrong cannot decide it
        let median = |samples: &[AccelSample], f: fn(&AccelSample) -> f64| {
            median_in_place(&mut samples.iter().map(f).collect::<Vec<_>>()).unwrap_or(0.0)
        };
        let slow_counts = median(&slow_samples, |s| s.counts);
        let fast_counts = median(&fast_samples, |s| s.counts);
        let slow_speed = median(&slow_samples, |s| s.speed);
        let fast_speed = median(&fast_samples, |s| s.speed);

        // If acceleration exists, fast counts will be higher than slow counts
        // for the same physical distance
        let count_ratio = fast_counts / slow_counts;
        let speed_ratio = fast_speed / slow_speed.max(f64::MIN_POSITIVE);

        println!("Slow movements: {} samples", slow_samples.len());
        println!("  Typical (median) counts: {:.0}", slow_counts);
        println!("  Typical (median) speed: {:.0} counts/s\n", slow_speed);

        println!("Fast movements: {} samples", fast_samples.len());
        println!("  Typical (median) counts: {:.0}", fast_counts);
        println!("  Typical (median) speed: {:.0} counts/s\n", fast_speed);

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

    terminal::print_lost_data_note(gaps);
    if device_lost {
        terminal::print_device_lost();
    }
    terminal::wait_for_enter();
    true
}

/// The key of a key press. Windows also reports key releases, and a SPACE
/// release would record the movement since the press as another pass.
fn pressed_key(event: &Event) -> Option<KeyCode> {
    match event {
        Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) => Some(*code),
        _ => None,
    }
}

/// Movement since the last SPACE press.
#[derive(Default)]
struct Pass {
    /// Every motion report: (time, dx, dy).
    reports: Vec<(Instant, i32, i32)>,
}

impl Pass {
    fn add(&mut self, t: Instant, dx: i32, dy: i32) {
        if dx != 0 || dy != 0 {
            self.reports.push((t, dx, dy));
        }
    }

    /// The pass itself: the movement from the point farthest from where the
    /// mouse ended up. Between passes the mouse has to get back to the start
    /// mark; when it is slid back rather than lifted, that return trip is
    /// left out instead of being added to the next pass's counts and time.
    fn leg(&self) -> &[(Instant, i32, i32)] {
        let end = self
            .reports
            .iter()
            .fold((0i64, 0i64), |(x, y), &(_, dx, dy)| {
                (x + dx as i64, y + dy as i64)
            });
        let (mut pos, mut start, mut farthest) = ((0i64, 0i64), 0, -1.0);
        for (i, &(_, dx, dy)) in self.reports.iter().enumerate() {
            // Distance to the end from where the mouse was before report i
            let to_end = ((end.0 - pos.0) as f64).hypot((end.1 - pos.1) as f64);
            if to_end >= farthest {
                farthest = to_end;
                start = i;
            }
            pos = (pos.0 + dx as i64, pos.1 + dy as i64);
        }
        &self.reports[start..]
    }

    /// Counts travelled during the pass.
    fn counts(&self) -> f64 {
        self.leg()
            .iter()
            .map(|&(_, dx, dy)| (dx as f64).hypot(dy as f64))
            .sum()
    }

    /// The pass as a sample, or `None` when it is too short to judge.
    fn sample(&self) -> Option<AccelSample> {
        let counts = self.counts();
        if counts < MIN_PASS_COUNTS {
            return None;
        }
        let leg = self.leg();
        let secs = match (leg.first(), leg.last()) {
            (Some(first), Some(last)) => last.0.saturating_duration_since(first.0).as_secs_f64(),
            _ => 0.0,
        }
        // A pass shorter than a millisecond counts as one, to avoid dividing by zero
        .max(0.001);
        Some(AccelSample {
            counts,
            secs,
            speed: counts / secs,
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
    use crossterm::event::KeyModifiers;

    #[test]
    fn key_releases_are_not_commands() {
        let key = |kind| {
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char(' '),
                KeyModifiers::NONE,
                kind,
            ))
        };
        assert_eq!(
            pressed_key(&key(KeyEventKind::Press)),
            Some(KeyCode::Char(' '))
        );
        assert_eq!(pressed_key(&key(KeyEventKind::Release)), None);
        assert_eq!(pressed_key(&key(KeyEventKind::Repeat)), None);
    }

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
    fn sliding_back_to_the_start_is_not_part_of_the_pass() {
        // After the previous pass the mouse is slid back 800 counts quickly,
        // rests on the start mark, then makes a slow 800-count pass (with a
        // little wobble) and stops on the end mark.
        let t0 = Instant::now();
        let mut pass = Pass::default();
        for i in 0..40u64 {
            pass.add(t0 + Duration::from_millis(i), -20, 0);
        }
        let start = t0 + Duration::from_millis(600);
        for i in 0..200u64 {
            let dy = [1, 0, -1, 0][i as usize % 4];
            pass.add(start + Duration::from_millis(5 * i), 4, dy);
        }
        let s = pass.sample().unwrap();
        assert!((s.counts - 800.0).abs() < 25.0, "{}", s.counts);
        assert!((s.secs - 0.995).abs() < 1e-6, "{}", s.secs);
        // Counting everything would double the counts and add the return
        // trip and the rest to the time
        let all: f64 = pass
            .reports
            .iter()
            .map(|&(_, dx, dy)| (dx as f64).hypot(dy as f64))
            .sum();
        assert!(all > 1500.0);
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
