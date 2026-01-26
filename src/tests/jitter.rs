//! Jitter Test
//! Measures micro-movements when mouse is stationary

use std::time::{Duration, Instant};
use std::io::{self, Write};
use crossterm::event::{self, Event, KeyCode};
use crate::terminal;

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

const SAMPLE_DURATION_SECS: u64 = 5;

pub fn run() {
    println!("\n=== Jitter Test ===");
    println!("This test measures sensor noise when the mouse is stationary.");
    println!("\nInstructions:");
    println!("  1. Place your mouse on a stable surface");
    println!("  2. DO NOT touch or move the mouse");
    println!("  3. Press SPACE to start a {}s sample", SAMPLE_DURATION_SECS);
    println!("  4. The test will measure any phantom movements\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return;
        }
    };

    #[cfg(target_os = "linux")]
    device.grab().ok();

    let mut samples: Vec<JitterSample> = Vec::new();
    let mut current_movements: Vec<(i32, i32)> = Vec::new();
    let mut sampling = false;
    let mut sample_start: Option<Instant> = None;
    let mut last_print = Instant::now();

    crossterm::terminal::enable_raw_mode().ok();

    println!("Press SPACE to start sampling (don't touch mouse!), 'q' to finish.\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') if !sampling => {
                        sampling = true;
                        sample_start = Some(Instant::now());
                        current_movements.clear();
                        println!("\r\x1B[KSampling... DO NOT TOUCH THE MOUSE!");
                    }
                    _ => {}
                }
            }
        }

        if let Ok(events) = device.fetch_events() {
            for ev in events {
                #[cfg(target_os = "linux")]
                let parsed = input::parse_event(&ev);
                #[cfg(target_os = "windows")]
                let parsed = Some(ev);

                if let Some(MouseEvent::Move { dx, dy }) = parsed {
                    if sampling {
                        current_movements.push((dx, dy));
                    }
                }
            }
        }

        // Check if sample complete
        if sampling {
            if let Some(start) = sample_start {
                if start.elapsed() >= Duration::from_secs(SAMPLE_DURATION_SECS) {
                    let sample = analyze_jitter(&current_movements);
                    samples.push(sample.clone());

                    println!("\r\x1B[KSample {}: {} events, {:.2} counts total jitter, {:.3} avg magnitude",
                        samples.len(),
                        sample.event_count,
                        sample.total_distance,
                        sample.avg_magnitude
                    );

                    sampling = false;
                    sample_start = None;
                    current_movements.clear();
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(200) {
            print!("\r\x1B[K");
            if sampling {
                let elapsed = sample_start.map(|s| s.elapsed().as_secs()).unwrap_or(0);
                print!("Sampling: {}s / {}s | Events: {} | ",
                    elapsed, SAMPLE_DURATION_SECS, current_movements.len());
            } else {
                print!("Samples: {} | Press SPACE to sample | ", samples.len());
            }
            io::stdout().flush().ok();
            last_print = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    crossterm::terminal::disable_raw_mode().ok();

    println!("\n\n=== Jitter Test Complete ===\n");

    if !samples.is_empty() {
        let avg_events: f64 = samples.iter().map(|s| s.event_count as f64).sum::<f64>()
            / samples.len() as f64;
        let avg_distance: f64 = samples.iter().map(|s| s.total_distance).sum::<f64>()
            / samples.len() as f64;
        let avg_magnitude: f64 = samples.iter().map(|s| s.avg_magnitude).sum::<f64>()
            / samples.len() as f64;
        let max_single: f64 = samples.iter().map(|s| s.max_single_move).fold(0.0, f64::max);

        println!("Samples taken: {}", samples.len());
        println!("Sample duration: {}s each\n", SAMPLE_DURATION_SECS);

        println!("Results:");
        println!("  Average jitter events:    {:.1} per sample", avg_events);
        println!("  Average total distance:   {:.2} counts", avg_distance);
        println!("  Average event magnitude:  {:.3} counts", avg_magnitude);
        println!("  Maximum single movement:  {:.1} counts", max_single);

        println!("\nSample details:");
        for (i, sample) in samples.iter().enumerate() {
            println!("  #{}: {} events, {:.2} total, max {:.1}",
                i + 1, sample.event_count, sample.total_distance, sample.max_single_move);
        }

        // Jitter rating
        if avg_events < 1.0 && avg_distance < 1.0 {
            println!("\n✓ EXCELLENT - Virtually no jitter");
            println!("  Your sensor is extremely stable.");
        } else if avg_events < 5.0 && avg_distance < 5.0 {
            println!("\n✓ GOOD - Minimal jitter");
            println!("  Normal sensor noise, not noticeable in use.");
        } else if avg_events < 20.0 && avg_distance < 20.0 {
            println!("\n⚠ MODERATE - Some jitter detected");
            println!("  May be noticeable at high sensitivity settings.");
        } else {
            println!("\n⚠ HIGH JITTER");
            println!("  Your sensor shows significant noise.");
            println!("  This may affect precision, especially at high DPI.");
            println!("  Try a different mousepad surface.");
        }
    } else {
        println!("No samples recorded.");
    }

    terminal::wait_for_enter();
}

fn analyze_jitter(movements: &[(i32, i32)]) -> JitterSample {
    if movements.is_empty() {
        return JitterSample {
            event_count: 0,
            total_distance: 0.0,
            avg_magnitude: 0.0,
            max_single_move: 0.0,
        };
    }

    let mut total_distance: f64 = 0.0;
    let mut max_single: f64 = 0.0;

    for (dx, dy) in movements {
        let magnitude = ((*dx as f64).powi(2) + (*dy as f64).powi(2)).sqrt();
        total_distance += magnitude;
        max_single = max_single.max(magnitude);
    }

    JitterSample {
        event_count: movements.len(),
        total_distance,
        avg_magnitude: total_distance / movements.len() as f64,
        max_single_move: max_single,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JitterSample {
    pub event_count: usize,
    pub total_distance: f64,
    pub avg_magnitude: f64,
    pub max_single_move: f64,
}

/// Analyze jitter from movement data - exposed for testing
#[allow(dead_code)]
pub fn analyze_jitter_pub(movements: &[(i32, i32)]) -> JitterSample {
    analyze_jitter(movements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_jitter_empty_input() {
        let result = analyze_jitter(&[]);
        assert_eq!(result.event_count, 0);
        assert_eq!(result.total_distance, 0.0);
        assert_eq!(result.avg_magnitude, 0.0);
        assert_eq!(result.max_single_move, 0.0);
    }

    #[test]
    fn test_analyze_jitter_single_movement() {
        let movements = vec![(3, 4)]; // Pythagorean: sqrt(9+16) = 5.0
        let result = analyze_jitter(&movements);

        assert_eq!(result.event_count, 1);
        assert!((result.total_distance - 5.0).abs() < 0.001);
        assert!((result.avg_magnitude - 5.0).abs() < 0.001);
        assert!((result.max_single_move - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_analyze_jitter_multiple_movements() {
        // (3,4) = 5.0, (0,1) = 1.0, (1,0) = 1.0
        let movements = vec![(3, 4), (0, 1), (1, 0)];
        let result = analyze_jitter(&movements);

        assert_eq!(result.event_count, 3);
        assert!((result.total_distance - 7.0).abs() < 0.001); // 5 + 1 + 1
        assert!((result.avg_magnitude - 7.0 / 3.0).abs() < 0.001);
        assert!((result.max_single_move - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_analyze_jitter_zero_movement() {
        let movements = vec![(0, 0), (0, 0)];
        let result = analyze_jitter(&movements);

        assert_eq!(result.event_count, 2);
        assert_eq!(result.total_distance, 0.0);
        assert_eq!(result.avg_magnitude, 0.0);
        assert_eq!(result.max_single_move, 0.0);
    }

    #[test]
    fn test_analyze_jitter_negative_values() {
        // (-3,-4) = 5.0, magnitude should be positive
        let movements = vec![(-3, -4)];
        let result = analyze_jitter(&movements);

        assert!((result.total_distance - 5.0).abs() < 0.001);
        assert!((result.max_single_move - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_analyze_jitter_large_movement() {
        let movements = vec![(100, 100), (1, 1)];
        let result = analyze_jitter(&movements);

        let large_magnitude = (100.0_f64.powi(2) + 100.0_f64.powi(2)).sqrt();
        assert!((result.max_single_move - large_magnitude).abs() < 0.001);
    }

    #[test]
    fn test_analyze_jitter_calculates_correct_average() {
        // Test that average is total_distance / event_count
        let movements = vec![(6, 8), (3, 4)]; // 10.0 + 5.0 = 15.0 total, avg = 7.5
        let result = analyze_jitter(&movements);

        assert_eq!(result.event_count, 2);
        assert!((result.total_distance - 15.0).abs() < 0.001);
        assert!((result.avg_magnitude - 7.5).abs() < 0.001);
    }
}
