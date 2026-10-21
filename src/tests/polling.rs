//! Polling Rate Monitor
//! Displays real-time mouse polling rate in Hz
//!
//! A mouse only sends a report once it has moved a whole count, so slow
//! movement and reversals legitimately skip polls. As in the GUI panel, a
//! rate is only taken from the intervals between reports sent while the
//! mouse moved fast enough to report at every poll
//! (`analysis::polling::window_hz`); a window without enough of them
//! publishes nothing, so it cannot drag Min/Avg down.

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::polling::{usb_nominal_rate, window_hz, PollingStats};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

/// Returns false when no mouse was measured (none selected, or it sent
/// nothing), so the test suite does not count the test as run.
pub fn run() -> bool {
    println!("\n=== Polling Rate Monitor ===");
    println!("Move your mouse quickly and continuously (e.g. in circles) to measure");
    println!("its polling rate. Slow movement sends fewer reports and is not measured.");
    println!("Press 'q' to quit.\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return false;
        }
    };

    // Set non-blocking and grab device
    terminal::grab_device(&mut device);

    let mut stats = PollingStats::new();
    let mut last_print = Instant::now();
    let mut reports: Vec<(Instant, i32, i32)> = Vec::new();

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
            if let MouseEvent::Move { dx, dy } = timed.event {
                reports.push((timed.time, dx, dy));
            }
        }

        // Calculate polling rate every 100ms
        if last_print.elapsed() >= Duration::from_millis(100) {
            let now = Instant::now();

            // Keep only reports from last second
            reports.retain(|r| now.saturating_duration_since(r.0) < Duration::from_secs(1));

            print!("\r\x1B[K");
            match window_rate(&reports) {
                Some(hz) => {
                    stats.update(hz);
                    print!("Current: {:4} Hz{} | ", hz, nominal_label(hz as f64));
                    print!("Min: {:4} Hz | ", stats.min_hz);
                    print!("Max: {:4} Hz | ", stats.max_hz);
                    print!("Avg: {:6.1} Hz | ", stats.avg_hz);
                    print!("Samples: {}", stats.samples);
                }
                None if !reports.is_empty() => {
                    print!("Current:    - (move faster: too slow to send a report every poll)")
                }
                None => print!("Current:    - (move the mouse)"),
            }
            use std::io::Write;
            std::io::stdout().flush().ok();

            last_print = now;
        }
    }

    drop(_guard);

    println!("\n\nPolling rate test complete.");
    if stats.samples > 0 {
        println!(
            "Final stats - Min: {} Hz, Max: {} Hz, Avg: {:.1} Hz{}",
            stats.min_hz,
            stats.max_hz,
            stats.avg_hz,
            nominal_label(stats.avg_hz)
        );
        if usb_nominal_rate(stats.avg_hz).is_none() {
            println!("Under 110 Hz no USB rate fits: normal for a Bluetooth mouse (67-133 Hz),");
            println!("low for a USB mouse, which should report 125 Hz or more.");
        }
    } else {
        println!("No rate measured: move the mouse quickly and continuously.");
    }

    terminal::wait_for_enter();
    true
}

/// Polling rate of the last second's reports, from fast movement only.
fn window_rate(reports: &[(Instant, i32, i32)]) -> Option<u32> {
    let origin = reports.first()?.0;
    let reports: Vec<(f64, i32, i32)> = reports
        .iter()
        .map(|&(t, dx, dy)| {
            (
                t.saturating_duration_since(origin).as_secs_f64() * 1000.0,
                dx,
                dy,
            )
        })
        .collect();
    window_hz(&reports)
}

/// " (~1000 Hz)" for a rate a USB mouse runs at; nothing for the Bluetooth
/// range, where a USB rate would read as a USB mouse running slow.
fn nominal_label(hz: f64) -> String {
    usb_nominal_rate(hz)
        .map(|n| format!(" (~{} Hz)", n))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(gaps_us: impl Iterator<Item = u64>, dx: i32) -> Vec<(Instant, i32, i32)> {
        let t0 = Instant::now();
        let mut t = 0;
        gaps_us
            .map(|g| {
                t += g;
                (t0 + Duration::from_micros(t), dx, 0)
            })
            .collect()
    }

    #[test]
    fn slow_movement_is_not_read_as_a_low_rate() {
        // A 1 kHz mouse at ordinary hand speed and 800 DPI: one count every
        // 3-8 ms. Timing every report would read about 200 Hz.
        let slow = window((0..200).map(|i| 3000 + 1000 * (i % 6)), 1);
        assert_eq!(window_rate(&slow), None);
        let fast = window((0..900).map(|_| 1000), 3);
        assert_eq!(window_rate(&fast), Some(1000));
    }

    #[test]
    fn bluetooth_rate_names_no_usb_rate() {
        assert_eq!(nominal_label(89.0), "");
        assert_eq!(nominal_label(997.0), " (~1000 Hz)");
    }
}
