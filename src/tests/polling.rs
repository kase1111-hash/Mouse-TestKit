//! Polling Rate Monitor
//! Displays real-time mouse polling rate in Hz
//!
//! A mouse only sends a report once it has moved a whole count, so slow
//! movement and reversals legitimately skip polls. As in the GUI panel, a
//! rate is only taken from the intervals between reports sent while the
//! mouse moved fast enough to report at every poll
//! (`analysis::polling::rate_hz`); a window without enough of them
//! publishes nothing, so it cannot drag Min/Avg down. Reports delivered in
//! batches (a Bluetooth mouse, `analysis::polling::batched_delivery`) are
//! measured per delivery, and no USB rate is named for them.

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use mouse_testkit::analysis::polling::{
    batched_delivery, rate_hz, saturated_intervals_ms, usb_nominal_rate, Batching, PollingStats,
    MIN_FAST_INTERVALS, USB_MIN_POLLING_HZ,
};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

/// Returns false when no mouse was selected, so the test suite does not
/// count the test as run.
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
    // The latest window's batching, and whether any window was batched (the
    // readings are then deliveries per second)
    let mut batching: Option<Batching> = None;
    let mut ever_batched = false;
    let mut gaps = 0usize;
    let mut device_lost = false;

    let _guard = terminal::TerminalGuard::new();

    println!("\nMonitoring... (press 'q' to quit)\n");

    loop {
        // Check for quit key (the wait happens on the mouse below)
        if event::poll(Duration::ZERO).unwrap_or(false) {
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
        let Ok(batch) = input::read_events(&mut device, terminal::READ_WAIT) else {
            device_lost = true;
            break;
        };
        if batch.lost_data {
            // Reports were dropped: the interval across the hole is not a
            // poll, so the rate window starts again after it
            gaps += 1;
            reports.clear();
        }
        for timed in batch.events {
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
                Some((hz, batched)) => {
                    stats.update(hz);
                    if batched.is_some() {
                        batching = batched;
                        ever_batched = true;
                    }
                    let label = match batched {
                        Some(_) => " (batched)".to_string(),
                        None => nominal_label(hz as f64),
                    };
                    print!("Current: {:4} Hz{} | ", hz, label);
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
            if ever_batched {
                String::new()
            } else {
                nominal_label(stats.avg_hz)
            }
        );
        if let Some(b) = batching {
            println!(
                "Reports arrived in batches, {:.1} per delivery on average, one delivery every\n{:.1} ms, as a Bluetooth mouse sends the reports it has ready once per\nconnection event. The cursor moves once per delivery, so the rates shown are\ndeliveries per second.",
                b.reports_per_event, b.event_period_ms
            );
        }
        if let Some(note) = no_usb_rate_note(stats.avg_hz, ever_batched) {
            println!("{}", note);
        }
    } else {
        println!("No rate measured: move the mouse quickly and continuously.");
    }

    terminal::print_lost_data_note(gaps);
    if device_lost {
        terminal::print_device_lost();
    }
    terminal::wait_for_enter();
    true
}

/// Polling rate of the last second's reports, from fast movement only, and
/// their batching: with batched delivery the rate is deliveries per second.
fn window_rate(reports: &[(Instant, i32, i32)]) -> Option<(u32, Option<Batching>)> {
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
    let intervals = saturated_intervals_ms(&reports);
    let hz = rate_hz(&intervals, MIN_FAST_INTERVALS)?;
    Some((hz, batched_delivery(&intervals)))
}

/// Why no USB rate is named for an average rate, if none is and the
/// batching note (deliveries are no USB rate) does not already say so.
fn no_usb_rate_note(avg_hz: f64, batched: bool) -> Option<&'static str> {
    if avg_hz < USB_MIN_POLLING_HZ as f64 {
        Some("Under 110 Hz no USB rate fits: normal for a Bluetooth mouse (67-133 Hz),\nlow for a USB mouse, which should report 125 Hz or more.")
    } else if batched || usb_nominal_rate(avg_hz).is_some() {
        None
    } else {
        Some("No USB rate fits: a USB mouse measures at or just under its rate, never well\nabove it. About 133 Hz is a Bluetooth mouse on a 7.5 ms connection interval.")
    }
}

/// " (~1000 Hz)" for a rate a USB mouse runs at; nothing for the Bluetooth
/// range, where a USB rate would read as a USB mouse running slow or fast.
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
        assert_eq!(window_rate(&fast), Some((1000, None)));
    }

    #[test]
    fn batched_deliveries_get_no_usb_rate() {
        // A Bluetooth mouse sampling at 250 Hz, served every 8.75 ms: two or
        // three reports per connection event, 0.4 ms apart
        let t0 = Instant::now();
        let mut reports = Vec::new();
        let mut sent = 0;
        for k in 1..=114u64 {
            let made = k * 8750 / 4000;
            for (i, _) in (sent..made).enumerate() {
                let at = t0 + Duration::from_micros(k * 8750 + 400 * i as u64);
                reports.push((at, 6, 2));
            }
            sent = made;
        }
        let (hz, batching) = window_rate(&reports).unwrap();
        assert_eq!(hz, 114);
        let b = batching.expect("batched");
        assert!((b.event_period_ms - 8.75).abs() < 0.1, "{:?}", b);
        // 114 deliveries a second are not a 125 Hz USB mouse
        assert_eq!(no_usb_rate_note(hz as f64, true), None);
        // A USB mouse's window is never batched
        let usb = window((0..900).map(|_| 8000), 3);
        assert_eq!(window_rate(&usb), Some((125, None)));
    }

    #[test]
    fn bluetooth_rate_names_no_usb_rate() {
        assert_eq!(nominal_label(89.0), "");
        assert_eq!(nominal_label(133.0), "");
        assert_eq!(nominal_label(997.0), " (~1000 Hz)");
        assert!(no_usb_rate_note(89.0, false)
            .unwrap()
            .starts_with("Under 110 Hz"));
        assert!(no_usb_rate_note(133.0, false).unwrap().contains("7.5 ms"));
        assert_eq!(no_usb_rate_note(997.0, false), None);
    }
}
