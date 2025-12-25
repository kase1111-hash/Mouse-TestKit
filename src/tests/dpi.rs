/// DPI Accuracy Test
/// Verifies mouse DPI matches advertised/configured value

use std::time::{Duration, Instant};
use std::io::{self, Write};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crate::input::{self, MouseEvent};

pub fn run() {
    println!("\n=== DPI Accuracy Test ===");
    println!("This test measures your mouse's actual DPI.");
    println!("\nInstructions:");
    println!("  1. Enter your mouse's configured DPI");
    println!("  2. Move your mouse EXACTLY one inch (2.54 cm) horizontally");
    println!("  3. Use a ruler or mousepad markings for accuracy");
    println!("  4. Press 'q' when done\n");

    // Get expected DPI from user
    print!("Enter your mouse's configured DPI (e.g., 800, 1600): ");
    io::stdout().flush().ok();

    let mut dpi_input = String::new();
    io::stdin().read_line(&mut dpi_input).ok();

    let expected_dpi: f64 = match dpi_input.trim().parse() {
        Ok(d) if d > 0.0 => d,
        _ => {
            println!("Invalid DPI value. Using 800 as default.");
            800.0
        }
    };

    println!("\nExpected DPI: {}", expected_dpi);

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            wait_for_enter();
            return;
        }
    };

    device.grab().ok();

    let mut total_counts_x: i64 = 0;
    let mut total_counts_y: i64 = 0;
    let mut samples: Vec<DpiSample> = Vec::new();
    let mut last_print = Instant::now();
    let mut sample_start = Instant::now();

    crossterm::terminal::enable_raw_mode().ok();

    println!("\nMove mouse horizontally one inch, then press SPACE to record.");
    println!("Press 'r' to reset, 'q' to finish.\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => {
                        total_counts_x = 0;
                        total_counts_y = 0;
                        sample_start = Instant::now();
                        println!("\r\x1B[KReset. Move one inch and press SPACE.");
                    }
                    KeyCode::Char(' ') => {
                        if total_counts_x.abs() > 10 {
                            let measured_dpi = total_counts_x.abs() as f64;
                            let accuracy = (measured_dpi / expected_dpi) * 100.0;

                            samples.push(DpiSample {
                                counts: total_counts_x.abs(),
                                measured_dpi,
                                accuracy,
                            });

                            println!("\r\x1B[KSample {}: {} counts = {:.0} DPI ({:.1}% of expected)",
                                samples.len(), total_counts_x.abs(), measured_dpi, accuracy);

                            total_counts_x = 0;
                            total_counts_y = 0;
                            sample_start = Instant::now();
                        } else {
                            println!("\r\x1B[KNot enough movement detected. Try again.");
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Ok(events) = device.fetch_events() {
            for ev in events {
                if let Some(MouseEvent::Move { dx, dy }) = input::parse_event(&ev) {
                    total_counts_x += dx as i64;
                    total_counts_y += dy as i64;
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(100) {
            print!("\r\x1B[K");
            print!("X: {:+6} counts | Y: {:+6} counts | ", total_counts_x, total_counts_y);
            print!("Samples: {} | Press SPACE to record", samples.len());
            io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    crossterm::terminal::disable_raw_mode().ok();

    println!("\n\n=== DPI Accuracy Test Complete ===\n");

    if !samples.is_empty() {
        let avg_dpi: f64 = samples.iter().map(|s| s.measured_dpi).sum::<f64>() / samples.len() as f64;
        let avg_accuracy: f64 = samples.iter().map(|s| s.accuracy).sum::<f64>() / samples.len() as f64;

        let mut dpis: Vec<f64> = samples.iter().map(|s| s.measured_dpi).collect();
        dpis.sort_by(|a, b| a.partial_cmp(b).unwrap());

        println!("Expected DPI: {:.0}", expected_dpi);
        println!("Samples taken: {}\n", samples.len());

        println!("Results:");
        println!("  Average measured DPI: {:.0}", avg_dpi);
        println!("  DPI range: {:.0} - {:.0}", dpis.first().unwrap(), dpis.last().unwrap());
        println!("  Average accuracy: {:.1}%", avg_accuracy);

        let deviation = ((avg_dpi - expected_dpi) / expected_dpi * 100.0).abs();
        println!("  Deviation from expected: {:.1}%", deviation);

        println!("\nSamples:");
        for (i, sample) in samples.iter().enumerate() {
            println!("  #{}: {} counts = {:.0} DPI ({:.1}%)",
                i + 1, sample.counts, sample.measured_dpi, sample.accuracy);
        }

        if deviation < 5.0 {
            println!("\n✓ DPI is accurate (within 5% tolerance)");
        } else if deviation < 10.0 {
            println!("\n⚠ DPI has minor deviation (5-10%)");
        } else {
            println!("\n✗ DPI is significantly off (>10% deviation)");
        }
    } else {
        println!("No samples recorded.");
    }

    wait_for_enter();
}

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
}

struct DpiSample {
    counts: i64,
    measured_dpi: f64,
    accuracy: f64,
}
