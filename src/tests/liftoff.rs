//! Lift-Off Jump Test
//! Tests for cursor jump during mouse lift
//!
//! The user lifts the mouse straight up and sets it down several times,
//! pressing SPACE after each lift. Motion is split into bursts and judged by
//! `mouse_testkit::analysis::liftoff` (the same rules the GUI panel and Auto
//! Diagnostics use): motion in the first 100 ms after stillness that carries
//! the cursor more than 2 mm is a jump, converted with the mouse's DPI;
//! longer sliding movements are not judged. A jump-sized movement that sped
//! up and slowed down smoothly is the hand and is not judged either; one
//! that could be either is only a possible jump.
//!
//! A clean lift produces no motion at all, so without enough recorded lifts
//! "no jumps" is not a verdict.

use super::standard::TestOutcome;
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use mouse_testkit::analysis::liftoff::{
    counts_to_mm, mm_to_counts, Burst, BurstKind, LiftDetector, LiftTally, LiftVerdict,
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
    let (dpi, dpi_assumed) = match super::dpi::parse_dpi(&dpi_input) {
        Some(d) => (d, false),
        None => {
            println!(
                "Assuming {:.0} DPI. If your mouse is set differently, the distances below",
                DEFAULT_DPI
            );
            println!("are off by the same factor.");
            (DEFAULT_DPI, true)
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

    // A clean lift sends nothing, so "no jumps" needs proof that this device
    // is the mouse and is connected. This also drops input queued while the
    // prompts waited.
    if !terminal::confirm_device_moves(
        "\nMove the mouse a little to confirm it is the one being tested...",
        || input::fetch_timed_events(&mut device),
    ) {
        println!("No movement came from the selected device: it may be the wrong one of");
        println!("several entries, in use by another program, or disconnected.");
        terminal::wait_for_enter();
        return TestOutcome::NoDevice;
    }
    println!("Now rest it on the pad and keep it still before the first lift.");

    let start = Instant::now();
    let secs = |t: Instant| t.saturating_duration_since(start).as_secs_f64();
    // The hand may still be settling the mouse after the nudge: motion is
    // only judged once the mouse has been seen still.
    let mut detector = LiftDetector::new(dpi).watching_from(0.0);
    let mut tally = LiftTally::default();
    let mut lifts = 0usize;
    let mut last_print = Instant::now();

    let _guard = terminal::TerminalGuard::new();

    println!("\nMonitoring... (SPACE after each lift, 'q' to finish)\n");

    let handle = |burst: Burst, tally: &mut LiftTally| match tally.record(&burst, dpi) {
        BurstKind::Jump => println!(
            "\r\x1B[K⚠ JUMP: {:.1} mm (dx {:+}, dy {:+} counts) in {:.0} ms, too abrupt for a hand",
            counts_to_mm(burst.peak_counts, dpi),
            burst.peak_dx,
            burst.peak_dy,
            burst.peak_ms
        ),
        BurstKind::PossibleJump => println!(
            "\r\x1B[K? Possible jump: {:.1} mm (dx {:+}, dy {:+} counts) in {:.0} ms, or the hand",
            counts_to_mm(burst.peak_counts, dpi),
            burst.peak_dx,
            burst.peak_dy,
            burst.peak_ms
        ),
        BurstKind::Nudge => println!(
            "\r\x1B[K  Hand movement ({:.1} mm in {:.0} ms) ignored - keep it still on the pad",
            counts_to_mm(burst.peak_counts, dpi),
            burst.shape.span_ms
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
            let Some(t) = terminal::secs_after(start, timed.time) else {
                continue;
            };
            if let MouseEvent::Move { dx, dy } = timed.event {
                if let Some(burst) = detector.feed(t, dx, dy) {
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
            print!("Status: {}", detector.state().label());
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
        if dpi_assumed { " (assumed)" } else { "" },
        JUMP_THRESHOLD_MM,
        mm_to_counts(JUMP_THRESHOLD_MM, dpi)
    );
    println!("Lifts recorded: {}", lifts);
    println!(
        "Jumps detected: {} (too abrupt for a hand) + {} possible (could also be the hand)",
        tally.jumps.len(),
        tally.possible.len()
    );

    if tally.jump_sized_count() > 0 {
        println!("\nJump statistics (jumps and possible jumps):");
        println!(
            "  Average distance: {:.1} mm",
            counts_to_mm(tally.avg_jump_counts(), dpi)
        );
        println!(
            "  Maximum distance: {:.1} mm",
            counts_to_mm(tally.max_jump_counts(), dpi)
        );

        println!("\nJump events:");
        let events = tally
            .jumps
            .iter()
            .map(|j| (j, "jump"))
            .chain(tally.possible.iter().map(|j| (j, "possible jump")));
        for (i, (jump, kind)) in events.enumerate() {
            println!(
                "  #{}: {:.1} mm (dx: {}, dy: {} counts) - {}",
                i + 1,
                counts_to_mm(jump.peak_counts, dpi),
                jump.peak_dx,
                jump.peak_dy,
                kind
            );
        }
    }

    let (mut outcome, mut lines) = verdict(&tally, lifts, dpi_assumed);
    // Not a single movement since the start: either every lift was perfectly
    // clean, or the mouse stopped sending input (unplugged, link lost).
    if outcome == TestOutcome::NoIssues && tally.bursts() == 0 {
        println!("\nNo movement at all was seen during the test.");
        if !terminal::confirm_device_moves(
            "Move the mouse a little to confirm it is still connected...",
            || input::fetch_timed_events(&mut device),
        ) {
            outcome = TestOutcome::Inconclusive;
            lines = vec![
                "? Inconclusive - the mouse stopped sending input during the test".to_string(),
                "  (unplugged or the connection was lost), so the lifts were not measured."
                    .to_string(),
            ];
        }
    }
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
    if tally.nudges > 0 {
        println!(
            "\n{} movement(s) sped up and slowed down smoothly, as the hand moves the mouse,",
            tally.nudges
        );
        println!("and were not judged.");
    }

    terminal::wait_for_enter();
    outcome
}

/// Overall verdict for the bursts in `tally` over `lifts` recorded lifts,
/// measured at a DPI the user gave or (`dpi_assumed`) at the default.
pub fn verdict(tally: &LiftTally, lifts: usize, dpi_assumed: bool) -> (TestOutcome, Vec<String>) {
    let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
    let jumps = tally.jump_sized_count();
    match tally.verdict(lifts, MIN_LIFTS) {
        // Without the DPI the counts are no distance: the 63 counts that are
        // 2 mm at the assumed 800 DPI are 0.5 mm at 3200 DPI, which an
        // ordinary set-down can shift the mouse (as in Auto Diagnostics).
        LiftVerdict::Jumps if dpi_assumed => (
            TestOutcome::Inconclusive,
            vec![
                format!(
                    "? {} possible jumps, measured at an assumed {:.0} DPI.",
                    jumps, DEFAULT_DPI
                ),
                format!(
                    "  If the mouse is set to {:.0} DPI or lower, the cursor jumps when it is lifted",
                    DEFAULT_DPI
                ),
                "  (lower the lift-off distance); at a higher DPI the same counts are a shorter".to_string(),
                "  distance and may be ordinary set-down shifts. Repeat with the mouse's DPI.".to_string(),
            ],
        ),
        LiftVerdict::Jumps => (
            TestOutcome::IssueFound,
            lines(&[
                "⚠ The cursor jumps when the mouse is lifted or set down, too abruptly for",
                "  the hand to have moved it.",
                "  Likely causes:",
                "  - High lift-off distance (LOD) setting - lower it in the mouse software",
                "  - Sensor tracking issues",
                "  - Surface compatibility problems",
            ]),
        ),
        // Movements a hand could also have made - typically the mouse
        // landing while it still slid sideways - prove neither a fault nor
        // its absence.
        LiftVerdict::PossibleJumps => {
            let mut l = vec![
                format!(
                    "? {} possible jumps: each moved the cursor more than {:.1} mm, but started or",
                    jumps, JUMP_THRESHOLD_MM
                ),
                "  stopped gently enough to be the hand - for example the mouse landing while it"
                    .to_string(),
                "  still slid sideways, or a mouse reporting too rarely to show how it moved."
                    .to_string(),
                "  Set the mouse straight down without sliding it and repeat the test.".to_string(),
            ];
            if dpi_assumed {
                l.push(format!(
                    "  The distances assume {:.0} DPI; at a higher DPI they are shorter.",
                    DEFAULT_DPI
                ));
            }
            (TestOutcome::PossibleIssue, l)
        }
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
    use mouse_testkit::analysis::liftoff::Shape;

    /// A tally with `jumps` jumps no hand could make and `possible` jumps
    /// that could also be the hand.
    fn tally(jumps: usize, possible: usize) -> LiftTally {
        let burst = Burst {
            peak_dx: 100,
            peak_dy: 0,
            peak_counts: 100.0,
            peak_ms: 10.0,
            after_window_counts: 0.0,
            duration_ms: 10.0,
            end_s: 1.0,
            shape: Shape::default(),
        };
        LiftTally {
            jumps: vec![burst.clone(); jumps],
            possible: vec![burst; possible],
            ..LiftTally::default()
        }
    }

    #[test]
    fn test_verdict_needs_lifts_before_calling_it_clean() {
        // Quitting at once, or never pressing SPACE, is not a clean result.
        assert_eq!(verdict(&tally(0, 0), 0, false).0, TestOutcome::Inconclusive);
        assert_eq!(
            verdict(&tally(0, 0), MIN_LIFTS - 1, false).0,
            TestOutcome::Inconclusive
        );
        assert_eq!(
            verdict(&tally(0, 0), MIN_LIFTS, false).0,
            TestOutcome::NoIssues
        );
        // Jumps are evidence whether or not lifts were counted.
        assert_eq!(
            verdict(&tally(1, 0), 0, false).0,
            TestOutcome::PossibleIssue
        );
        assert_eq!(verdict(&tally(2, 0), 0, false).0, TestOutcome::IssueFound);
        assert_eq!(verdict(&tally(3, 0), 8, false).0, TestOutcome::IssueFound);
    }

    #[test]
    fn test_jumps_the_hand_could_have_made_are_only_possible() {
        // Five careless set-downs that slid the mouse 3 mm: their shape
        // cannot rule out the hand, so they are no proof of a fault.
        let (outcome, lines) = verdict(&tally(0, 5), 5, false);
        assert_eq!(outcome, TestOutcome::PossibleIssue);
        assert!(lines[0].contains("5 possible jumps"), "{:?}", lines);
        assert!(lines.join(" ").contains("without sliding"), "{:?}", lines);
        // One jump that no hand could make does not confirm it either.
        assert_eq!(
            verdict(&tally(1, 1), 5, false).0,
            TestOutcome::PossibleIssue
        );
        assert_eq!(
            verdict(&tally(0, 1), 5, false).0,
            TestOutcome::PossibleIssue
        );
        // Two that no hand could make do.
        assert_eq!(verdict(&tally(2, 3), 5, false).0, TestOutcome::IssueFound);
    }

    #[test]
    fn test_jumps_at_an_assumed_dpi_are_not_a_problem_found() {
        // Enter at the DPI prompt, a 3200 DPI mouse: two 0.6 mm set-down
        // shifts read as 2.4 mm jumps at 800 DPI
        let (outcome, lines) = verdict(&tally(2, 0), 6, true);
        assert_eq!(outcome, TestOutcome::Inconclusive);
        assert!(lines.join(" ").contains("assumed 800 DPI"), "{:?}", lines);
        assert_eq!(verdict(&tally(0, 0), 6, true).0, TestOutcome::NoIssues);
        let (outcome, lines) = verdict(&tally(0, 2), 6, true);
        assert_eq!(outcome, TestOutcome::PossibleIssue);
        assert!(lines.join(" ").contains("assume 800 DPI"), "{:?}", lines);
    }
}
