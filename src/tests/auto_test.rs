//! Auto Diagnostics (CLI)
//!
//! Guided one-minute check-up that looks for faults by itself. Drives the
//! shared `mouse_testkit::analysis::diagnostics::AutoTest` engine with raw
//! device events and prints the report at the end.
//!
//! Every event is fed with the time the device reported it (kernel timestamp
//! on Linux, raw-input thread time on Windows), not the time the queue was
//! drained: stamping a whole batch with one time would turn the loop period
//! into the "polling rate" and squash the gaps the timing checks look at.

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::diagnostics::{AutoTest, Config, Phase, Report, Severity};
use mouse_testkit::types::InputBatch;
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input;
#[cfg(target_os = "windows")]
use crate::input_windows as input;

/// Seconds from `epoch` to `t`; negative for events stamped before the run
/// started (already queued when the loop began).
fn secs_since(epoch: Instant, t: Instant) -> f64 {
    match t.checked_duration_since(epoch) {
        Some(d) => d.as_secs_f64(),
        None => -epoch.duration_since(t).as_secs_f64(),
    }
}

pub fn run() {
    println!("\n=== Auto Diagnostics ===");
    println!("A guided check-up that looks for faults on its own.");
    println!("It takes about one minute and has these steps:\n");
    let preview = AutoTest::new(Config::default());
    for (i, phase) in preview.phases().iter().enumerate() {
        println!("  {}. {} - {}", i + 1, phase.title(), phase.instruction());
    }
    println!("\nEach step ends by itself. Press 'q' at any time to stop early.\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return;
        }
    };

    terminal::grab_device(&mut device);

    let _guard = terminal::TerminalGuard::new();

    let epoch = Instant::now();
    let secs = |t: Instant| secs_since(epoch, t);

    let mut engine = AutoTest::new(Config::default());
    engine.start(secs(Instant::now()));

    let mut last_phase: Option<Phase> = None;
    let mut last_print = Instant::now();
    let mut announced: usize = 0;
    let mut gaps = 0usize;
    let mut device_lost = false;

    loop {
        // The wait happens on the mouse below
        if event::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            })) = event::read()
            {
                engine.abort();
                break;
            }
        }

        // One move per hardware report (X and Y merged), each with its own
        // device timestamp.
        match input::read_events(&mut device, terminal::READ_WAIT) {
            Ok(batch) => feed_batch(&mut engine, &batch, &secs, &mut gaps),
            Err(_) => {
                // The step in progress did not see everything: the engine
                // says so instead of judging it, and the run stops
                device_lost = true;
                engine.input_lost();
                engine.abort();
                break;
            }
        }

        engine.tick(secs(Instant::now()));

        if engine.is_finished() {
            break;
        }

        let phase = engine.current_phase();
        if phase != last_phase {
            if let Some(p) = phase {
                let index = engine.phases().iter().position(|x| *x == p).unwrap_or(0);
                // Raw mode: "\n" alone would not return to the line start
                print!(
                    "\r\x1B[K{}",
                    terminal::raw_lines(&format!(
                        "\n>>> Step {}/{}: {}\n    {}\n",
                        index + 1,
                        engine.phases().len(),
                        p.title(),
                        p.instruction()
                    ))
                );
            }
            last_phase = phase;
        }

        // Announce new findings as they appear
        let findings = engine.findings_so_far();
        while announced < findings.len() {
            let f = &findings[announced];
            if f.severity != Severity::Info {
                print!("\r\x1B[K    [{}] {}\r\n", f.severity.label(), f.title);
            }
            announced += 1;
        }

        if last_print.elapsed() >= Duration::from_millis(200) {
            if let Some(status) = engine.status() {
                print!("\r\x1B[K");
                print!(
                    "    [{}] {:>3.0}%  {}  ({:.0}s left)",
                    progress_bar(status.progress, 20),
                    status.progress * 100.0,
                    status.detail,
                    status.remaining_secs.ceil()
                );
                io::stdout().flush().ok();
            }
            last_print = Instant::now();
        }
    }

    drop(_guard);

    print!("\r\x1B[K");
    match engine.report() {
        Some(report) => print_report(report, engine.phases().len()),
        None => println!("\nNo report produced."),
    }

    terminal::print_lost_data_note(gaps);
    if device_lost {
        terminal::print_device_lost();
    }
    terminal::wait_for_enter();
}

/// Feed one read of the mouse to the engine; a gap left by dropped reports
/// is passed on first (and counted) so nothing is judged across it.
fn feed_batch(
    engine: &mut AutoTest,
    batch: &InputBatch,
    secs: &impl Fn(Instant) -> f64,
    gaps: &mut usize,
) {
    if batch.lost_data {
        *gaps += 1;
        let resumed = batch.events.first().map_or_else(Instant::now, |e| e.time);
        engine.input_gap(secs(resumed));
    }
    for timed in &batch.events {
        engine.feed(secs(timed.time), &timed.event);
    }
}

fn progress_bar(progress: f64, width: usize) -> String {
    let filled = ((progress.clamp(0.0, 1.0)) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn print_report(report: &Report, total_phases: usize) {
    println!("\n\n╔══════════════════════════════════════════════════════════╗");
    println!("║                 AUTO DIAGNOSTICS REPORT                  ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    println!("Verdict: {}", report.verdict.label().to_uppercase());
    println!(
        "Steps completed: {}/{} in {:.0} s{}",
        report.phases_completed.len(),
        total_phases,
        report.duration_secs,
        if report.aborted {
            " (aborted early)"
        } else {
            ""
        }
    );
    println!(
        "Critical: {}   Warnings: {}   Notes: {}\n",
        report.count(Severity::Critical),
        report.count(Severity::Warning),
        report.count(Severity::Info)
    );

    for (heading, sev) in [
        ("FAULTS", Severity::Critical),
        ("WARNINGS", Severity::Warning),
        ("NOTES", Severity::Info),
    ] {
        let items: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.severity == sev)
            .collect();
        if items.is_empty() {
            continue;
        }
        println!("{}", heading);
        println!("─────────────────────────────────────────────");
        for f in items {
            println!("  • {} [{}]", f.title, f.phase.title());
            println!("    {}", f.detail);
        }
        println!();
    }

    let m = &report.metrics;
    println!("Measurements");
    println!("─────────────────────────────────────────────");
    println!(
        "  Rest: {} movement report(s), {:.1} counts in {} separate second(s), {} click(s), {} scroll event(s)",
        m.rest_move_events,
        m.rest_distance_counts,
        m.rest_motion_seconds,
        m.rest_clicks,
        m.rest_scroll_events
    );
    match m.polling_hz {
        Some(hz) => println!(
            "  Movement: {} Hz{}, {} fast intervals, {} stutter(s) ({:.2}%), worst gap {:.1} ms",
            hz,
            m.polling_nominal_hz
                .map(|n| format!(" (nominal {} Hz)", n))
                .unwrap_or_default(),
            m.move_intervals,
            m.stutter_count,
            m.stutter_rate_percent,
            m.max_interval_ms
        ),
        None => println!(
            "  Movement: {} fast intervals (not enough fast movement for a rate)",
            m.move_intervals
        ),
    }
    println!(
        "  Clicks: left {}, right {}, other {}; bounces {}, micro-presses {}, sticky {}, stuck {}; hold avg {:.0} ms / max {:.0} ms",
        m.left_clicks,
        m.right_clicks,
        m.other_clicks,
        m.bounce_events,
        m.micro_presses,
        m.sticky_releases,
        m.stuck_buttons,
        m.avg_hold_ms,
        m.max_hold_ms
    );
    println!(
        "  Scroll: {} down, {} up, {} reversal(s)",
        m.scroll_down,
        m.scroll_up,
        report
            .scroll_reversals()
            .map_or("not tested".to_string(), |n| n.to_string())
    );
    match report.lift_jumps() {
        Some(jumps) => println!(
            "  Lift: {} jump(s), largest {:.0} counts",
            jumps, m.lift_max_jump_counts
        ),
        None => println!("  Lift: not tested"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mouse_testkit::analysis::diagnostics::Verdict;
    use mouse_testkit::types::{MouseEvent, TimedEvent};

    #[test]
    fn test_secs_since_handles_events_before_the_epoch() {
        let epoch = Instant::now();
        let later = epoch + Duration::from_micros(1_250);
        assert!((secs_since(epoch, later) - 0.00125).abs() < 1e-9);
        if let Some(earlier) = epoch.checked_sub(Duration::from_millis(5)) {
            assert!((secs_since(epoch, earlier) + 0.005).abs() < 1e-9);
        }
        assert_eq!(secs_since(epoch, epoch), 0.0);
    }

    /// Stutters the Movement step counts in 4 s of fast 1000 Hz movement
    /// with the reports of 30 ms missing at 1.5 s, read as two batches; the
    /// second is marked as following dropped reports when `marked`.
    fn movement_with_a_hole(marked: bool) -> (usize, usize) {
        let cfg = Config::default();
        let mut engine = AutoTest::new(cfg.clone());
        let epoch = Instant::now();
        let secs = |t: Instant| secs_since(epoch, t);
        engine.start(0.0);
        let rest_end = cfg.rest_settle_secs + cfg.rest_secs;
        engine.tick(rest_end);
        assert_eq!(engine.current_phase(), Some(Phase::Movement));

        let at = |ms: u64| epoch + Duration::from_secs_f64(rest_end) + Duration::from_millis(ms);
        let report = |ms: u64| TimedEvent {
            time: at(ms),
            event: MouseEvent::Move { dx: 4, dy: 1 },
        };
        let before = InputBatch {
            events: (1..1500).map(report).collect(),
            lost_data: false,
        };
        let after = InputBatch {
            events: (1530..5500).map(report).collect(),
            lost_data: marked,
        };
        let mut gaps = 0;
        feed_batch(&mut engine, &before, &secs, &mut gaps);
        feed_batch(&mut engine, &after, &secs, &mut gaps);
        assert_ne!(engine.current_phase(), Some(Phase::Movement));
        engine.abort();
        let report = engine.report().expect("aborted runs report");
        (report.metrics.stutter_count, gaps)
    }

    #[test]
    fn test_reports_dropped_by_the_system_are_not_a_stutter() {
        assert_eq!(movement_with_a_hole(true), (0, 1));
        // The same hole sent by the mouse itself is still a stutter
        assert_eq!(movement_with_a_hole(false), (1, 0));
    }

    #[test]
    fn test_a_mouse_lost_mid_run_is_never_healthy() {
        // Unplugged during the rest step, which then saw nothing at all
        let mut engine = AutoTest::new(Config::default());
        engine.start(0.0);
        engine.tick(4.0);
        engine.input_lost();
        engine.abort();
        let report = engine.report().unwrap();
        assert_eq!(report.verdict, Verdict::Inconclusive);
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "Input lost during the step"));
    }

    #[test]
    fn test_progress_bar_widths() {
        assert_eq!(progress_bar(0.0, 4), "░░░░");
        assert_eq!(progress_bar(0.5, 4), "██░░");
        assert_eq!(progress_bar(1.0, 4), "████");
        assert_eq!(progress_bar(7.0, 4), "████");
    }
}
