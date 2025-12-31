//! Click Stickiness Test
//! Tests for stuck or delayed click release

use std::time::{Duration, Instant};
use crossterm::event::{self, Event, KeyCode, KeyEvent};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent, MouseButton};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent, MouseButton};

pub const STICKY_THRESHOLD_MS: f64 = 100.0;

pub fn run() {
    println!("\n=== Click Stickiness Test ===");
    println!("Click and release rapidly to test for stickiness.");
    println!("The test will measure how long each click is held.");
    println!("Press 'q' to quit.\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            wait_for_enter();
            return;
        }
    };

    #[cfg(target_os = "linux")]
    device.grab().ok();

    let mut click_holds: Vec<ClickHold> = Vec::new();
    let mut pending_presses: Vec<(Instant, MouseButton)> = Vec::new();
    let mut last_print = Instant::now();

    crossterm::terminal::enable_raw_mode().ok();

    println!("\nMonitoring... (press 'q' to quit)\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent { code: KeyCode::Char('q'), .. })) = event::read() {
                break;
            }
        }

        if let Ok(events) = device.fetch_events() {
            for ev in events {
                #[cfg(target_os = "linux")]
                let parsed = input::parse_event(&ev);
                #[cfg(target_os = "windows")]
                let parsed = Some(ev);

                match parsed {
                    Some(MouseEvent::ButtonPress(button)) => {
                        pending_presses.push((Instant::now(), button));
                    }
                    Some(MouseEvent::ButtonRelease(button)) => {
                        if let Some(pos) = pending_presses.iter().position(|(_, b)| *b == button) {
                            let (press_time, _) = pending_presses.remove(pos);
                            let duration = press_time.elapsed().as_secs_f64() * 1000.0;
                            let is_sticky = duration > STICKY_THRESHOLD_MS;

                            if is_sticky {
                                println!("\r\x1B[K⚠ STICKY: {:?} held for {:.1}ms", button, duration);
                            }

                            click_holds.push(ClickHold {
                                button: format!("{:?}", button),
                                duration_ms: duration,
                                is_sticky,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(500) {
            print!("\r\x1B[K");
            let total = click_holds.len();
            let sticky_count = click_holds.iter().filter(|h| h.is_sticky).count();

            if total > 0 {
                let avg = click_holds.iter().map(|h| h.duration_ms).sum::<f64>() / total as f64;
                let max = click_holds.iter().map(|h| h.duration_ms).fold(f64::MIN, f64::max);
                print!("Clicks: {} | Sticky: {} | Avg hold: {:.1}ms | Max: {:.1}ms", total, sticky_count, avg, max);
            } else {
                print!("Waiting for clicks... (Press 'q' to quit)");
            }

            use std::io::Write;
            std::io::stdout().flush().ok();
            last_print = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    crossterm::terminal::disable_raw_mode().ok();

    println!("\n\n=== Click Stickiness Test Complete ===\n");

    if !click_holds.is_empty() {
        let total = click_holds.len();
        let sticky_count = click_holds.iter().filter(|h| h.is_sticky).count();
        let avg = click_holds.iter().map(|h| h.duration_ms).sum::<f64>() / total as f64;

        let mut durations: Vec<f64> = click_holds.iter().map(|h| h.duration_ms).collect();
        durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        println!("Total clicks analyzed: {}", total);
        println!("Sticky clicks (>{}ms): {}", STICKY_THRESHOLD_MS, sticky_count);
        println!("\nHold duration statistics:");
        println!("  Average:  {:.1} ms", avg);
        println!("  Minimum:  {:.1} ms", durations.first().unwrap_or(&0.0));
        println!("  Maximum:  {:.1} ms", durations.last().unwrap_or(&0.0));
        if durations.len() >= 2 { println!("  Median:   {:.1} ms", durations[durations.len() / 2]); }

        if sticky_count > 0 {
            println!("\n⚠ Warning: {} sticky clicks detected!", sticky_count);
            println!("  This may indicate worn or faulty switches.");
        } else {
            println!("\n✓ No sticky clicks detected. Switches appear healthy.");
        }
    } else {
        println!("No clicks recorded.");
    }

    wait_for_enter();
}

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}

pub struct ClickHold {
    #[allow(dead_code)]
    pub button: String,
    pub duration_ms: f64,
    pub is_sticky: bool,
}

#[allow(dead_code)]
pub fn is_sticky(duration_ms: f64) -> bool {
    duration_ms > STICKY_THRESHOLD_MS
}
