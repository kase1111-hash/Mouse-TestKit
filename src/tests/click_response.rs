//! Click Reaction Time Test
//!
//! Shows a cue after a random wait and measures how long the user takes to
//! click. The result is human reaction time plus terminal/display and input
//! latency: it cannot isolate the mouse's own click latency (a few ms),
//! which is far smaller than the variation in anyone's reaction.
//!
//! Clicks before the cue are false starts and the trial is repeated. The
//! press time is the device's own timestamp, so a click queued while the
//! program was waiting can never be read as a near-zero reaction.

use super::standard::TestOutcome;
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use mouse_testkit::types::TimedEvent;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseButton, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseButton, MouseEvent};

const NUM_TRIALS: usize = 10;
/// The wait before the cue is drawn at random from this range so its timing
/// cannot be learned (fixed or stepped delays let people click on rhythm).
pub const MIN_DELAY_MS: u64 = 1500;
pub const MAX_DELAY_MS: u64 = 4000;
/// Faster than this after the cue is an anticipation, not a reaction: human
/// simple visual reaction time does not go below about 100 ms.
pub const ANTICIPATION_MS: f64 = 100.0;
/// Waiting longer than this for a click ends the trial without a result.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// Repeated false starts in one trial skip it rather than loop forever
/// (e.g. on a button that is stuck or bouncing).
const MAX_FALSE_STARTS_PER_TRIAL: usize = 3;

/// How a press relates to the cue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PressTiming {
    /// Before the cue appeared.
    FalseStart,
    /// After the cue, but too soon to be a reaction to it.
    Anticipation,
    /// A real reaction, in milliseconds after the cue.
    Reaction(f64),
}

/// Classify a press by its time relative to the cue (negative = before).
pub fn classify_press(ms_after_cue: f64) -> PressTiming {
    if ms_after_cue < 0.0 {
        PressTiming::FalseStart
    } else if ms_after_cue < ANTICIPATION_MS {
        PressTiming::Anticipation
    } else {
        PressTiming::Reaction(ms_after_cue)
    }
}

/// Small xorshift generator for the random cue delays (no extra crates).
pub struct DelayRng(u64);

impl DelayRng {
    pub fn new(seed: u64) -> Self {
        // xorshift must not start at zero.
        Self(seed.max(1))
    }

    /// Seeded from the process's random hasher keys.
    pub fn from_entropy() -> Self {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        Self::new(hasher.finish())
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A delay in `MIN_DELAY_MS..=MAX_DELAY_MS`.
    pub fn next_delay_ms(&mut self) -> u64 {
        MIN_DELAY_MS + self.next_u64() % (MAX_DELAY_MS - MIN_DELAY_MS + 1)
    }
}

/// Print one line; works in raw terminal mode, where "\n" alone does not
/// return to the start of the line.
fn say(msg: &str) {
    print!("{}\r\n", msg);
    io::stdout().flush().ok();
}

/// Whether 'q' or Esc was pressed (raw mode must be on).
fn quit_pressed() -> bool {
    while event::poll(Duration::ZERO).unwrap_or(false) {
        if let Ok(Event::Key(key)) = event::read() {
            if key.kind == KeyEventKind::Press
                && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            {
                return true;
            }
        }
    }
    false
}

/// Signed milliseconds from `cue` to `t` (negative when `t` is earlier).
fn ms_after(cue: Instant, t: Instant) -> f64 {
    if t >= cue {
        t.duration_since(cue).as_secs_f64() * 1000.0
    } else {
        -(cue.duration_since(t).as_secs_f64() * 1000.0)
    }
}

/// How one trial ended.
enum Trial {
    Reaction(f64, MouseButton),
    FalseStart,
    Anticipation(f64),
    Timeout,
    Quit,
}

pub fn run() -> TestOutcome {
    println!("\n=== Click Reaction Time Test ===");
    println!("Measures how quickly you click after a cue appears.");
    println!("When you see 'CLICK NOW!', click as fast as possible. Wait for it:");
    println!("clicking early is a false start and the trial is repeated.");
    println!("\nNote: the result is YOUR reaction time plus display and input delay.");
    println!("It does not measure the mouse's own click latency (a few ms), which is");
    println!("far smaller than the variation in human reaction time.");
    println!("Press any key to start, or 'q' to quit...\n");

    {
        let _guard = terminal::TerminalGuard::new();
        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                // Windows also reports key releases (e.g. of the Enter that
                // chose this test); only a new key press starts it.
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.code == KeyCode::Char('q') {
                        return TestOutcome::Inconclusive; // _guard drops on return
                    }
                    break;
                }
            }
        }
    } // _guard drops here

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return TestOutcome::NoDevice;
        }
    };

    terminal::grab_device(&mut device);

    let mut rng = DelayRng::from_entropy();
    let mut results: Vec<ClickResult> = Vec::new();
    let mut false_starts = 0usize;
    let mut timeouts = 0usize;
    let mut quit = false;

    {
        // Raw mode so 'q' can stop the test between and during trials.
        let _guard = terminal::TerminalGuard::new();
        say("Press 'q' at any time to stop.");

        'trials: for trial in 0..NUM_TRIALS {
            let mut attempts = 0usize;
            loop {
                say("");
                say(&format!(
                    "Trial {}/{}  -  Get ready... (wait for the cue)",
                    trial + 1,
                    NUM_TRIALS
                ));
                let mut fetch = || input::fetch_timed_events(&mut device);
                match run_trial(&mut fetch, rng.next_delay_ms()) {
                    Trial::Reaction(ms, button) => {
                        say(&format!("Reaction time: {:.0} ms ({:?})", ms, button));
                        results.push(ClickResult {
                            latency_ms: ms,
                            button,
                        });
                        break;
                    }
                    Trial::Timeout => {
                        say("No click within 5 s - trial skipped");
                        timeouts += 1;
                        break;
                    }
                    Trial::Quit => {
                        quit = true;
                        break 'trials;
                    }
                    early => {
                        false_starts += 1;
                        attempts += 1;
                        match early {
                            Trial::Anticipation(ms) => say(&format!(
                                "Too fast ({:.0} ms) - that was a guess, not a reaction. Wait for the cue.",
                                ms
                            )),
                            _ => say("Too early! You clicked before the cue."),
                        }
                        if attempts >= MAX_FALSE_STARTS_PER_TRIAL {
                            say("Too many false starts - skipping this trial.");
                            break;
                        }
                        say("Repeating this trial...");
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    println!("\n=== Click Reaction Time Test Complete ===\n");
    if quit {
        println!("Stopped early.");
    }
    println!(
        "Valid trials: {} of {} | false starts: {} | no response: {}",
        results.len(),
        NUM_TRIALS,
        false_starts,
        timeouts
    );

    let outcome = if !results.is_empty() {
        let avg = results.iter().map(|r| r.latency_ms).sum::<f64>() / results.len() as f64;
        let mut sorted: Vec<f64> = results.iter().map(|r| r.latency_ms).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        println!("\nReaction time (you + display + input system):");
        println!("  Median:  {:.0} ms", median(&sorted));
        println!("  Average: {:.0} ms", avg);
        println!("  Fastest: {:.0} ms", sorted.first().unwrap_or(&0.0));
        println!("  Slowest: {:.0} ms", sorted.last().unwrap_or(&0.0));

        println!("\nAll responses:");
        for (i, result) in results.iter().enumerate() {
            println!("  #{}: {:.0} ms", i + 1, result.latency_ms);
        }

        println!("\nTypical results are about 200-300 ms. This is a measure of you and");
        println!("the whole system, not a pass/fail check of the mouse.");
        TestOutcome::Measured
    } else {
        println!("No valid clicks recorded.");
        TestOutcome::Inconclusive
    };

    terminal::wait_for_enter();
    outcome
}

/// Run one trial: random wait (a press here is a false start), then the
/// cue, then wait for the first press.
fn run_trial(fetch: &mut dyn FnMut() -> Vec<TimedEvent>, delay_ms: u64) -> Trial {
    // Discard anything from before this trial (e.g. the last click's release).
    let _ = fetch();

    let wait_end = Instant::now() + Duration::from_millis(delay_ms);
    while Instant::now() < wait_end {
        if quit_pressed() {
            return Trial::Quit;
        }
        for timed in fetch() {
            if let MouseEvent::ButtonPress(_) = timed.event {
                return Trial::FalseStart;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    say(">>> CLICK NOW! <<<");
    let cue = Instant::now();

    while cue.elapsed() < RESPONSE_TIMEOUT {
        if quit_pressed() {
            return Trial::Quit;
        }
        for timed in fetch() {
            if let MouseEvent::ButtonPress(button) = timed.event {
                // The device timestamp: a press made before the cue but read
                // after it is still a false start.
                let ms = ms_after(cue, timed.time);
                return match classify_press(ms) {
                    PressTiming::FalseStart => Trial::FalseStart,
                    PressTiming::Anticipation => Trial::Anticipation(ms),
                    PressTiming::Reaction(ms) => Trial::Reaction(ms, button),
                };
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Trial::Timeout
}

fn median(sorted: &[f64]) -> f64 {
    match sorted.len() {
        0 => 0.0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
    }
}

#[derive(Clone)]
pub struct ClickResult {
    /// Reaction time in ms (human + display + input latency).
    pub latency_ms: f64,
    #[allow(dead_code)]
    pub button: MouseButton,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_press_before_cue_is_false_start_not_zero_ms() {
        // A click during "Get ready" used to be read after the cue and
        // recorded as a ~0 ms response.
        assert_eq!(classify_press(-350.0), PressTiming::FalseStart);
        assert_eq!(classify_press(-0.5), PressTiming::FalseStart);
    }

    #[test]
    fn test_press_right_after_cue_is_anticipation() {
        assert_eq!(classify_press(0.0), PressTiming::Anticipation);
        assert_eq!(classify_press(60.0), PressTiming::Anticipation);
    }

    #[test]
    fn test_normal_reactions_are_kept() {
        assert_eq!(classify_press(100.0), PressTiming::Reaction(100.0));
        assert_eq!(classify_press(230.0), PressTiming::Reaction(230.0));
        // A slow, distracted reaction is still a reaction.
        assert_eq!(classify_press(900.0), PressTiming::Reaction(900.0));
    }

    #[test]
    fn test_delays_are_random_and_in_range() {
        let mut rng = DelayRng::new(12345);
        let delays: Vec<u64> = (0..200).map(|_| rng.next_delay_ms()).collect();
        assert!(delays
            .iter()
            .all(|d| (MIN_DELAY_MS..=MAX_DELAY_MS).contains(d)));
        // Not a fixed or stepped sequence: many distinct values, and
        // consecutive delays are not a constant step apart.
        let mut distinct = delays.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(distinct.len() > 150, "only {} distinct", distinct.len());
        let steps: Vec<i64> = delays
            .windows(2)
            .map(|w| w[1] as i64 - w[0] as i64)
            .collect();
        assert!(steps.windows(2).any(|w| w[0] != w[1]));
        // Spread over the range rather than bunched at one end.
        let mean = delays.iter().sum::<u64>() as f64 / delays.len() as f64;
        let mid = (MIN_DELAY_MS + MAX_DELAY_MS) as f64 / 2.0;
        assert!((mean - mid).abs() < 250.0, "mean {}", mean);
    }

    #[test]
    fn test_zero_seed_does_not_stick() {
        let mut rng = DelayRng::new(0);
        let a = rng.next_delay_ms();
        let b = rng.next_delay_ms();
        let c = rng.next_delay_ms();
        assert!(a != b || b != c);
    }

    #[test]
    fn test_ms_after_is_signed() {
        let cue = Instant::now();
        let later = cue + Duration::from_millis(250);
        assert!((ms_after(cue, later) - 250.0).abs() < 1e-6);
        assert!((ms_after(later, cue) + 250.0).abs() < 1e-6);
    }

    #[test]
    fn test_median() {
        assert_eq!(median(&[]), 0.0);
        assert_eq!(median(&[200.0]), 200.0);
        assert_eq!(median(&[200.0, 220.0, 400.0]), 220.0);
        assert_eq!(median(&[200.0, 220.0, 240.0, 400.0]), 230.0);
    }
}
