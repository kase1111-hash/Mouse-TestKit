//! Polling Rate Monitor
//! Displays real-time mouse polling rate in Hz

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::polling::PollingStats;
use std::time::{Duration, Instant, SystemTime};

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
    let mut last_event_time: Option<SystemTime> = None;

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

        // Read mouse events
        #[cfg(target_os = "linux")]
        if let Ok(events) = device.fetch_events() {
            for ev in events {
                if let Some(MouseEvent::Move { .. }) = input::parse_event(&ev) {
                    // Use event timestamp to deduplicate X/Y events from same poll
                    let event_time = ev.timestamp();
                    if last_event_time != Some(event_time) {
                        timestamps.push(Instant::now());
                        last_event_time = Some(event_time);
                    }
                }
            }
        }

        #[cfg(target_os = "windows")]
        if let Ok(events) = device.fetch_events() {
            for ev in events {
                if let MouseEvent::Move { .. } = ev {
                    // Windows Raw Input already deduplicates events
                    timestamps.push(Instant::now());
                }
            }
        }

        // Calculate polling rate every 100ms
        if last_print.elapsed() >= Duration::from_millis(100) {
            let now = Instant::now();

            // Keep only timestamps from last second
            timestamps.retain(|t| now.duration_since(*t) < Duration::from_secs(1));

            if timestamps.len() >= 2 {
                let hz = timestamps.len() as u32;
                stats.update(hz);

                // Calculate interval-based Hz
                let mut intervals: Vec<u128> = Vec::new();
                for i in 1..timestamps.len() {
                    let delta = timestamps[i].duration_since(timestamps[i - 1]).as_micros();
                    if delta > 0 {
                        intervals.push(delta);
                    }
                }

                let avg_interval_hz = if !intervals.is_empty() {
                    let avg_interval = intervals.iter().sum::<u128>() / intervals.len() as u128;
                    if avg_interval > 0 {
                        1_000_000 / avg_interval
                    } else {
                        0
                    }
                } else {
                    0
                };

                print!("\r\x1B[K");
                print!("Current: {:4} Hz | ", avg_interval_hz);
                print!("Min: {:4} Hz | ", stats.min_hz);
                print!("Max: {:4} Hz | ", stats.max_hz);
                print!("Avg: {:6.1} Hz | ", stats.avg_hz);
                print!("Samples: {}", stats.samples);

                use std::io::Write;
                std::io::stdout().flush().ok();
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
