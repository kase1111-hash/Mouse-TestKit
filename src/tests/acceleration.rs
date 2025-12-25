/// Acceleration Detection
/// Tests for unwanted mouse acceleration curves

use std::time::{Duration, Instant};
use std::io::{self, Write};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crate::input::{self, MouseEvent};

pub fn run() {
    println!("\n=== Acceleration Detection ===");
    println!("This test detects if your mouse has acceleration enabled.");
    println!("\nInstructions:");
    println!("  1. Move mouse SLOWLY across the same distance");
    println!("  2. Press SPACE to record slow movement");
    println!("  3. Move mouse QUICKLY across the same distance");
    println!("  4. Press SPACE to record fast movement");
    println!("  5. Repeat for multiple samples\n");
    println!("With acceleration: fast movement = more counts");
    println!("Without acceleration: same distance = same counts\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            wait_for_enter();
            return;
        }
    };

    device.grab().ok();

    let mut total_counts: i64 = 0;
    let mut total_time: Duration = Duration::ZERO;
    let mut move_start: Option<Instant> = None;
    let mut slow_samples: Vec<AccelSample> = Vec::new();
    let mut fast_samples: Vec<AccelSample> = Vec::new();
    let mut last_print = Instant::now();
    let mut recording_slow = true;

    crossterm::terminal::enable_raw_mode().ok();

    println!("Recording SLOW movements first. Press 's' for slow, 'f' for fast mode.");
    println!("Press SPACE to record, 'r' to reset, 'q' to finish.\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('s') => {
                        recording_slow = true;
                        total_counts = 0;
                        move_start = None;
                        println!("\r\x1B[KMode: SLOW - Move slowly and press SPACE");
                    }
                    KeyCode::Char('f') => {
                        recording_slow = false;
                        total_counts = 0;
                        move_start = None;
                        println!("\r\x1B[KMode: FAST - Move quickly and press SPACE");
                    }
                    KeyCode::Char('r') => {
                        total_counts = 0;
                        move_start = None;
                        println!("\r\x1B[KReset. Move and press SPACE to record.");
                    }
                    KeyCode::Char(' ') => {
                        if total_counts.abs() > 100 {
                            let time = move_start.map(|s| s.elapsed()).unwrap_or(Duration::ZERO);
                            let speed = if time.as_secs_f64() > 0.0 {
                                total_counts.abs() as f64 / time.as_secs_f64()
                            } else {
                                0.0
                            };

                            let sample = AccelSample {
                                counts: total_counts.abs(),
                                time,
                                speed,
                            };

                            if recording_slow {
                                slow_samples.push(sample);
                                println!("\r\x1B[KSlow #{}: {} counts in {:.2}s ({:.0} counts/s)",
                                    slow_samples.len(), total_counts.abs(),
                                    time.as_secs_f64(), speed);
                            } else {
                                fast_samples.push(sample);
                                println!("\r\x1B[KFast #{}: {} counts in {:.2}s ({:.0} counts/s)",
                                    fast_samples.len(), total_counts.abs(),
                                    time.as_secs_f64(), speed);
                            }

                            total_counts = 0;
                            move_start = None;
                        } else {
                            println!("\r\x1B[KNot enough movement. Try again.");
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Ok(events) = device.fetch_events() {
            for ev in events {
                if let Some(MouseEvent::Move { dx, .. }) = input::parse_event(&ev) {
                    if dx != 0 {
                        if move_start.is_none() {
                            move_start = Some(Instant::now());
                        }
                        total_counts += dx.abs() as i64;
                    }
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(100) {
            print!("\r\x1B[K");
            print!("[{}] Counts: {} | Slow: {} | Fast: {} | ",
                if recording_slow { "SLOW" } else { "FAST" },
                total_counts,
                slow_samples.len(),
                fast_samples.len()
            );
            io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    crossterm::terminal::disable_raw_mode().ok();

    println!("\n\n=== Acceleration Detection Complete ===\n");

    if !slow_samples.is_empty() && !fast_samples.is_empty() {
        let avg_slow_counts: f64 = slow_samples.iter().map(|s| s.counts as f64).sum::<f64>()
            / slow_samples.len() as f64;
        let avg_fast_counts: f64 = fast_samples.iter().map(|s| s.counts as f64).sum::<f64>()
            / fast_samples.len() as f64;

        let avg_slow_speed: f64 = slow_samples.iter().map(|s| s.speed).sum::<f64>()
            / slow_samples.len() as f64;
        let avg_fast_speed: f64 = fast_samples.iter().map(|s| s.speed).sum::<f64>()
            / fast_samples.len() as f64;

        // Calculate acceleration ratio
        // If acceleration exists, fast counts will be higher than slow counts
        // for the same physical distance
        let count_ratio = avg_fast_counts / avg_slow_counts;
        let speed_ratio = avg_fast_speed / avg_slow_speed;

        println!("Slow movements: {} samples", slow_samples.len());
        println!("  Average counts: {:.0}", avg_slow_counts);
        println!("  Average speed: {:.0} counts/s\n", avg_slow_speed);

        println!("Fast movements: {} samples", fast_samples.len());
        println!("  Average counts: {:.0}", avg_fast_counts);
        println!("  Average speed: {:.0} counts/s\n", avg_fast_speed);

        println!("Analysis:");
        println!("  Count ratio (fast/slow): {:.2}", count_ratio);
        println!("  Speed ratio (fast/slow): {:.2}", speed_ratio);

        // Acceleration coefficient
        // With perfect 1:1, count_ratio should be ~1.0 regardless of speed
        let accel_coefficient = (count_ratio - 1.0) / (speed_ratio - 1.0).max(0.1);

        if count_ratio > 1.2 {
            println!("\n⚠ ACCELERATION DETECTED");
            println!("  Fast movements register {:.0}% more counts.", (count_ratio - 1.0) * 100.0);
            println!("  Acceleration coefficient: {:.2}", accel_coefficient);
            println!("  Check OS settings or mouse software to disable.");
        } else if count_ratio > 1.1 {
            println!("\n⚠ Mild acceleration detected");
            println!("  Fast movements register {:.0}% more counts.", (count_ratio - 1.0) * 100.0);
        } else if count_ratio < 0.9 {
            println!("\n⚠ Negative acceleration (deceleration) detected");
            println!("  Fast movements register {:.0}% fewer counts.", (1.0 - count_ratio) * 100.0);
        } else {
            println!("\n✓ No significant acceleration detected");
            println!("  Your mouse appears to have 1:1 input.");
        }
    } else {
        println!("Need both slow and fast samples for comparison.");
        println!("Slow samples: {}, Fast samples: {}", slow_samples.len(), fast_samples.len());
    }

    wait_for_enter();
}

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
}

struct AccelSample {
    counts: i64,
    time: Duration,
    speed: f64,
}
