//! DPI Accuracy Test
//! Verifies mouse DPI matches advertised/configured value
//!
//! The user moves the mouse a measured distance along a ruler. Counting
//! starts and stops on a key press, so positioning the mouse on the start
//! mark is not counted, and the full 2-D displacement is used so a slightly
//! slanted (or vertical) move measures correctly. A longer distance shrinks
//! the effect of placing the mouse a millimetre off at either end.

use super::standard::{self, TestOutcome};
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use mouse_testkit::analysis::dpi::SETUP_ERROR_FRACTION;
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

    // Get expected DPI from user. Without one the DPI is only measured: a
    // guessed value would turn every mouse set differently into a fault.
    let expected_dpi = loop {
        print!("\nEnter your mouse's configured DPI (e.g., 800, 1600) [Enter = just measure it]: ");
        io::stdout().flush().ok();
        let mut dpi_input = String::new();
        let read = io::stdin().read_line(&mut dpi_input).unwrap_or(0);
        if read == 0 || dpi_input.trim().is_empty() {
            println!("No DPI entered: the DPI will be measured, not judged.");
            break None;
        }
        match parse_dpi(&dpi_input) {
            Some(d) => break Some(d),
            None => println!("Could not read that DPI - type a number such as 1600."),
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

    if let Some(dpi) = expected_dpi {
        println!("\nExpected DPI: {}", dpi);
    }
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
    // Some of the sample being counted was lost: it will not be recorded
    let mut spoiled = false;
    let (mut total_x, mut total_y) = (0i64, 0i64);
    let mut last_print = Instant::now();
    let mut device_lost = false;

    let _guard = terminal::TerminalGuard::new();

    let say = |msg: &str| {
        print!("\r\x1B[K{}\r\n", msg);
        io::stdout().flush().ok();
    };
    say("");
    say("Instructions:");
    say("  1. Line up one edge of the mouse with the START mark of your ruler");
    say("     and keep it still");
    say("  2. Press SPACE to start counting");
    say(&format!(
        "  3. Slide it exactly {} along the ruler in one movement, without lifting",
        distance_label
    ));
    say("     (any direction: horizontal, vertical or slanted)");
    say("  4. Stop with the same edge on the END mark, hold still, press SPACE");
    say("  Repeat 3 or more times. 'r' cancels the current sample, 'q' finishes.");
    say("");

    loop {
        let Ok(batch) = input::read_events(&mut device, terminal::READ_WAIT) else {
            if counting {
                say("The mouse stopped responding - this sample was cancelled.");
            }
            device_lost = true;
            break;
        };
        if batch.lost_data && counting && !spoiled {
            // Dropped reports took their counts with them. The user is still
            // sliding, so the SPACE that ends the slide discards the sample
            // (it must not start a new one on the end mark).
            spoiled = true;
            say("Some movement was lost (the computer fell behind reading the mouse), so this");
            say("sample will not be recorded. Finish it and press SPACE, then take it again.");
        }
        for (dx, dy) in moves(batch.events) {
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
                    spoiled = false;
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
                KeyCode::Char(' ') if spoiled => {
                    counting = false;
                    spoiled = false;
                    say("Sample not recorded. Put the mouse on the start mark and press SPACE.");
                }
                KeyCode::Char(' ') => {
                    counting = false;
                    match DpiSample::new(total_x, total_y, distance_mm, expected_dpi) {
                        Some(sample) => {
                            say(&format!(
                                "Sample {}: {:.0} counts (X {:+}, Y {:+}) over {} = {:.0} DPI{}",
                                samples.len() + 1,
                                sample.counts,
                                total_x,
                                total_y,
                                distance_label,
                                sample.measured_dpi,
                                sample.of_expected()
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

        let mut dpis: Vec<f64> = samples.iter().map(|s| s.measured_dpi).collect();
        dpis.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        match expected_dpi {
            Some(dpi) => println!("Expected DPI: {:.0}", dpi),
            None => println!("Expected DPI: not entered (measured only)"),
        }
        println!("Distance used: {}", distance_label);
        println!("Samples taken: {}\n", samples.len());

        // The verdict uses the median: one sample where the mouse was lifted
        // or started off the mark must not drag an accurate result off.
        let typical_dpi = median(&dpis);
        let spread = spread_pct(&dpis);
        let disagree = spread > spread_limit_pct(distance_mm);

        println!("Results:");
        println!("  Median measured DPI:  {:.0}", typical_dpi);
        println!("  Average measured DPI: {:.0}", avg_dpi);
        println!(
            "  DPI range: {:.0} - {:.0}",
            dpis.first().copied().unwrap_or(0.0),
            dpis.last().copied().unwrap_or(0.0)
        );
        let deviation = expected_dpi.map(|expected| {
            let avg_accuracy: f64 = samples
                .iter()
                .map(|s| s.measured_dpi / expected * 100.0)
                .sum::<f64>()
                / samples.len() as f64;
            println!("  Average accuracy: {:.1}%", avg_accuracy);
            let deviation = ((typical_dpi - expected) / expected * 100.0).abs();
            println!("  Deviation of the median from expected: {:.1}%", deviation);
            deviation
        });

        println!("\nSamples:");
        for (i, sample) in samples.iter().enumerate() {
            println!(
                "  #{}: {:.0} counts over {} = {:.0} DPI{}",
                i + 1,
                sample.counts,
                distance_label,
                sample.measured_dpi,
                sample.of_expected()
            );
        }

        let (mut outcome, mut message) = if disagree && samples.len() < 3 {
            (
                TestOutcome::Inconclusive,
                format!(
                    "? The samples differ by {:.0}%, more than hand placement explains - one was\n  probably lifted or started off the mark. Take 3 or more samples.",
                    spread
                ),
            )
        } else if let Some(deviation) = deviation {
            verdict(deviation, distance_mm)
        } else {
            (
                TestOutcome::Measured,
                format!(
                    "Measured DPI: {:.0}. No DPI was entered, so this is not judged: compare it with\n  the DPI set in the mouse's software or on its DPI button.",
                    typical_dpi
                ),
            )
        };
        if device_lost {
            let lines;
            (outcome, lines) = standard::verdict_after_device_lost(outcome, vec![message]);
            message = lines.join("\n");
        }
        println!("\n{}", message);
        if disagree && samples.len() >= 3 {
            println!(
                "Note: the samples differ by {:.0}% - at least one was probably lifted or started",
                spread
            );
            println!("off the mark. The median above is not thrown off by a single odd sample.");
        } else if samples.len() < 3 {
            println!("Take 3 or more samples to average out hand placement.");
        }
        outcome
    } else {
        println!("No samples recorded.");
        TestOutcome::Inconclusive
    };
    let outcome = if device_lost {
        standard::after_device_lost(outcome)
    } else {
        outcome
    };

    if device_lost {
        terminal::print_device_lost();
    }
    terminal::wait_for_enter();
    outcome
}

/// The key just pressed, if any (without waiting: the loop waits on the
/// mouse). Windows also reports key releases; those are skipped, or
/// releasing SPACE would stop the sample it just started.
fn poll_key_press() -> Option<KeyCode> {
    if !event::poll(Duration::ZERO).unwrap_or(false) {
        return None;
    }
    match event::read() {
        Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => Some(key.code),
        _ => None,
    }
}

/// Motion as (dx, dy) per report.
fn moves(events: Vec<input::TimedEvent>) -> Vec<(i32, i32)> {
    events
        .into_iter()
        .filter_map(|timed| match timed.event {
            MouseEvent::Move { dx, dy } => Some((dx, dy)),
            _ => None,
        })
        .collect()
}

/// Parse a DPI value typed by the user, e.g. "1600", "1,600", "1.6k",
/// "1,6k" or "1600 DPI" (CPI is accepted too). Accepts 50 to 100000.
pub fn parse_dpi(input: &str) -> Option<f64> {
    let chars: Vec<char> = input.trim().to_ascii_lowercase().chars().collect();
    // A comma before a group of three digits separates thousands ("1,600");
    // any other comma is a decimal comma ("1,6k" is 1600, not 16000)
    let s: String = chars
        .iter()
        .enumerate()
        .filter_map(|(i, &c)| match c {
            '_' => None,
            ',' => {
                let digits = chars[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .count();
                (digits != 3).then_some('.')
            }
            c => Some(c),
        })
        .collect();
    let number = s
        .strip_suffix("dpi")
        .or_else(|| s.strip_suffix("cpi"))
        .unwrap_or(&s)
        .trim();
    let (number, scale) = match number.strip_suffix('k') {
        Some(n) => (n.trim(), 1000.0),
        None => (number, 1.0),
    };
    let dpi = number.parse::<f64>().ok()? * scale;
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

/// Median of values sorted ascending (0 when empty).
fn median(sorted: &[f64]) -> f64 {
    match sorted.len() {
        0 => 0.0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
    }
}

/// Spread of the measured DPIs (sorted ascending), in percent of the median.
fn spread_pct(sorted: &[f64]) -> f64 {
    match (sorted.first(), sorted.last()) {
        (Some(lo), Some(hi)) if median(sorted) > 0.0 => (hi - lo) / median(sorted) * 100.0,
        _ => 0.0,
    }
}

/// Largest spread between samples that hand placement explains: each sample
/// can be off by the placement error, in opposite directions.
fn spread_limit_pct(distance_mm: f64) -> f64 {
    ACCURATE_PCT.max(2.0 * PLACEMENT_ERROR_MM / distance_mm * 100.0)
}

/// Verdict for a deviation (percent) of the median measured over `distance_mm`.
pub fn verdict(deviation_pct: f64, distance_mm: f64) -> (TestOutcome, String) {
    let placement_pct = PLACEMENT_ERROR_MM / distance_mm * 100.0;
    if deviation_pct > SETUP_ERROR_FRACTION * 100.0 {
        // As in the GUI: that far off is the setup, not the sensor.
        (
            TestOutcome::Inconclusive,
            format!(
                "? Off by {:.0}%: far more than a sensor deviates (a few %). This is almost always\n  the setup: the mouse on a different DPI stage than entered, a wrong distance,\n  or a missed mark. Check them and repeat.",
                deviation_pct
            ),
        )
    } else if deviation_pct < ACCURATE_PCT {
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
    /// Percent of the DPI entered, when one was.
    accuracy: Option<f64>,
}

impl DpiSample {
    fn new(dx: i64, dy: i64, distance_mm: f64, expected_dpi: Option<f64>) -> Option<Self> {
        let counts = net_counts(dx, dy);
        if counts < MIN_SAMPLE_COUNTS {
            return None;
        }
        let measured_dpi = dpi_from_counts(counts, distance_mm);
        Some(Self {
            counts,
            measured_dpi,
            accuracy: expected_dpi.map(|e| measured_dpi / e * 100.0),
        })
    }

    /// " (99.6% of expected)", or nothing when no DPI was entered.
    fn of_expected(&self) -> String {
        self.accuracy
            .map(|a| format!(" ({:.1}% of expected)", a))
            .unwrap_or_default()
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
        // Typing the unit, a thousands separator or "k" is natural.
        assert_eq!(parse_dpi("1600 DPI\n"), Some(1600.0));
        assert_eq!(parse_dpi("1,600"), Some(1600.0));
        assert_eq!(parse_dpi("1.6k"), Some(1600.0));
        assert_eq!(parse_dpi("26K DPI"), Some(26000.0));
        // A decimal comma, as typed in much of Europe
        assert_eq!(parse_dpi("3,2k"), Some(3200.0));
        assert_eq!(parse_dpi("12,000"), Some(12000.0));
        assert_eq!(parse_dpi("1,60"), None);
        assert_eq!(parse_dpi(""), None);
        assert_eq!(parse_dpi("3200dpi"), Some(3200.0));
        assert_eq!(parse_dpi("400 cpi"), Some(400.0));
        assert_eq!(parse_dpi("dpi"), None);
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
    fn test_setup_mistakes_are_not_reported_as_a_fault() {
        // A 1600 DPI mouse measured against 800 (another DPI stage, or the
        // DPI typed differently): 100% off is the setup, not the sensor
        let (outcome, message) = verdict(100.0, 100.0);
        assert_eq!(outcome, TestOutcome::Inconclusive);
        assert!(message.contains("DPI stage"), "{}", message);
        assert_eq!(verdict(50.0, 100.0).0, TestOutcome::Inconclusive);
    }

    #[test]
    fn test_one_lifted_sample_does_not_fail_an_accurate_mouse() {
        // 800 DPI, 10 cm: two good samples and one where the mouse was
        // lifted near the end. The average (742) reads 7% low - "minor
        // deviation" - but the median is within 2%.
        let mut dpis = vec![795.0, 640.0, 790.0];
        dpis.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let avg = dpis.iter().sum::<f64>() / 3.0;
        assert_eq!(
            verdict((avg - 800.0f64).abs() / 8.0, 100.0).0,
            TestOutcome::PossibleIssue
        );
        let typical = median(&dpis);
        assert_eq!(typical, 790.0);
        assert_eq!(
            verdict((typical - 800.0f64).abs() / 8.0, 100.0).0,
            TestOutcome::NoIssues
        );
        assert!(spread_pct(&dpis) > spread_limit_pct(100.0));
    }

    #[test]
    fn test_consistent_samples_do_not_disagree() {
        // 1 mm placement error either way over 10 cm: 2% spread is normal.
        let dpis = [784.0, 792.0, 800.0];
        assert!(spread_pct(&dpis) < spread_limit_pct(100.0));
        // Over one inch, placement alone can spread samples by ~16%.
        let dpis = [740.0, 800.0, 850.0];
        assert!(spread_pct(&dpis) < spread_limit_pct(MM_PER_INCH));
    }

    #[test]
    fn test_sample_needs_movement() {
        assert!(DpiSample::new(3, 2, 100.0, Some(800.0)).is_none());
        let s = DpiSample::new(3150, 0, 100.0, Some(800.0)).unwrap();
        assert!((s.measured_dpi - 800.1).abs() < 0.1);
        assert!((s.accuracy.unwrap() - 100.0).abs() < 0.1);
        assert_eq!(DpiSample::new(3150, 0, 100.0, None).unwrap().accuracy, None);
    }
}
