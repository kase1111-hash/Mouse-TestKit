//! Auto Diagnostics (CLI)
//!
//! Guided one-minute check-up that looks for faults by itself. Drives the
//! shared `mouse_testkit::analysis::diagnostics::AutoTest` engine with raw
//! device events and prints the report at the end.

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::diagnostics::{AutoTest, Config, Phase, Report, Severity};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input};
#[cfg(target_os = "linux")]
use mouse_testkit::analysis::coalesce::EventCoalescer;

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
    let secs = |t: Instant| t.duration_since(epoch).as_secs_f64();

    let mut engine = AutoTest::new(Config::default());
    engine.start(secs(Instant::now()));

    #[cfg(target_os = "linux")]
    let mut coalescer = EventCoalescer::new();

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

        #[cfg(target_os = "linux")]
        if let Ok(events) = device.fetch_events() {
            let now = secs(Instant::now());
            for ev in events {
                match input::parse_event(&ev) {
                    Some(MouseEvent::Move { dx, dy }) => {
                        if let Some((fdx, fdy)) = coalescer.accumulate(ev.timestamp(), dx, dy) {
                            engine.feed(now, &MouseEvent::Move { dx: fdx, dy: fdy });
                        }
                    }
                    Some(other) => engine.feed(now, &other),
                    None => {}
                }
            }
            if let Some((dx, dy)) = coalescer.flush() {
                engine.feed(now, &MouseEvent::Move { dx, dy });
            }
        }

        #[cfg(target_os = "windows")]
        if let Ok(events) = device.fetch_events() {
            let now = secs(Instant::now());
            for ev in events {
                engine.feed(now, &ev);
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
        "  Rest: {} movement report(s), {:.1} counts, {} click(s), {} scroll event(s)",
        m.rest_move_events, m.rest_distance_counts, m.rest_clicks, m.rest_scroll_events
    );
    match m.polling_hz {
        Some(hz) => println!(
            "  Movement: {} Hz (nominal {} Hz), {} intervals, {} stutter(s) ({:.2}%), worst gap {:.1} ms",
            hz,
            m.polling_nominal_hz.unwrap_or(hz),
            m.move_intervals,
            m.stutter_count,
            m.stutter_rate_percent,
            m.max_interval_ms
        ),
        None => println!("  Movement: {} intervals (not enough for a rate)", m.move_intervals),
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
    fn test_progress_bar_widths() {
        assert_eq!(progress_bar(0.0, 4), "░░░░");
        assert_eq!(progress_bar(0.5, 4), "██░░");
        assert_eq!(progress_bar(1.0, 4), "████");
        assert_eq!(progress_bar(7.0, 4), "████");
    }
}
