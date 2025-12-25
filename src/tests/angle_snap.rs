/// Angle Snapping Detection
/// Detects if mouse has angle snapping/prediction enabled

use std::time::{Duration, Instant};
use std::io::{self, Write};
use crossterm::event::{self, Event, KeyCode};
use crate::input::{self, MouseEvent};

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
            wait_for_enter();
            return;
        }
    };

    device.grab().ok();

    let mut movements: Vec<(i32, i32)> = Vec::new();
    let mut last_print = Instant::now();
    let mut analysis_results: Vec<LineAnalysis> = Vec::new();

    crossterm::terminal::enable_raw_mode().ok();

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
                if let Some(MouseEvent::Move { dx, dy }) = input::parse_event(&ev) {
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

    crossterm::terminal::disable_raw_mode().ok();

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

    wait_for_enter();
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

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
}

struct LineAnalysis {
    straightness: f64,
    average_angle: f64,
    angle_variance: f64,
    has_snapping: bool,
}
