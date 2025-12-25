/// Button Durability Test
/// Rapid click endurance test for switch reliability

use std::time::{Duration, Instant};
use std::io::{self, Write};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crate::input::{self, MouseEvent, MouseButton};

pub fn run() {
    println!("\n=== Button Durability Test ===");
    println!("This test monitors rapid clicking to assess switch reliability.");
    println!("\nInstructions:");
    println!("  1. Click as fast as you can for extended periods");
    println!("  2. The test tracks click timing consistency");
    println!("  3. Irregularities may indicate switch wear\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            wait_for_enter();
            return;
        }
    };

    device.grab().ok();

    let mut clicks: Vec<ClickRecord> = Vec::new();
    let mut releases: Vec<(Instant, MouseButton)> = Vec::new();
    let mut pending_presses: Vec<(Instant, MouseButton)> = Vec::new();
    let mut test_start: Option<Instant> = None;
    let mut last_print = Instant::now();
    let mut peak_cps: f64 = 0.0;

    crossterm::terminal::enable_raw_mode().ok();

    println!("Start clicking rapidly! Press 'q' to finish.\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent { code: KeyCode::Char('q'), .. })) = event::read() {
                break;
            }
        }

        if let Ok(events) = device.fetch_events() {
            for ev in events {
                match input::parse_event(&ev) {
                    Some(MouseEvent::ButtonPress(button)) => {
                        let now = Instant::now();
                        if test_start.is_none() {
                            test_start = Some(now);
                        }
                        pending_presses.push((now, button));
                    }
                    Some(MouseEvent::ButtonRelease(button)) => {
                        let now = Instant::now();
                        releases.push((now, button));

                        // Match with press
                        if let Some(pos) = pending_presses.iter().position(|(_, b)| *b == button) {
                            let (press_time, _) = pending_presses.remove(pos);
                            let hold_duration = now.duration_since(press_time);

                            clicks.push(ClickRecord {
                                button,
                                press_time,
                                hold_duration,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(200) {
            print!("\r\x1B[K");

            let total_clicks = clicks.len();

            if let Some(start) = test_start {
                let elapsed = start.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    let cps = total_clicks as f64 / elapsed;
                    peak_cps = peak_cps.max(cps);

                    print!("Clicks: {} | Time: {:.1}s | ", total_clicks, elapsed);
                    print!("CPS: {:.1} | Peak: {:.1} | ", cps, peak_cps);
                }
            } else {
                print!("Clicks: {} | Start clicking! | ", total_clicks);
            }

            io::stdout().flush().ok();
            last_print = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    crossterm::terminal::disable_raw_mode().ok();

    println!("\n\n=== Button Durability Test Complete ===\n");

    if clicks.is_empty() {
        println!("No clicks recorded.");
        wait_for_enter();
        return;
    }

    let test_duration = test_start
        .map(|s| s.elapsed().as_secs_f64())
        .unwrap_or(0.0);

    // Separate by button
    let left_clicks: Vec<_> = clicks.iter()
        .filter(|c| c.button == MouseButton::Left)
        .collect();
    let right_clicks: Vec<_> = clicks.iter()
        .filter(|c| c.button == MouseButton::Right)
        .collect();

    println!("Test duration: {:.1}s", test_duration);
    println!("Total clicks: {}", clicks.len());
    println!("Average CPS: {:.1}", clicks.len() as f64 / test_duration);
    println!("Peak CPS: {:.1}\n", peak_cps);

    // Analyze left button
    if !left_clicks.is_empty() {
        println!("Left Button:");
        println!("  Clicks: {}", left_clicks.len());

        let hold_times: Vec<f64> = left_clicks.iter()
            .map(|c| c.hold_duration.as_secs_f64() * 1000.0)
            .collect();

        let avg_hold: f64 = hold_times.iter().sum::<f64>() / hold_times.len() as f64;
        let min_hold = hold_times.iter().cloned().fold(f64::MAX, f64::min);
        let max_hold = hold_times.iter().cloned().fold(f64::MIN, f64::max);

        println!("  Hold time: avg {:.1}ms, min {:.1}ms, max {:.1}ms", avg_hold, min_hold, max_hold);

        // Check for consistency
        let variance: f64 = hold_times.iter()
            .map(|h| (h - avg_hold).powi(2))
            .sum::<f64>() / hold_times.len() as f64;
        let std_dev = variance.sqrt();

        println!("  Consistency (std dev): {:.2}ms", std_dev);

        // Detect anomalies
        let anomalies: Vec<_> = hold_times.iter()
            .filter(|h| (**h - avg_hold).abs() > std_dev * 3.0)
            .collect();

        if !anomalies.is_empty() {
            println!("  ⚠ Anomalies detected: {} clicks with unusual timing", anomalies.len());
        }
    }

    // Analyze right button
    if !right_clicks.is_empty() {
        println!("\nRight Button:");
        println!("  Clicks: {}", right_clicks.len());

        let hold_times: Vec<f64> = right_clicks.iter()
            .map(|c| c.hold_duration.as_secs_f64() * 1000.0)
            .collect();

        let avg_hold: f64 = hold_times.iter().sum::<f64>() / hold_times.len() as f64;
        println!("  Avg hold time: {:.1}ms", avg_hold);
    }

    // Calculate click intervals for left button
    if left_clicks.len() >= 2 {
        println!("\nClick Interval Analysis (Left):");

        let mut intervals: Vec<f64> = Vec::new();
        for i in 1..left_clicks.len() {
            let interval = left_clicks[i].press_time
                .duration_since(left_clicks[i-1].press_time)
                .as_secs_f64() * 1000.0;
            intervals.push(interval);
        }

        let avg_interval: f64 = intervals.iter().sum::<f64>() / intervals.len() as f64;
        let min_interval = intervals.iter().cloned().fold(f64::MAX, f64::min);

        println!("  Average interval: {:.1}ms", avg_interval);
        println!("  Fastest interval: {:.1}ms", min_interval);
        println!("  Theoretical max CPS: {:.1}", 1000.0 / min_interval);
    }

    // Overall assessment
    println!("\n─────────────────────────────────");
    if clicks.len() >= 100 {
        let anomaly_rate = clicks.iter()
            .map(|c| c.hold_duration.as_secs_f64() * 1000.0)
            .filter(|h| *h < 5.0 || *h > 200.0)
            .count() as f64 / clicks.len() as f64;

        if anomaly_rate < 0.01 {
            println!("✓ EXCELLENT - Switches performing consistently");
        } else if anomaly_rate < 0.05 {
            println!("✓ GOOD - Minor timing variations");
        } else {
            println!("⚠ WARNING - Significant timing irregularities");
            println!("  This may indicate switch wear.");
        }
    } else {
        println!("Need more clicks (100+) for reliable analysis.");
    }

    wait_for_enter();
}

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
}

struct ClickRecord {
    button: MouseButton,
    press_time: Instant,
    hold_duration: Duration,
}
