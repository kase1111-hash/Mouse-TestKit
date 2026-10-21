//! DPI Accuracy Test
//! Verifies mouse DPI matches advertised/configured value
//!
//! The user moves the mouse a measured distance along a ruler. Counting
//! starts and stops on a key press, so positioning the mouse on the start
//! mark is not counted, and the full 2-D displacement is used so a slightly
//! slanted (or vertical) move measures correctly. A longer distance shrinks
//! the effect of placing the mouse a millimetre off at either end.

use super::standard::TestOutcome;
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

const MM_PER_INCH: f64 = 25.4;
/// Distance used when the user just presses Enter: long enough that a 1 mm
/// placement error is 1%, short enough for any mousepad.
const DEFAULT_DISTANCE_MM: f64 = 100.0;
/// Allowed distance range in mm (1 cm to 1 m).
const MIN_DISTANCE_MM: f64 = 10.0;
const MAX_DISTANCE_MM: f64 = 1000.0;
/// How far off the marks a careful hand places the mouse in total (start
/// plus end, about 1 mm each). Deviations within this are not judged.
const PLACEMENT_ERROR_MM: f64 = 2.0;
/// Deviation limits for the verdict, in percent.
const ACCURATE_PCT: f64 = 5.0;
const MINOR_PCT: f64 = 10.0;
/// Below this many counts a sample is treated as "no movement".
const MIN_SAMPLE_COUNTS: f64 = 10.0;

pub fn run() -> TestOutcome {
    println!("\n=== DPI Accuracy Test ===");
    println!("This test measures your mouse's actual DPI against a ruler.");

    // Get expected DPI from user
    print!("\nEnter your mouse's configured DPI (e.g., 800, 1600): ");
    io::stdout().flush().ok();
    let mut dpi_input = String::new();
    io::stdin().read_line(&mut dpi_input).ok();
    let expected_dpi = match parse_dpi(&dpi_input) {
        Some(d) => d,
        None => {
            println!("Invalid DPI value. Using 800 as default.");
            800.0
        }
    };

    print!("Distance you will move, e.g. 10cm, 100mm or 4in [Enter = 10cm]: ");
    io::stdout().flush().ok();
    let mut dist_input = String::new();
    io::stdin().read_line(&mut dist_input).ok();
    let distance_mm = match parse_distance_mm(&dist_input) {
        Some(Distance { mm, unit_given }) => {
            if !unit_given {
                println!("No unit given - assuming cm.");
            }
            mm
        }
        None => {
            if !dist_input.trim().is_empty() {
                println!(
                    "Could not read that distance (use {:.0}-{:.0} cm). Using 10 cm.",
                    MIN_DISTANCE_MM / 10.0,
                    MAX_DISTANCE_MM / 10.0
                );
            }
            DEFAULT_DISTANCE_MM
        }
    };
    let distance_label = format_distance(distance_mm);

    println!("\nExpected DPI: {}", expected_dpi);
    println!("Distance: {}", distance_label);
    println!(
        "A 1 mm placement error changes the result by {:.1}%.",
        100.0 / distance_mm
    );
    if distance_mm < 50.0 {
        println!("Tip: a longer distance (10 cm or more) gives a more reliable result.");
    }

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return TestOutcome::NoDevice;
        }
    };

    terminal::grab_device(&mut device);

    let mut samples: Vec<DpiSample> = Vec::new();
    let mut counting = false;
    let (mut total_x, mut total_y) = (0i64, 0i64);
    let mut last_print = Instant::now();

    let _guard = terminal::TerminalGuard::new();

    let say = |msg: &str| {
        print!("\r\x1B[K{}\r\n", msg);
        io::stdout().flush().ok();
    };
    say("");
    say("Instructions:");
    say("  1. Hold the mouse against the START mark of your ruler and keep it still");
    say("  2. Press SPACE to start counting");
    say(&format!(
        "  3. Slide it exactly {} along the ruler in one movement, without lifting",
        distance_label
    ));
    say("     (any direction: horizontal, vertical or slanted)");
    say("  4. Hold it still on the END mark and press SPACE to record");
    say("  Repeat 3 or more times. 'r' cancels the current sample, 'q' finishes.");
    say("");

    loop {
        for (dx, dy) in drain_moves(&mut device) {
            if counting {
                total_x += dx as i64;
                total_y += dy as i64;
            }
        }

        // The mouse is held still on a mark when SPACE is pressed, so motion
        // read just before the key belongs to the sample it ends.
        if let Some(key) = poll_key_press() {
            match key {
                KeyCode::Char('q') => break,
                KeyCode::Char('r') => {
                    counting = false;
                    say("Sample cancelled. Put the mouse on the start mark and press SPACE.");
                }
                KeyCode::Char(' ') if !counting => {
                    counting = true;
                    (total_x, total_y) = (0, 0);
                    say(&format!(
                        "Counting... slide {} and press SPACE on the end mark.",
                        distance_label
                    ));
                }
                KeyCode::Char(' ') => {
                    counting = false;
                    match DpiSample::new(total_x, total_y, distance_mm, expected_dpi) {
                        Some(sample) => {
                            say(&format!(
                                "Sample {}: {:.0} counts (X {:+}, Y {:+}) over {} = {:.0} DPI ({:.1}% of expected)",
                                samples.len() + 1,
                                sample.counts,
                                total_x,
                                total_y,
                                distance_label,
                                sample.measured_dpi,
                                sample.accuracy
                            ));
                            samples.push(sample);
                        }
                        None => say("Not enough movement detected. Try again."),
                    }
                }
                _ => {}
            }
        }

        if last_print.elapsed() >= Duration::from_millis(100) {
            print!("\r\x1B[K");
            if counting {
                let net = net_counts(total_x, total_y);
                print!(
                    "Counting | X: {:+6} | Y: {:+6} | net {:.0} counts = {:.0} DPI at {}",
                    total_x,
                    total_y,
                    net,
                    dpi_from_counts(net, distance_mm),
                    distance_label
                );
            } else {
                print!(
                    "Ready - mouse on the start mark, then SPACE | Samples: {}",
                    samples.len()
                );
            }
            io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    drop(_guard);

    println!("\n\n=== DPI Accuracy Test Complete ===\n");

    let outcome = if !samples.is_empty() {
        let avg_dpi: f64 =
            samples.iter().map(|s| s.measured_dpi).sum::<f64>() / samples.len() as f64;
        let avg_accuracy: f64 =
            samples.iter().map(|s| s.accuracy).sum::<f64>() / samples.len() as f64;

        let mut dpis: Vec<f64> = samples.iter().map(|s| s.measured_dpi).collect();
        dpis.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        println!("Expected DPI: {:.0}", expected_dpi);
        println!("Distance used: {}", distance_label);
        println!("Samples taken: {}\n", samples.len());

        println!("Results:");
        println!("  Average measured DPI: {:.0}", avg_dpi);
        println!(
            "  DPI range: {:.0} - {:.0}",
            dpis.first().copied().unwrap_or(0.0),
            dpis.last().copied().unwrap_or(0.0)
        );
        println!("  Average accuracy: {:.1}%", avg_accuracy);

        let deviation = ((avg_dpi - expected_dpi) / expected_dpi * 100.0).abs();
        println!("  Deviation from expected: {:.1}%", deviation);

        println!("\nSamples:");
        for (i, sample) in samples.iter().enumerate() {
            println!(
                "  #{}: {:.0} counts over {} = {:.0} DPI ({:.1}%)",
                i + 1,
                sample.counts,
                distance_label,
                sample.measured_dpi,
                sample.accuracy
            );
        }

        let (outcome, message) = verdict(deviation, distance_mm);
        println!("\n{}", message);
        if samples.len() < 3 {
            println!("Take 3 or more samples to average out hand placement.");
        }
        outcome
    } else {
        println!("No samples recorded.");
        TestOutcome::Inconclusive
    };

    terminal::wait_for_enter();
    outcome
}

/// The key just pressed, if any. Windows also reports key releases; those
/// are skipped, or releasing SPACE would stop the sample it just started.
fn poll_key_press() -> Option<KeyCode> {
    if !event::poll(Duration::from_millis(1)).unwrap_or(false) {
        return None;
    }
    match event::read() {
        Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => Some(key.code),
        _ => None,
    }
}

/// Read pending motion as (dx, dy) per report.
#[cfg(target_os = "linux")]
fn drain_moves(device: &mut evdev::Device) -> Vec<(i32, i32)> {
    moves(input::fetch_timed_events(device))
}

#[cfg(target_os = "windows")]
fn drain_moves(device: &mut input::MouseDevice) -> Vec<(i32, i32)> {
    moves(input::fetch_timed_events(device))
}

fn moves(events: Vec<input::TimedEvent>) -> Vec<(i32, i32)> {
    events
        .into_iter()
        .filter_map(|timed| match timed.event {
            MouseEvent::Move { dx, dy } => Some((dx, dy)),
            _ => None,
        })
        .collect()
}

/// Parse a DPI value typed by the user. Accepts 50 to 100000.
pub fn parse_dpi(input: &str) -> Option<f64> {
    let dpi: f64 = input.trim().parse().ok()?;
    (50.0..=100_000.0).contains(&dpi).then_some(dpi)
}

/// A distance typed by the user.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distance {
    pub mm: f64,
    /// False when a bare number was typed and cm was assumed.
    pub unit_given: bool,
}

/// Parse "10cm", "100 mm", "4in", "4\"" or a bare number (cm). Empty input,
/// unknown units and distances outside 1 cm - 1 m give `None`.
pub fn parse_distance_mm(input: &str) -> Option<Distance> {
    let s = input.trim().to_ascii_lowercase();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (number, unit) = s.split_at(split);
    let value: f64 = number.parse().ok()?;
    let (mm_per_unit, unit_given) = match unit.trim() {
        "" => (10.0, false),
        "mm" => (1.0, true),
        "cm" => (10.0, true),
        "in" | "inch" | "inches" | "\"" => (MM_PER_INCH, true),
        _ => return None,
    };
    let mm = value * mm_per_unit;
    (MIN_DISTANCE_MM..=MAX_DISTANCE_MM)
        .contains(&mm)
        .then_some(Distance { mm, unit_given })
}

fn format_distance(mm: f64) -> String {
    format!("{:.1} cm ({:.2} in)", mm / 10.0, mm / MM_PER_INCH)
}

/// Straight-line length of the net displacement, in counts.
pub fn net_counts(dx: i64, dy: i64) -> f64 {
    (dx as f64).hypot(dy as f64)
}

/// Counts per inch for `counts` over `distance_mm`.
pub fn dpi_from_counts(counts: f64, distance_mm: f64) -> f64 {
    counts / (distance_mm / MM_PER_INCH)
}

/// Verdict for an average deviation (percent) measured over `distance_mm`.
pub fn verdict(deviation_pct: f64, distance_mm: f64) -> (TestOutcome, String) {
    let placement_pct = PLACEMENT_ERROR_MM / distance_mm * 100.0;
    if deviation_pct < ACCURATE_PCT {
        (
            TestOutcome::NoIssues,
            format!("✓ DPI is accurate (within {:.0}% tolerance)", ACCURATE_PCT),
        )
    } else if deviation_pct <= placement_pct {
        (
            TestOutcome::Inconclusive,
            format!(
                "? Off by {:.1}%, but over this distance placing the mouse 1 mm off at each end\n  already changes the result by {:.1}%. Repeat with 10 cm or more before concluding.",
                deviation_pct, placement_pct
            ),
        )
    } else if deviation_pct < MINOR_PCT {
        (
            TestOutcome::PossibleIssue,
            format!(
                "⚠ DPI has minor deviation ({:.0}-{:.0}%)",
                ACCURATE_PCT, MINOR_PCT
            ),
        )
    } else {
        (
            TestOutcome::IssueFound,
            format!("✗ DPI is significantly off (>{:.0}% deviation)", MINOR_PCT),
        )
    }
}

struct DpiSample {
    counts: f64,
    measured_dpi: f64,
    accuracy: f64,
}

impl DpiSample {
    fn new(dx: i64, dy: i64, distance_mm: f64, expected_dpi: f64) -> Option<Self> {
        let counts = net_counts(dx, dy);
        if counts < MIN_SAMPLE_COUNTS {
            return None;
        }
        let measured_dpi = dpi_from_counts(counts, distance_mm);
        Some(Self {
            counts,
            measured_dpi,
            accuracy: measured_dpi / expected_dpi * 100.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_distance_units() {
        let d = |s| parse_distance_mm(s).map(|d| d.mm);
        assert_eq!(d("10cm"), Some(100.0));
        assert_eq!(d(" 10 cm \n"), Some(100.0));
        assert_eq!(d("150mm"), Some(150.0));
        assert_eq!(d("4in"), Some(101.6));
        assert_eq!(d("4 inches"), Some(101.6));
        assert_eq!(d("4\""), Some(101.6));
        assert_eq!(d("12.5CM"), Some(125.0));
    }

    #[test]
    fn test_parse_distance_bare_number_is_cm_and_flagged() {
        assert_eq!(
            parse_distance_mm("10"),
            Some(Distance {
                mm: 100.0,
                unit_given: false
            })
        );
    }

    #[test]
    fn test_parse_distance_rejects_nonsense() {
        assert_eq!(parse_distance_mm(""), None);
        assert_eq!(parse_distance_mm("abc"), None);
        assert_eq!(parse_distance_mm("10 furlongs"), None);
        assert_eq!(parse_distance_mm("0cm"), None);
        assert_eq!(parse_distance_mm("5mm"), None); // too short to be useful
        assert_eq!(parse_distance_mm("5m"), None);
    }

    #[test]
    fn test_parse_dpi() {
        assert_eq!(parse_dpi("1600\n"), Some(1600.0));
        assert_eq!(parse_dpi("0"), None);
        assert_eq!(parse_dpi("-800"), None);
        assert_eq!(parse_dpi("lots"), None);
    }

    #[test]
    fn test_slanted_move_measures_full_distance() {
        // 800 DPI mouse moved 10 cm at about 10 degrees off horizontal:
        // X alone reads 1.5% low; the 2-D distance is exact.
        let counts = 800.0 * 100.0 / MM_PER_INCH;
        let angle = 10f64.to_radians();
        let (dx, dy) = (
            (counts * angle.cos()).round() as i64,
            (counts * angle.sin()).round() as i64,
        );
        let dpi = dpi_from_counts(net_counts(dx, dy), 100.0);
        assert!((dpi - 800.0).abs() < 1.0, "dpi {}", dpi);
        let x_only = dpi_from_counts(dx as f64, 100.0);
        assert!(x_only < 790.0);
    }

    #[test]
    fn test_vertical_move_is_measured() {
        let dpi = dpi_from_counts(net_counts(0, -1600), 2.0 * MM_PER_INCH);
        assert!((dpi - 800.0).abs() < 1e-9);
    }

    #[test]
    fn test_accurate_mouse_with_hand_error_passes() {
        // 1 mm short on a 10 cm move: 1% deviation.
        assert_eq!(verdict(1.0, 100.0).0, TestOutcome::NoIssues);
    }

    #[test]
    fn test_short_distance_deviation_is_not_judged() {
        // 6% off over one inch is within what 2 mm of placement error causes.
        assert_eq!(verdict(6.0, MM_PER_INCH).0, TestOutcome::Inconclusive);
        // Over 10 cm the same deviation is real.
        assert_eq!(verdict(6.0, 100.0).0, TestOutcome::PossibleIssue);
    }

    #[test]
    fn test_wrong_dpi_is_flagged() {
        assert_eq!(verdict(25.0, 100.0).0, TestOutcome::IssueFound);
        assert_eq!(verdict(25.0, MM_PER_INCH).0, TestOutcome::IssueFound);
    }

    #[test]
    fn test_sample_needs_movement() {
        assert!(DpiSample::new(3, 2, 100.0, 800.0).is_none());
        let s = DpiSample::new(3150, 0, 100.0, 800.0).unwrap();
        assert!((s.measured_dpi - 800.1).abs() < 0.1);
        assert!((s.accuracy - 100.0).abs() < 0.1);
    }
}
