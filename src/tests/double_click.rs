/// Double-Click Test
/// Detects faulty switches causing unintended double-clicks

use std::time::{Duration, Instant};
use std::io::{self, Write};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crate::input::{self, MouseEvent, MouseButton};

const DOUBLE_CLICK_THRESHOLD_MS: f64 = 50.0; // Clicks faster than this are suspicious

pub fn run() {
    println!("\n=== Double-Click Detection Test ===");
    println!("This test detects faulty switches causing unintended double-clicks.");
    println!("\nInstructions:");
    println!("  1. Click normally at your regular pace");
    println!("  2. The test will detect any unintended rapid clicks");
    println!("  3. Faulty switches often register multiple clicks from one press\n");
    println!("Threshold: Clicks within {}ms are flagged as suspicious\n", DOUBLE_CLICK_THRESHOLD_MS);

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            wait_for_enter();
            return;
        }
    };

    device.grab().ok();

    let mut clicks: Vec<ClickEvent> = Vec::new();
    let mut double_clicks: Vec<DoubleClickEvent> = Vec::new();
    let mut last_click_time: Option<Instant> = None;
    let mut last_button: Option<MouseButton> = None;
    let mut last_print = Instant::now();

    crossterm::terminal::enable_raw_mode().ok();

    println!("Click at normal pace... Press 'q' to finish.\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent { code: KeyCode::Char('q'), .. })) = event::read() {
                break;
            }
        }

        if let Ok(events) = device.fetch_events() {
            for ev in events {
                if let Some(MouseEvent::ButtonPress(button)) = input::parse_event(&ev) {
                    let now = Instant::now();

                    // Check for double-click
                    if let (Some(last_time), Some(last_btn)) = (last_click_time, last_button) {
                        if button == last_btn {
                            let interval = now.duration_since(last_time).as_secs_f64() * 1000.0;

                            if interval < DOUBLE_CLICK_THRESHOLD_MS {
                                double_clicks.push(DoubleClickEvent {
                                    button,
                                    interval_ms: interval,
                                });
                                println!("\r\x1B[K⚠ DOUBLE-CLICK: {:?} - {:.1}ms interval!",
                                    button, interval);
                            }
                        }
                    }

                    clicks.push(ClickEvent {
                        button,
                        timestamp: now,
                    });

                    last_click_time = Some(now);
                    last_button = Some(button);
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(500) {
            print!("\r\x1B[K");
            print!("Clicks: {} | Double-clicks detected: {} | ",
                clicks.len(), double_clicks.len());
            print!("Press 'q' to finish");
            io::stdout().flush().ok();
            last_print = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    crossterm::terminal::disable_raw_mode().ok();

    println!("\n\n=== Double-Click Test Complete ===\n");

    println!("Total clicks: {}", clicks.len());
    println!("Double-clicks detected: {}\n", double_clicks.len());

    if !double_clicks.is_empty() {
        // Group by button
        let left_doubles: Vec<_> = double_clicks.iter()
            .filter(|d| d.button == MouseButton::Left)
            .collect();
        let right_doubles: Vec<_> = double_clicks.iter()
            .filter(|d| d.button == MouseButton::Right)
            .collect();
        let middle_doubles: Vec<_> = double_clicks.iter()
            .filter(|d| d.button == MouseButton::Middle)
            .collect();

        println!("By button:");
        if !left_doubles.is_empty() {
            let avg: f64 = left_doubles.iter().map(|d| d.interval_ms).sum::<f64>()
                / left_doubles.len() as f64;
            println!("  Left:   {} occurrences (avg {:.1}ms interval)", left_doubles.len(), avg);
        }
        if !right_doubles.is_empty() {
            let avg: f64 = right_doubles.iter().map(|d| d.interval_ms).sum::<f64>()
                / right_doubles.len() as f64;
            println!("  Right:  {} occurrences (avg {:.1}ms interval)", right_doubles.len(), avg);
        }
        if !middle_doubles.is_empty() {
            let avg: f64 = middle_doubles.iter().map(|d| d.interval_ms).sum::<f64>()
                / middle_doubles.len() as f64;
            println!("  Middle: {} occurrences (avg {:.1}ms interval)", middle_doubles.len(), avg);
        }

        // Calculate double-click rate
        let rate = double_clicks.len() as f64 / clicks.len() as f64 * 100.0;
        println!("\nDouble-click rate: {:.1}%", rate);

        if rate > 5.0 {
            println!("\n⚠ HIGH DOUBLE-CLICK RATE");
            println!("  Your mouse switches may be failing.");
            println!("  Consider switch replacement or mouse replacement.");
        } else if rate > 1.0 {
            println!("\n⚠ Occasional double-clicks detected");
            println!("  Monitor for worsening symptoms.");
        }

        println!("\nRecent double-click events:");
        for (i, dc) in double_clicks.iter().rev().take(10).enumerate() {
            println!("  #{}: {:?} - {:.1}ms interval", i + 1, dc.button, dc.interval_ms);
        }
    } else {
        println!("✓ No double-click issues detected");
        println!("  Your mouse switches appear healthy.");
    }

    // Show click interval histogram
    if clicks.len() >= 2 {
        println!("\nClick interval distribution:");
        let mut intervals: Vec<f64> = Vec::new();
        for i in 1..clicks.len() {
            let interval = clicks[i].timestamp
                .duration_since(clicks[i-1].timestamp)
                .as_secs_f64() * 1000.0;
            intervals.push(interval);
        }

        let under_50: usize = intervals.iter().filter(|i| **i < 50.0).count();
        let _50_to_100: usize = intervals.iter().filter(|i| **i >= 50.0 && **i < 100.0).count();
        let _100_to_200: usize = intervals.iter().filter(|i| **i >= 100.0 && **i < 200.0).count();
        let over_200: usize = intervals.iter().filter(|i| **i >= 200.0).count();

        println!("  <50ms:     {} (suspicious)", under_50);
        println!("  50-100ms:  {} (fast clicking)", _50_to_100);
        println!("  100-200ms: {} (normal)", _100_to_200);
        println!("  >200ms:    {} (slow)", over_200);
    }

    wait_for_enter();
}

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
}

struct ClickEvent {
    button: MouseButton,
    timestamp: Instant,
}

struct DoubleClickEvent {
    button: MouseButton,
    interval_ms: f64,
}
