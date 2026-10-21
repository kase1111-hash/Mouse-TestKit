//! Lift-Off Jump Test
//! Tests for cursor jump during mouse lift
//!
//! The user lifts the mouse straight up and sets it down several times,
//! pressing SPACE after each lift. Motion is split into bursts and judged by
//! `mouse_testkit::analysis::liftoff` (the same rules the GUI panel and Auto
//! Diagnostics use): motion in the first 100 ms after stillness that carries
//! the cursor more than 2 mm is a jump, converted with the mouse's DPI;
//! longer sliding movements are not judged.
//!
//! A clean lift produces no motion at all, so without enough recorded lifts
//! "no jumps" is not a verdict.

use super::standard::TestOutcome;
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use mouse_testkit::analysis::liftoff::{
    self, counts_to_mm, mm_to_counts, Burst, BurstKind, LiftDetector, LiftTally, LiftVerdict,
    DEFAULT_DPI, JUMP_THRESHOLD_MM, MIN_LIFTS,
};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

pub fn run() -> TestOutcome {
    println!("\n=== Lift-Off Jump Test ===");
    println!("This test detects cursor jumps when the mouse is lifted or set down.");
    println!("\nInstructions:");
    println!("  1. Rest the mouse on the pad and keep it still");
    println!("  2. Lift it straight up about 1 cm, hold it there a moment,");
    println!("     then set it straight back down - try not to slide it");
    println!("  3. Press SPACE (with your other hand) after each lift and set-down,");
    println!("     so the test knows how many lifts you made");
    println!(
        "  4. Wait about a second and repeat - at least {} times - then press 'q'\n",
        MIN_LIFTS
    );

    print!("Enter your mouse's DPI so jumps can be measured in mm [Enter = 800]: ");
    io::stdout().flush().ok();
    let mut dpi_input = String::new();
    io::stdin().read_line(&mut dpi_input).ok();
    let (dpi, dpi_note) = match super::dpi::parse_dpi(&dpi_input) {
        Some(d) => (d, ""),
        None => {
            println!(
                "Assuming {:.0} DPI. If your mouse is set differently, the distances below",
                DEFAULT_DPI
            );
            println!("are off by the same factor.");
            (DEFAULT_DPI, " (assumed)")
        }
    };
    println!(
        "Jump threshold: {:.1} mm ({:.0} counts at {:.0} DPI)",
        JUMP_THRESHOLD_MM,
        mm_to_counts(JUMP_THRESHOLD_MM, dpi),
        dpi
    );

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return TestOutcome::NoDevice;
        }
    };

    terminal::grab_device(&mut device);

    let start = Instant::now();
    let secs = |t: Instant| t.saturating_duration_since(start).as_secs_f64();
    let mut detector = LiftDetector::new(dpi);
    let mut tally = LiftTally::default();
    let mut lifts = 0usize;
    let mut last_print = Instant::now();

    let _guard = terminal::TerminalGuard::new();

    println!("\nMonitoring... (SPACE after each lift, 'q' to finish)\n");

    let handle = |burst: Burst, tally: &mut LiftTally| match tally.record(&burst, dpi) {
        BurstKind::Jump => println!(
            "\r\x1B[K⚠ JUMP: cursor moved {:.1} mm (dx {:+}, dy {:+} counts) within {:.0} ms",
            counts_to_mm(burst.peak_counts, dpi),
            burst.peak_dx,
            burst.peak_dy,
            burst.peak_ms
        ),
        BurstKind::Slide => println!(
            "\r\x1B[K  Sliding movement ({:.0} ms) ignored - lift straight up for this test",
            burst.duration_ms
        ),
        BurstKind::Quiet => {}
    };

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press,
                ..
            })) = event::read()
            {
                match code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') => {
                        lifts += 1;
                        println!("\r\x1B[K  Lift {} recorded", lifts);
                    }
                    _ => {}
                }
            }
        }

        for timed in input::fetch_timed_events(&mut device) {
            if let MouseEvent::Move { dx, dy } = timed.event {
                if let Some(burst) = detector.feed(secs(timed.time), dx, dy) {
                    handle(burst, &mut tally);
                }
            }
        }
        if let Some(burst) = detector.tick(secs(Instant::now())) {
            handle(burst, &mut tally);
        }

        if last_print.elapsed() >= Duration::from_millis(200) {
            print!("\r\x1B[K");
            print!("Lifts: {} | ", lifts);
            print!("Jumps: {} | ", tally.jumps.len());
            print!(
                "Largest lift/set-down motion: {:.1} mm | ",
                counts_to_mm(tally.largest_counts, dpi)
            );
            print!("Slides ignored: {} | ", tally.slides);
            print!(
                "Status: {}",
                if detector.in_burst() {
                    "moving"
                } else {
                    "still"
                }
            );
            io::stdout().flush().ok();
            last_print = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    if let Some(burst) = detector.flush() {
        handle(burst, &mut tally);
    }

    drop(_guard);

    println!("\n\n=== Lift-Off Jump Test Complete ===\n");
    println!(
        "DPI used: {:.0}{} | jump threshold: {:.1} mm ({:.0} counts)",
        dpi,
        dpi_note,
        JUMP_THRESHOLD_MM,
        mm_to_counts(JUMP_THRESHOLD_MM, dpi)
    );
    println!("Lifts recorded: {}", lifts);
    let jumps = &tally.jumps;
    println!("Total jumps detected: {}", jumps.len());

    if !jumps.is_empty() {
        let dists: Vec<f64> = jumps
            .iter()
            .map(|j| counts_to_mm(j.peak_counts, dpi))
            .collect();
        let avg = dists.iter().sum::<f64>() / dists.len() as f64;
        let max = dists.iter().copied().fold(0.0, f64::max);

        println!("\nJump statistics:");
        println!("  Average distance: {:.1} mm", avg);
        println!("  Maximum distance: {:.1} mm", max);

        println!("\nJump events:");
        for (i, (jump, mm)) in jumps.iter().zip(&dists).enumerate() {
            println!(
                "  #{}: {:.1} mm (dx: {}, dy: {} counts)",
                i + 1,
                mm,
                jump.peak_dx,
                jump.peak_dy
            );
        }
    }

    let (outcome, lines) = verdict(jumps.len(), lifts);
    println!();
    for line in lines {
        println!("{}", line);
    }

    if tally.slides > 0 {
        println!(
            "\n{} longer movement(s) were treated as sliding and not judged. If the mouse",
            tally.slides
        );
        println!("was in the air at those moments, the sensor is tracking while lifted");
        println!("(lift-off distance too high).");
    }

    terminal::wait_for_enter();
    outcome
}

/// Overall verdict for `jumps` found over `lifts` recorded lifts.
pub fn verdict(jumps: usize, lifts: usize) -> (TestOutcome, Vec<String>) {
    let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
    match liftoff::verdict(jumps, lifts, MIN_LIFTS) {
        LiftVerdict::Jumps => (
            TestOutcome::IssueFound,
            lines(&[
                "⚠ The cursor jumps when the mouse is lifted or set down.",
                "  Likely causes:",
                "  - High lift-off distance (LOD) setting - lower it in the mouse software",
                "  - Sensor tracking issues",
                "  - Surface compatibility problems",
            ]),
        ),
        LiftVerdict::OneJump => (
            TestOutcome::PossibleIssue,
            lines(&[
                "⚠ One jump. A single jump is often the hand sliding the mouse on set-down;",
                "  repeat the test - a second jump points to a high lift-off distance.",
            ]),
        ),
        // A clean lift produces no motion, so "no jumps" only means
        // something once the user has actually lifted the mouse.
        LiftVerdict::TooFewLifts => (
            TestOutcome::Inconclusive,
            vec![
                format!(
                    "? Inconclusive - no jumps, but only {} lift(s) were recorded.",
                    lifts
                ),
                format!(
                    "  Lift and set down the mouse at least {} times, pressing SPACE after each.",
                    MIN_LIFTS
                ),
            ],
        ),
        LiftVerdict::Clean => (
            TestOutcome::NoIssues,
            vec![format!(
                "✓ No jumps in {} lifts. Lift-off behavior appears normal.",
                lifts
            )],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdict_needs_lifts_before_calling_it_clean() {
        // Quitting at once, or never pressing SPACE, is not a clean result.
        assert_eq!(verdict(0, 0).0, TestOutcome::Inconclusive);
        assert_eq!(verdict(0, MIN_LIFTS - 1).0, TestOutcome::Inconclusive);
        assert_eq!(verdict(0, MIN_LIFTS).0, TestOutcome::NoIssues);
        // Jumps are evidence whether or not lifts were counted.
        assert_eq!(verdict(1, 0).0, TestOutcome::PossibleIssue);
        assert_eq!(verdict(2, 0).0, TestOutcome::IssueFound);
        assert_eq!(verdict(3, 8).0, TestOutcome::IssueFound);
    }
}
