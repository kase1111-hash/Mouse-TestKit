//! Stutter Detection Test
//! Detects mouse movement stutter and irregularities
//!
//! Only intervals during fast, steady movement are judged (see
//! `mouse_testkit::analysis::stutter::StutterDetector`): slow movement,
//! reversals and pauses legitimately leave gaps between reports.

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::stutter::{
    StutterDetector, StutterRating, DEFAULT_STUTTER_MULTIPLIER, MIN_JUDGED_SECS, MIN_STUTTER_GAP_MS,
};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

/// Judged intervals kept for the live graph.
const GRAPH_SAMPLES: usize = 60;

/// Returns false when no mouse was selected, so the test suite does not
/// count the test as run.
pub fn run() -> bool {
    println!("\n=== Stutter Detection Test ===");
    println!("Move your mouse quickly and continuously in circles.");
    println!("Only fast, steady movement is judged; slow movement, direction");
    println!("reversals and pauses are ignored because a mouse legitimately");
    println!("sends fewer reports then. At 4000-8000 Hz use 1600+ DPI or move faster.");
    println!("Press 'q' to quit.\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return false;
        }
    };

    terminal::grab_device(&mut device);

    let mut detector = StutterDetector::new(DEFAULT_STUTTER_MULTIPLIER);
    let mut recent: VecDeque<f64> = VecDeque::with_capacity(GRAPH_SAMPLES + 1);
    let epoch = Instant::now();
    let mut last_print = Instant::now();

    let _guard = terminal::TerminalGuard::new();

    println!("\nMonitoring... (press 'q' to quit)\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            })) = event::read()
            {
                break;
            }
        }

        // One Move per hardware report, stamped when the device sent it
        for timed in input::fetch_timed_events(&mut device) {
            if let MouseEvent::Move { dx, dy } = timed.event {
                let t = timed.time.saturating_duration_since(epoch).as_secs_f64();
                if let Some(interval) = detector.feed(t, dx, dy) {
                    recent.push_back(interval);
                    if recent.len() > GRAPH_SAMPLES {
                        recent.pop_front();
                    }
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(200) {
            // Raw mode: every line must end in "\r\n" (see raw_lines)
            let frame = format!(
                "=== Stutter Detection ===\n\n{}\nPress 'q' to quit\n",
                live_text(&detector, &recent)
            );
            print!("\x1B[2J\x1B[1;1H{}", terminal::raw_lines(&frame));

            use std::io::Write;
            std::io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    drop(_guard);

    println!("\n\n=== Stutter Test Complete ===");
    print_summary(&detector);

    terminal::wait_for_enter();
    true
}

/// The live screen below the title.
fn live_text(detector: &StutterDetector, recent: &VecDeque<f64>) -> String {
    match (detector.median_interval_ms(), detector.threshold_ms()) {
        (Some(median), Some(threshold)) => {
            let samples: Vec<f64> = recent.iter().copied().collect();
            format!(
                "Median interval: {:.3} ms (~{:.0} Hz) | Stutter threshold: {:.2} ms\nFast movement judged: {:.1} s ({} intervals) | Stutters: {} | Worst: {:.1} ms\n\n{}\n",
                median,
                1000.0 / median,
                threshold,
                detector.judged_secs(),
                detector.judged(),
                detector.stutter_count(),
                detector.worst_gap_ms(),
                render_stutter_graph(&samples, median, threshold)
            )
        }
        _ => format!(
            "Waiting for fast movement... ({} reports seen, {} fast intervals collected)\nMove the mouse quickly in circles.\n",
            detector.reports(),
            detector.pending()
        ),
    }
}

fn print_summary(detector: &StutterDetector) {
    let rating = detector.rating();
    println!(
        "Fast movement judged: {:.1} s ({} intervals)",
        detector.judged_secs(),
        detector.judged()
    );
    if let Some(hz) = detector.polling_hz() {
        println!("Polling rate during fast movement: ~{:.0} Hz", hz);
    }
    println!("Total stutter events: {}", detector.stutter_count());

    if detector.stutter_count() > 0 {
        let (minor, moderate, severe) = detector.severity_counts();
        println!("  Severe (8+ ms late):  {}", severe);
        println!("  Moderate (4-8 ms):    {}", moderate);
        println!("  Minor (under 4 ms):   {}", minor);
        println!("  Worst gap: {:.1} ms", detector.worst_gap_ms());
        if detector.freezes() > 0 {
            println!(
                "  Freezes (0.1-1 s while moving): {} ({} lost the movement)",
                detector.freezes(),
                detector.dropouts()
            );
        }
    }
    if let Some(b) = detector.batching() {
        println!(
            "Reports arrive in batches of about {:.1} every {:.1} ms (Bluetooth): rate and stutters are per delivery.",
            b.reports_per_event, b.event_period_ms
        );
    }
    if let Some(c) = detector.clipping() {
        println!(
            "Clipped at high speed: {} of {} fast reports held exactly {} counts on one axis, the most the mouse's reports carry. Faster movement is lost or sent late (not a stutter): lower the DPI or raise the polling rate.",
            c.pinned, c.fast_reports, c.limit
        );
    }

    println!("\nRating: {} - {}", rating.label(), rating.message());
    if rating == StutterRating::Inconclusive {
        println!(
            "Need at least {:.0} s of fast movement to rate (got {:.1} s).",
            MIN_JUDGED_SECS,
            detector.judged_secs()
        );
    }
    println!(
        "(A stutter is a gap between reports during fast movement longer than {:.1}x the median interval and longer than {:.1} ms. A freeze is a stutter of 0.1-1 s while moving: the held-back movement then arrives at once, or the reports go on at full speed with it lost (judged at 1 kHz and up, where a hand's slowing down shows); two or more rate the session at least Fair.)",
        DEFAULT_STUTTER_MULTIPLIER, MIN_STUTTER_GAP_MS
    );
}

fn render_stutter_graph(deltas: &[f64], median: f64, threshold: f64) -> String {
    let width = GRAPH_SAMPLES;
    let height = 10;

    if deltas.is_empty() {
        return String::from("No data yet...");
    }

    let max = deltas.iter().cloned().fold(threshold, f64::max);
    let min = 0.0;
    let range = max - min;

    let mut output = String::new();
    output.push_str(&format!(
        "Interval graph, fast movement only (last {} samples)\n",
        deltas.len()
    ));
    output.push_str(&format!("{:5.1}ms ┌", max));
    output.push_str(&"─".repeat(width));
    output.push_str("┐\n");

    for row in (0..height).rev() {
        let level = min + (range * row as f64 / height as f64);
        output.push_str("        │");

        for val in deltas {
            if *val >= level {
                let late = *val - median;
                if *val <= threshold {
                    output.push('░');
                } else if late >= 8.0 {
                    output.push('█');
                } else if late >= 4.0 {
                    output.push('▓');
                } else {
                    output.push('▒');
                }
            } else {
                output.push(' ');
            }
        }
        for _ in 0..(width.saturating_sub(deltas.len())) {
            output.push(' ');
        }
        output.push_str("│\n");
    }

    output.push_str(&format!("{:5.1}ms └", min));
    output.push_str(&"─".repeat(width));
    output.push_str("┘\n");
    output.push_str("      Legend: ░ Normal  ▒ Minor  ▓ Moderate  █ Severe\n");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_screen_has_no_bare_line_feeds() {
        let mut d = StutterDetector::new(DEFAULT_STUTTER_MULTIPLIER);
        for i in 0..200 {
            d.feed(i as f64 / 1000.0, 3, 1);
        }
        let recent: VecDeque<f64> = std::iter::repeat_n(1.0, 30).collect();
        let screen = terminal::raw_lines(&live_text(&d, &recent));
        assert!(screen.contains("Median interval"), "{}", screen);
        assert!(!screen.replace("\r\n", "").contains('\n'), "{}", screen);
    }

    #[test]
    fn graph_marks_only_intervals_over_the_threshold() {
        let rendered = render_stutter_graph(&[1.0, 1.0, 1.1, 6.0, 12.0], 1.0, 2.5);
        let graph = rendered.split("Legend").next().unwrap();
        assert!(graph.contains('░'), "{}", graph);
        assert!(graph.contains('▓'), "{}", graph);
        assert!(graph.contains('█'), "{}", graph);
        assert!(!graph.contains('▒'), "{}", graph);
        assert_eq!(render_stutter_graph(&[], 1.0, 2.5), "No data yet...");
    }
}
