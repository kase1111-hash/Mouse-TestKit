//! Jitter Test
//! Measures micro-movements when mouse is stationary
//!
//! As in the GUI panel, recording starts a moment after SPACE so the hand
//! and the key press do not count. As in Auto Diagnostics, movement confined
//! to one or two seconds of a sample (a bump of the desk, the hand leaving
//! late) is not sensor noise: such a sample is set aside and taken again.

use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use mouse_testkit::analysis::diagnostics::Config;
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

const SAMPLE_DURATION_SECS: u64 = 5;
/// Movement (counts) a sample may hold anywhere before it is judged at all:
/// ordinary sensor noise.
const NOISE_COUNTS: f64 = 3.0;

/// Seconds after SPACE before recording starts (Auto Diagnostics' settle
/// time): pressing the key and the hand leaving the desk do not count.
fn settle() -> Duration {
    Duration::from_secs_f64(Config::default().rest_settle_secs)
}

/// Whether a sample's movement is a disturbance rather than sensor noise:
/// more than noise, but confined to fewer separate seconds than a noisy or
/// drifting sensor fills (`Config::rest_spread_windows`). `moves` are
/// (seconds into the sample, dx, dy).
fn disturbed(moves: &[(f64, i32, i32)]) -> bool {
    let total: f64 = moves
        .iter()
        .map(|&(_, dx, dy)| (dx as f64).hypot(dy as f64))
        .sum();
    let mut seconds: Vec<u64> = moves.iter().map(|&(t, ..)| t.max(0.0) as u64).collect();
    seconds.dedup();
    total >= NOISE_COUNTS && seconds.len() < Config::default().rest_spread_windows
}

/// Returns false when no mouse was measured (none selected, or it sent
/// nothing), so the test suite does not count the test as run.
pub fn run() -> bool {
    println!("\n=== Jitter Test ===");
    println!("This test measures sensor noise when the mouse is stationary.");
    println!("\nInstructions:");
    println!("  1. Place your mouse on a stable surface");
    println!("  2. DO NOT touch or move the mouse");
    println!(
        "  3. Press SPACE to start a {}s sample",
        SAMPLE_DURATION_SECS
    );
    println!("  4. The test will measure any phantom movements\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return false;
        }
    };

    terminal::grab_device(&mut device);

    // A healthy sensor at rest sends nothing, so "no jitter" needs proof
    // that this device is the mouse and is connected.
    if !terminal::confirm_device_moves(
        "\nMove the mouse a little to confirm it is the one being tested...",
        |wait| input::read_events(&mut device, wait),
    ) {
        println!("No movement came from the selected device: it may be the wrong one of");
        println!("several entries, in use by another program, or disconnected.");
        terminal::wait_for_enter();
        return false;
    }
    println!("Now let go of the mouse and leave it still.");

    let mut samples: Vec<JitterSample> = Vec::new();
    let mut disturbed_samples = 0usize;
    let mut current_movements: Vec<(f64, i32, i32)> = Vec::new();
    let mut sampling = false;
    let mut sample_start: Option<Instant> = None;
    let mut last_print = Instant::now();
    let mut device_lost = false;
    let mut gaps = 0usize;

    let _guard = terminal::TerminalGuard::new();

    println!("Press SPACE to start sampling (don't touch mouse!), 'q' to finish.\n");

    loop {
        // The wait happens on the mouse below
        if event::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    _ if key.kind != KeyEventKind::Press => {}
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') if !sampling => {
                        sampling = true;
                        // Recording starts once the hand has left
                        sample_start = Some(Instant::now() + settle());
                        current_movements.clear();
                        println!("\r\x1B[KSampling... DO NOT TOUCH THE MOUSE OR THE DESK!");
                    }
                    _ => {}
                }
            }
        }

        // One Move per hardware report (evdev's separate REL_X/REL_Y events
        // merged), so a diagonal twitch counts as one event, not two.
        let Ok(batch) = input::read_events(&mut device, terminal::READ_WAIT) else {
            // The sample in progress did not see its whole time
            device_lost = true;
            break;
        };
        if batch.lost_data && sampling {
            // Some of the movement is gone: the sample cannot be rated
            sampling = false;
            sample_start = None;
            current_movements.clear();
            gaps += 1;
            println!("\r\x1B[KSample set aside: some movement was lost (the computer fell behind reading the mouse). Press SPACE to take it again.");
        }
        for timed in batch.events {
            if let (MouseEvent::Move { dx, dy }, Some(start)) = (timed.event, sample_start) {
                if sampling && timed.time >= start {
                    let t = timed.time.duration_since(start).as_secs_f64();
                    current_movements.push((t, dx, dy));
                }
            }
        }

        // Check if sample complete
        if sampling {
            if let Some(start) = sample_start {
                if start.elapsed() >= Duration::from_secs(SAMPLE_DURATION_SECS) {
                    let moves: Vec<(i32, i32)> = current_movements
                        .iter()
                        .map(|&(_, dx, dy)| (dx, dy))
                        .collect();
                    let sample = analyze_jitter(&moves);
                    if disturbed(&current_movements) {
                        disturbed_samples += 1;
                        println!(
                            "\r\x1B[KSample set aside: {:.0} counts of movement in one burst (a bump or the hand), not steady sensor noise. Press SPACE to take it again.",
                            sample.total_distance
                        );
                    } else {
                        samples.push(sample.clone());
                        println!("\r\x1B[KSample {}: {} events, {:.2} counts total jitter, {:.3} avg magnitude",
                            samples.len(),
                            sample.event_count,
                            sample.total_distance,
                            sample.avg_magnitude
                        );
                    }

                    sampling = false;
                    sample_start = None;
                    current_movements.clear();
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(200) {
            print!("\r\x1B[K");
            let now = Instant::now();
            if sampling && sample_start.is_some_and(|s| now < s) {
                print!("Letting the hand leave - recording starts in a moment | ");
            } else if sampling {
                let elapsed = sample_start
                    .map(|s| now.saturating_duration_since(s).as_secs())
                    .unwrap_or(0);
                print!(
                    "Sampling: {}s / {}s | Events: {} | ",
                    elapsed,
                    SAMPLE_DURATION_SECS,
                    current_movements.len()
                );
            } else {
                print!("Samples: {} | Press SPACE to sample | ", samples.len());
            }
            io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    drop(_guard);

    println!("\n\n=== Jitter Test Complete ===\n");
    if disturbed_samples > 0 {
        println!(
            "{} sample(s) set aside as disturbed (one burst of movement).\n",
            disturbed_samples
        );
    }

    // Not a single report in any sample: a still sensor, or a mouse that
    // stopped sending input (unplugged, link lost). Only the mouse can say.
    let silent = !samples.is_empty() && samples.iter().all(|s| s.event_count == 0);
    if silent
        && (device_lost
            || !terminal::confirm_device_moves(
                "No movement at all was recorded. Move the mouse a little to confirm it is still connected...",
                |wait| input::read_events(&mut device, wait),
            ))
    {
        println!("The mouse stopped sending input, so the samples measured nothing.");
        println!("Check the connection and run the test again.");
    } else if !samples.is_empty() {
        let avg_events: f64 =
            samples.iter().map(|s| s.event_count as f64).sum::<f64>() / samples.len() as f64;
        let avg_distance: f64 =
            samples.iter().map(|s| s.total_distance).sum::<f64>() / samples.len() as f64;
        let avg_magnitude: f64 =
            samples.iter().map(|s| s.avg_magnitude).sum::<f64>() / samples.len() as f64;
        let max_single: f64 = samples
            .iter()
            .map(|s| s.max_single_move)
            .fold(0.0, f64::max);

        println!("Samples taken: {}", samples.len());
        println!("Sample duration: {}s each\n", SAMPLE_DURATION_SECS);

        println!("Results:");
        println!("  Average jitter events:    {:.1} per sample", avg_events);
        println!("  Average total distance:   {:.2} counts", avg_distance);
        println!("  Average event magnitude:  {:.3} counts", avg_magnitude);
        println!("  Maximum single movement:  {:.1} counts", max_single);

        println!("\nSample details:");
        for (i, sample) in samples.iter().enumerate() {
            println!(
                "  #{}: {} events, {:.2} total, max {:.1}",
                i + 1,
                sample.event_count,
                sample.total_distance,
                sample.max_single_move
            );
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

    terminal::print_lost_data_note(gaps);
    if device_lost {
        terminal::print_device_lost();
    }
    terminal::wait_for_enter();
    true
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
    fn one_burst_is_a_disturbance_not_jitter() {
        // The desk is bumped once: 25 reports, 40 counts, within a second
        let bump: Vec<(f64, i32, i32)> = (0..25).map(|i| (1.2 + 0.01 * i as f64, 1, 1)).collect();
        assert!(disturbed(&bump));
        // A noisy sensor reports in most seconds of the sample
        let noise: Vec<(f64, i32, i32)> = (0..25).map(|i| (0.2 * i as f64, 1, 1)).collect();
        assert!(!disturbed(&noise));
        // A count or two of noise is not judged either way
        assert!(!disturbed(&[(0.5, 1, 0), (0.6, 1, 0)]));
        assert!(!disturbed(&[]));
    }

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
