//! Polling Rate Monitor
//! Displays real-time mouse polling rate in Hz

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::polling::{estimate_hz, nominal_rate, PollingStats};
use std::time::{Duration, Instant};

/// Minimum reports in the one-second window before a Hz value is published.
const MIN_EVENTS_FOR_ESTIMATE: usize = 10;

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

pub fn run() {
    println!("\n=== Polling Rate Monitor ===");
    println!("Move your mouse to measure polling rate.");
    println!("Press 'q' to quit.\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return;
        }
    };

    // Set non-blocking and grab device
    terminal::grab_device(&mut device);

    let mut stats = PollingStats::new();
    let mut last_print = Instant::now();
    let mut timestamps: Vec<Instant> = Vec::new();

    let _guard = terminal::TerminalGuard::new();

    println!("\nMonitoring... (press 'q' to quit)\n");

    loop {
        // Check for quit key
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            })) = event::read()
            {
                break;
            }
        }

        // Read mouse events. Each report keeps the time the device sent it;
        // stamping at read time would bunch a whole batch together and turn
        // a 2-8 kHz mouse into millions of Hz.
        for timed in input::fetch_timed_events(&mut device) {
            if let MouseEvent::Move { .. } = timed.event {
                timestamps.push(timed.time);
            }
        }

        // Calculate polling rate every 100ms
        if last_print.elapsed() >= Duration::from_millis(100) {
            let now = Instant::now();

            // Keep only timestamps from last second
            timestamps.retain(|t| now.duration_since(*t) < Duration::from_secs(1));

            if timestamps.len() >= MIN_EVENTS_FOR_ESTIMATE {
                // Median interval between reports: robust against the window
                // being only partly filled or a single missed report.
                let intervals: Vec<f64> = timestamps
                    .windows(2)
                    .map(|w| w[1].duration_since(w[0]).as_secs_f64() * 1000.0)
                    .collect();

                if let Some(hz) = estimate_hz(&intervals, MIN_EVENTS_FOR_ESTIMATE - 1) {
                    stats.update(hz);

                    print!("\r\x1B[K");
                    print!("Current: {:4} Hz (~{} Hz) | ", hz, nominal_rate(hz as f64));
                    print!("Min: {:4} Hz | ", stats.min_hz);
                    print!("Max: {:4} Hz | ", stats.max_hz);
                    print!("Avg: {:6.1} Hz | ", stats.avg_hz);
                    print!("Samples: {}", stats.samples);

                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            }

            last_print = now;
        }
    }

    drop(_guard);

    println!("\n\nPolling rate test complete.");
    if stats.min_hz < u32::MAX {
        println!(
            "Final stats - Min: {} Hz, Max: {} Hz, Avg: {:.1} Hz",
            stats.min_hz, stats.max_hz, stats.avg_hz
        );
    }

    terminal::wait_for_enter();
}

// PollingStats and its unit tests now live in mouse_testkit::analysis::polling
