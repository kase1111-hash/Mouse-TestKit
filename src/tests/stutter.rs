//! Stutter Detection Test
//! Detects mouse movement stutter and irregularities

use std::time::{Duration, Instant};
use crossterm::event::{self, Event, KeyCode, KeyEvent};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

const STUTTER_THRESHOLD_MS: f64 = 4.0;

pub fn run() {
    println!("\n=== Stutter Detection Test ===");
    println!("Move your mouse in circles to detect stutter.");
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
    // Windows doesn't require device grab - Raw Input works without exclusive access

    let mut timestamps: Vec<Instant> = Vec::new();
    let mut deltas: Vec<f64> = Vec::new();
    let mut stutter_events: Vec<StutterEvent> = Vec::new();
    let mut last_print = Instant::now();

    crossterm::terminal::enable_raw_mode().ok();

    println!("\nMonitoring... (press 'q' to quit)\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent { code: KeyCode::Char('q'), .. })) = event::read() {
                break;
            }
        }

        #[cfg(target_os = "linux")]
        if let Ok(events) = device.fetch_events() {
            for ev in events {
                if let Some(MouseEvent::Move { .. }) = input::parse_event(&ev) {
                    let now = Instant::now();
                    if let Some(last) = timestamps.last() {
                        let delta = now.duration_since(*last).as_secs_f64() * 1000.0;
                        deltas.push(delta);
                        if deltas.len() > 100 { deltas.remove(0); }
                    }
                    timestamps.push(now);
                    if timestamps.len() > 1000 { timestamps.remove(0); }
                }
            }
        }

        #[cfg(target_os = "windows")]
        if let Ok(events) = device.fetch_events() {
            for ev in events {
                if let MouseEvent::Move { .. } = ev {
                    let now = Instant::now();
                    if let Some(last) = timestamps.last() {
                        let delta = now.duration_since(*last).as_secs_f64() * 1000.0;
                        deltas.push(delta);
                        if deltas.len() > 100 { deltas.remove(0); }
                    }
                    timestamps.push(now);
                    if timestamps.len() > 1000 { timestamps.remove(0); }
                }
            }
        }

        if last_print.elapsed() >= Duration::from_millis(200) && deltas.len() >= 10 {
            let new_stutters = analyze_stutter(&deltas);
            stutter_events.extend(new_stutters.iter().cloned());

            let avg_delta: f64 = deltas.iter().sum::<f64>() / deltas.len() as f64;
            let max_delta = deltas.iter().cloned().fold(f64::MIN, f64::max);
            let min_delta = deltas.iter().cloned().fold(f64::MAX, f64::min);

            let stutter_count = deltas.iter()
                .filter(|d| (**d - avg_delta).abs() > STUTTER_THRESHOLD_MS)
                .count();

            print!("\x1B[2J\x1B[1;1H");
            println!("=== Stutter Detection ===\n");
            println!("Avg interval: {:.2} ms | Min: {:.2} ms | Max: {:.2} ms",
                avg_delta, min_delta, max_delta);
            println!("Stutters detected: {} (threshold: {:.1}ms deviation)\n",
                stutter_count, STUTTER_THRESHOLD_MS);
            println!("{}", render_stutter_graph(&deltas, avg_delta));
            println!("\nTotal stutter events this session: {}", stutter_events.len());
            println!("\nPress 'q' to quit");

            use std::io::Write;
            std::io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    crossterm::terminal::disable_raw_mode().ok();

    println!("\n\n=== Stutter Test Complete ===");
    println!("Total stutter events: {}", stutter_events.len());

    if !stutter_events.is_empty() {
        let severe = stutter_events.iter().filter(|s| matches!(s.severity, StutterSeverity::Severe)).count();
        let moderate = stutter_events.iter().filter(|s| matches!(s.severity, StutterSeverity::Moderate)).count();
        let minor = stutter_events.iter().filter(|s| matches!(s.severity, StutterSeverity::Minor)).count();
        println!("  Severe (8ms+):    {}", severe);
        println!("  Moderate (4-8ms): {}", moderate);
        println!("  Minor (2-4ms):    {}", minor);
    }

    wait_for_enter();
}

fn render_stutter_graph(deltas: &[f64], avg: f64) -> String {
    let width = 60;
    let height = 10;
    let display_deltas: Vec<f64> = if deltas.len() > width {
        deltas[deltas.len() - width..].to_vec()
    } else {
        deltas.to_vec()
    };

    if display_deltas.is_empty() { return String::from("No data yet..."); }

    let max = display_deltas.iter().cloned().fold(f64::MIN, f64::max);
    let min = display_deltas.iter().cloned().fold(f64::MAX, f64::min);
    let range = if (max - min).abs() < 0.001 { 1.0 } else { max - min };

    let mut output = String::new();
    output.push_str(&format!("Delta Time Graph (last {} samples)\n", display_deltas.len()));
    output.push_str(&format!("{:.1}ms ┌", max));
    output.push_str(&"─".repeat(width));
    output.push_str("┐\n");

    for row in (0..height).rev() {
        let threshold = min + (range * row as f64 / height as f64);
        if row == height / 2 {
            output.push_str(&format!("{:.1}ms │", avg));
        } else {
            output.push_str("      │");
        }

        for val in &display_deltas {
            if *val >= threshold {
                let deviation = (*val - avg).abs();
                if deviation > 8.0 { output.push('█'); }
                else if deviation > 4.0 { output.push('▓'); }
                else if deviation > 2.0 { output.push('▒'); }
                else { output.push('░'); }
            } else {
                output.push(' ');
            }
        }
        for _ in 0..(width.saturating_sub(display_deltas.len())) { output.push(' '); }
        output.push_str("│\n");
    }

    output.push_str(&format!("{:.1}ms └", min));
    output.push_str(&"─".repeat(width));
    output.push_str("┘\n");
    output.push_str("      Legend: ░ Normal  ▒ Minor  ▓ Moderate  █ Severe\n");
    output
}

pub fn analyze_stutter(deltas: &[f64]) -> Vec<StutterEvent> {
    if deltas.len() < 2 { return Vec::new(); }
    let avg: f64 = deltas.iter().sum::<f64>() / deltas.len() as f64;
    let mut events = Vec::new();

    for (i, delta) in deltas.iter().enumerate() {
        let deviation = (*delta - avg).abs();
        let severity = if deviation > 8.0 { Some(StutterSeverity::Severe) }
            else if deviation > 4.0 { Some(StutterSeverity::Moderate) }
            else if deviation > 2.0 { Some(StutterSeverity::Minor) }
            else { None };

        if let Some(sev) = severity {
            events.push(StutterEvent { index: i, delta_ms: *delta, deviation_ms: deviation, severity: sev });
        }
    }
    events
}

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}

#[derive(Clone)]
pub struct StutterEvent {
    #[allow(dead_code)]
    pub index: usize,
    #[allow(dead_code)]
    pub delta_ms: f64,
    #[allow(dead_code)]
    pub deviation_ms: f64,
    pub severity: StutterSeverity,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StutterSeverity { Minor, Moderate, Severe }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_stutter_empty_input() {
        let result = analyze_stutter(&[]);
        assert!(result.is_empty(), "Empty input should return empty result");
    }

    #[test]
    fn test_analyze_stutter_single_element() {
        let result = analyze_stutter(&[5.0]);
        assert!(result.is_empty(), "Single element should return empty result");
    }

    #[test]
    fn test_analyze_stutter_uniform_deltas() {
        // Uniform deltas with no deviation should produce no stutter events
        let deltas = vec![2.0, 2.0, 2.0, 2.0, 2.0];
        let result = analyze_stutter(&deltas);
        assert!(result.is_empty(), "Uniform deltas should produce no stutter events");
    }

    #[test]
    fn test_analyze_stutter_minor_deviation() {
        // Need deviation > 2.0 but <= 4.0 for Minor
        // deltas = [5.0, 5.0, 5.0, 5.0, 8.0], avg = 5.6
        // deviation of 8.0 from 5.6 = 2.4 (minor)
        let deltas = vec![5.0, 5.0, 5.0, 5.0, 8.0];
        let result = analyze_stutter(&deltas);
        let minor_count = result.iter().filter(|e| e.severity == StutterSeverity::Minor).count();
        assert!(minor_count > 0, "Should detect minor stutter for small deviations");
    }

    #[test]
    fn test_analyze_stutter_moderate_deviation() {
        // Create a delta with 4-8ms deviation from average
        let deltas = vec![5.0, 5.0, 5.0, 5.0, 11.0]; // avg=6.2, deviation of 11-6.2=4.8
        let result = analyze_stutter(&deltas);
        let moderate_count = result.iter().filter(|e| e.severity == StutterSeverity::Moderate).count();
        assert!(moderate_count > 0, "Should detect moderate stutter for 4-8ms deviations");
    }

    #[test]
    fn test_analyze_stutter_severe_deviation() {
        // Create a delta with >8ms deviation from average
        let deltas = vec![2.0, 2.0, 2.0, 2.0, 15.0]; // avg=4.6, deviation of 15-4.6=10.4
        let result = analyze_stutter(&deltas);
        let severe_count = result.iter().filter(|e| e.severity == StutterSeverity::Severe).count();
        assert!(severe_count > 0, "Should detect severe stutter for >8ms deviations");
    }

    #[test]
    fn test_analyze_stutter_event_fields() {
        let deltas = vec![2.0, 2.0, 2.0, 20.0]; // Clear outlier
        let result = analyze_stutter(&deltas);

        assert!(!result.is_empty(), "Should detect at least one stutter event");
        let event = &result[result.len() - 1]; // The outlier should be last
        assert_eq!(event.index, 3, "Event index should match the outlier position");
        assert!((event.delta_ms - 20.0).abs() < 0.001, "Delta should match input value");
        assert!(event.deviation_ms > 8.0, "Deviation should be significant");
    }

    #[test]
    fn test_stutter_severity_thresholds() {
        // Test threshold boundaries
        // Minor: deviation > 2.0 and <= 4.0
        // Moderate: deviation > 4.0 and <= 8.0
        // Severe: deviation > 8.0

        // For minor: avg = (10*4 + 13) / 5 = 10.6, deviation of 13 = 2.4 (minor)
        let deltas_minor = vec![10.0, 10.0, 10.0, 10.0, 13.0];
        // For moderate: avg = (10*4 + 16) / 5 = 11.2, deviation of 16 = 4.8 (moderate)
        let deltas_moderate = vec![10.0, 10.0, 10.0, 10.0, 16.0];
        // For severe: avg = (10*4 + 22) / 5 = 12.4, deviation of 22 = 9.6 (severe)
        let deltas_severe = vec![10.0, 10.0, 10.0, 10.0, 22.0];

        let result_minor = analyze_stutter(&deltas_minor);
        let result_moderate = analyze_stutter(&deltas_moderate);
        let result_severe = analyze_stutter(&deltas_severe);

        assert!(result_minor.iter().any(|e| e.severity == StutterSeverity::Minor),
            "Should detect minor stutter");
        assert!(result_moderate.iter().any(|e| e.severity == StutterSeverity::Moderate),
            "Should detect moderate stutter");
        assert!(result_severe.iter().any(|e| e.severity == StutterSeverity::Severe),
            "Should detect severe stutter");
    }
}
