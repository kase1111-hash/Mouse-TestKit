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

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
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
        for timed in input::fetch_timed_events(&mut device) {
            engine.feed(secs(timed.time), &timed.event);
        }

        engine.tick(secs(Instant::now()));

        if engine.is_finished() {
            break;
        }

        let phase = engine.current_phase();
        if phase != last_phase {
            if let Some(p) = phase {
                let index = engine.phases().iter().position(|x| *x == p).unwrap_or(0);
                print!("\r\x1B[K");
                println!(
                    "\n>>> Step {}/{}: {}",
                    index + 1,
                    engine.phases().len(),
                    p.title()
                );
                println!("    {}", p.instruction());
            }
            last_phase = phase;
        }

        // Announce new findings as they appear
        let findings = engine.findings_so_far();
        while announced < findings.len() {
            let f = &findings[announced];
            if f.severity != Severity::Info {
                print!("\r\x1B[K");
                println!("    [{}] {}", f.severity.label(), f.title);
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

        std::thread::sleep(Duration::from_millis(1));
    }

    drop(_guard);

    print!("\r\x1B[K");
    match engine.report() {
        Some(report) => print_report(report, engine.phases().len()),
        None => println!("\nNo report produced."),
    }

    terminal::wait_for_enter();
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
        m.scroll_down, m.scroll_up, m.scroll_reversals
    );
    println!(
        "  Lift: {} jump(s), largest {:.0} counts",
        m.lift_jumps, m.lift_max_jump_counts
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_progress_bar_widths() {
        assert_eq!(progress_bar(0.0, 4), "░░░░");
        assert_eq!(progress_bar(0.5, 4), "██░░");
        assert_eq!(progress_bar(1.0, 4), "████");
        assert_eq!(progress_bar(7.0, 4), "████");
    }
}
