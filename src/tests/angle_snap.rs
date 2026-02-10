//! Angle Snapping Detection
//! Detects if mouse has angle snapping/prediction enabled

use std::time::{Duration, Instant};
use std::io::{self, Write};
use crossterm::event::{self, Event, KeyCode};
use crate::terminal;

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

const MIN_SAMPLES: usize = 100;

pub fn run() {
    println!("\n=== Angle Snapping Detection ===");
    println!("This test detects if your mouse has angle snapping enabled.");
    println!("\nInstructions:");
    println!("  1. Draw slow diagonal lines across your mousepad");
    println!("  2. Try to draw at various angles (not perfectly straight)");
    println!("  3. Natural hand movement should have small variations");
    println!("  4. Angle snapping artificially straightens lines\n");

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

    let mut movements: Vec<(i32, i32)> = Vec::new();
    let mut last_print = Instant::now();
    let mut analysis_results: Vec<LineAnalysis> = Vec::new();

    let _guard = terminal::TerminalGuard::new();

    println!("Draw diagonal lines... Press SPACE to analyze, 'q' to finish.\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') => {
                        if movements.len() >= MIN_SAMPLES {
                            let analysis = analyze_line(&movements);
                            println!("\r\x1B[KLine {}: Straightness: {:.1}% | Angle: {:.1}° | Snapping: {}",
                                analysis_results.len() + 1,
                                analysis.straightness * 100.0,
                                analysis.average_angle,
                                if analysis.has_snapping { "DETECTED" } else { "None" }
                            );
                            analysis_results.push(analysis);
                            movements.clear();
                        } else {
                            println!("\r\x1B[KNeed more movement data. Keep drawing...");
                        }
                    }
                    KeyCode::Char('r') => {
                        movements.clear();
                        println!("\r\x1B[KReset. Draw a new line...");
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
                    if dx != 0 || dy != 0 {
                        movements.push((dx, dy));
                    }
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(100) {
            print!("\r\x1B[K");
            print!("Samples: {} | Lines analyzed: {} | ", movements.len(), analysis_results.len());
            print!("Press SPACE to analyze line");
            io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    drop(_guard);

    println!("\n\n=== Angle Snapping Detection Complete ===\n");

    if !analysis_results.is_empty() {
        let avg_straightness: f64 = analysis_results.iter()
            .map(|a| a.straightness).sum::<f64>() / analysis_results.len() as f64;
        let snapping_count = analysis_results.iter().filter(|a| a.has_snapping).count();

        println!("Lines analyzed: {}", analysis_results.len());
        println!("Average straightness: {:.1}%", avg_straightness * 100.0);
        println!("Lines with snapping detected: {}\n", snapping_count);

        println!("Line details:");
        for (i, result) in analysis_results.iter().enumerate() {
            println!("  #{}: {:.1}% straight, {:.1}° avg angle {}",
                i + 1,
                result.straightness * 100.0,
                result.average_angle,
                if result.has_snapping { "[SNAPPING]" } else { "" }
            );
        }

        let snapping_ratio = snapping_count as f64 / analysis_results.len() as f64;

        if snapping_ratio > 0.5 {
            println!("\n⚠ ANGLE SNAPPING LIKELY ENABLED");
            println!("  Your mouse appears to have angle snapping/prediction.");
            println!("  This can affect precision in games and design work.");
            println!("  Check your mouse software to disable it.");
        } else if avg_straightness > 0.95 {
            println!("\n⚠ Lines are unusually straight");
            println!("  This might indicate mild angle snapping.");
        } else {
            println!("\n✓ No angle snapping detected");
            println!("  Your mouse appears to have raw input.");
        }
    } else {
        println!("No lines analyzed.");
    }

    terminal::wait_for_enter();
}

fn analyze_line(movements: &[(i32, i32)]) -> LineAnalysis {
    if movements.len() < 2 {
        return LineAnalysis {
            straightness: 0.0,
            average_angle: 0.0,
            angle_variance: 0.0,
            has_snapping: false,
        };
    }

    // Calculate angles between consecutive movements
    let mut angles: Vec<f64> = Vec::new();

    for (dx, dy) in movements {
        if *dx != 0 || *dy != 0 {
            let angle = (*dy as f64).atan2(*dx as f64).to_degrees();
            angles.push(angle);
        }
    }

    if angles.is_empty() {
        return LineAnalysis {
            straightness: 0.0,
            average_angle: 0.0,
            angle_variance: 0.0,
            has_snapping: false,
        };
    }

    // Calculate average angle
    let avg_angle: f64 = angles.iter().sum::<f64>() / angles.len() as f64;

    // Calculate variance (how much angles deviate from average)
    let variance: f64 = angles.iter()
        .map(|a| (a - avg_angle).powi(2))
        .sum::<f64>() / angles.len() as f64;
    let std_dev = variance.sqrt();

    // Straightness is inverse of variance (normalized)
    // Lower variance = straighter line
    let straightness = 1.0 / (1.0 + std_dev / 10.0);

    // Angle snapping typically shows very low variance (< 5 degrees std dev)
    // and movements tend to snap to common angles (0, 45, 90, etc.)
    let has_snapping = std_dev < 3.0 && angles.len() > 20;

    LineAnalysis {
        straightness,
        average_angle: avg_angle,
        angle_variance: variance,
        has_snapping,
    }
}

#[derive(Debug, PartialEq)]
pub struct LineAnalysis {
    pub straightness: f64,
    pub average_angle: f64,
    pub angle_variance: f64,
    pub has_snapping: bool,
}

/// Exposed for testing
#[allow(dead_code)]
pub fn analyze_line_pub(movements: &[(i32, i32)]) -> LineAnalysis {
    analyze_line(movements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_line_empty_input() {
        let result = analyze_line(&[]);
        assert_eq!(result.straightness, 0.0);
        assert_eq!(result.average_angle, 0.0);
        assert_eq!(result.angle_variance, 0.0);
        assert!(!result.has_snapping);
    }

    #[test]
    fn test_analyze_line_single_movement() {
        let result = analyze_line(&[(1, 0)]);
        // Single movement should not trigger snapping
        assert!(!result.has_snapping);
    }

    #[test]
    fn test_analyze_line_horizontal_movement() {
        // All horizontal movements (angle = 0 degrees)
        let movements: Vec<(i32, i32)> = (0..30).map(|_| (10, 0)).collect();
        let result = analyze_line(&movements);

        // Average angle should be ~0 for horizontal
        assert!(result.average_angle.abs() < 1.0, "Horizontal movement should have ~0 degree angle");
        // Very straight line
        assert!(result.straightness > 0.9, "Horizontal line should be very straight");
    }

    #[test]
    fn test_analyze_line_vertical_movement() {
        // All vertical movements (angle = 90 degrees)
        let movements: Vec<(i32, i32)> = (0..30).map(|_| (0, 10)).collect();
        let result = analyze_line(&movements);

        // Average angle should be ~90 for vertical
        assert!((result.average_angle - 90.0).abs() < 1.0, "Vertical movement should have ~90 degree angle");
    }

    #[test]
    fn test_analyze_line_diagonal_45_degrees() {
        // 45 degree diagonal (equal dx and dy)
        let movements: Vec<(i32, i32)> = (0..30).map(|_| (10, 10)).collect();
        let result = analyze_line(&movements);

        // Average angle should be ~45 degrees
        assert!((result.average_angle - 45.0).abs() < 1.0, "45 degree diagonal should have ~45 degree angle");
    }

    #[test]
    fn test_analyze_line_detects_snapping() {
        // Very consistent movements with no variation (simulating angle snapping)
        let movements: Vec<(i32, i32)> = (0..30).map(|_| (10, 10)).collect();
        let result = analyze_line(&movements);

        // Perfectly consistent movements should trigger snapping detection
        // (std_dev < 3.0 and len > 20)
        assert!(result.has_snapping, "Perfectly consistent diagonal should detect snapping");
    }

    #[test]
    fn test_analyze_line_no_snapping_with_variation() {
        // Movements with natural hand variation
        let movements: Vec<(i32, i32)> = vec![
            (10, 9), (9, 11), (11, 10), (10, 8), (8, 12),
            (12, 10), (10, 11), (11, 9), (9, 10), (10, 10),
            (10, 13), (13, 7), (7, 10), (10, 12), (12, 8),
            (8, 11), (11, 9), (9, 14), (14, 6), (6, 10),
            (10, 11), (11, 9), (9, 10), (10, 12), (12, 8),
        ];
        let result = analyze_line(&movements);

        // Variable movements should NOT trigger snapping
        assert!(!result.has_snapping, "Variable movements should not detect snapping");
    }

    #[test]
    fn test_analyze_line_zero_movements_filtered() {
        // Zero movements should be ignored in angle calculation
        let movements = vec![(0, 0), (0, 0), (10, 0), (10, 0)];
        let result = analyze_line(&movements);

        // Should only consider the non-zero movements
        assert!(result.average_angle.abs() < 1.0);
    }

    #[test]
    fn test_analyze_line_straightness_calculation() {
        // Perfect line should have high straightness
        let perfect_line: Vec<(i32, i32)> = (0..10).map(|_| (10, 10)).collect();
        let result_perfect = analyze_line(&perfect_line);

        // Variable line should have lower straightness
        let variable_line: Vec<(i32, i32)> = vec![
            (10, 0), (0, 10), (10, 10), (-10, 10), (5, -5),
            (10, 0), (0, 10), (10, 10), (-10, 10), (5, -5),
        ];
        let result_variable = analyze_line(&variable_line);

        assert!(result_perfect.straightness > result_variable.straightness,
            "Perfect line should be straighter than variable line");
    }

    #[test]
    fn test_analyze_line_negative_movements() {
        // Negative movements (moving left/up)
        let movements: Vec<(i32, i32)> = (0..30).map(|_| (-10, -10)).collect();
        let result = analyze_line(&movements);

        // Should calculate angle correctly for negative quadrant
        // -10, -10 => atan2(-10, -10) = -135 degrees
        assert!((result.average_angle - (-135.0)).abs() < 1.0);
    }
}
