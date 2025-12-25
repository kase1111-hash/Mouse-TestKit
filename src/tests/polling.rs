/// Polling Rate Monitor
/// Displays real-time mouse polling rate in Hz

use std::time::{Duration, Instant};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crate::input::{self, MouseEvent};

pub fn run() {
    println!("\n=== Polling Rate Monitor ===");
    println!("Move your mouse to measure polling rate.");
    println!("Press 'q' to quit.\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            wait_for_enter();
            return;
        }
    };

    // Set non-blocking
    device.grab().ok(); // Grab device for exclusive access

    let mut stats = PollingStats::new();
    let mut last_print = Instant::now();
    let mut timestamps: Vec<Instant> = Vec::new();

    crossterm::terminal::enable_raw_mode().ok();

    println!("\nMonitoring... (press 'q' to quit)\n");

    loop {
        // Check for quit key
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent { code: KeyCode::Char('q'), .. })) = event::read() {
                break;
            }
        }

        // Read mouse events
        if let Ok(events) = device.fetch_events() {
            for ev in events {
                if let Some(MouseEvent::Move { .. }) = input::parse_event(&ev) {
                    timestamps.push(Instant::now());
                }
            }
        }

        // Calculate polling rate every 100ms
        if last_print.elapsed() >= Duration::from_millis(100) {
            let now = Instant::now();

            // Keep only timestamps from last second
            timestamps.retain(|t| now.duration_since(*t) < Duration::from_secs(1));

            if timestamps.len() >= 2 {
                let hz = timestamps.len() as u32;
                stats.update(hz);

                // Calculate interval-based Hz
                let mut intervals: Vec<u128> = Vec::new();
                for i in 1..timestamps.len() {
                    let delta = timestamps[i].duration_since(timestamps[i-1]).as_micros();
                    if delta > 0 {
                        intervals.push(delta);
                    }
                }

                let avg_interval_hz = if !intervals.is_empty() {
                    let avg_interval = intervals.iter().sum::<u128>() / intervals.len() as u128;
                    if avg_interval > 0 { 1_000_000 / avg_interval } else { 0 }
                } else {
                    0
                };

                print!("\r\x1B[K");
                print!("Current: {:4} Hz | ", avg_interval_hz);
                print!("Min: {:4} Hz | ", stats.min_hz);
                print!("Max: {:4} Hz | ", stats.max_hz);
                print!("Avg: {:6.1} Hz | ", stats.avg_hz);
                print!("Samples: {}", stats.samples);

                use std::io::Write;
                std::io::stdout().flush().ok();
            }

            last_print = now;
        }
    }

    crossterm::terminal::disable_raw_mode().ok();

    println!("\n\nPolling rate test complete.");
    if stats.min_hz < u32::MAX {
        println!("Final stats - Min: {} Hz, Max: {} Hz, Avg: {:.1} Hz",
            stats.min_hz, stats.max_hz, stats.avg_hz);
    }

    wait_for_enter();
}

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}

pub struct PollingStats {
    pub current_hz: u32,
    pub min_hz: u32,
    pub max_hz: u32,
    pub avg_hz: f64,
    pub samples: u32,
}

impl PollingStats {
    pub fn new() -> Self {
        Self {
            current_hz: 0,
            min_hz: u32::MAX,
            max_hz: 0,
            avg_hz: 0.0,
            samples: 0,
        }
    }

    pub fn update(&mut self, hz: u32) {
        if hz == 0 { return; }
        self.current_hz = hz;
        self.min_hz = self.min_hz.min(hz);
        self.max_hz = self.max_hz.max(hz);
        self.samples += 1;
        self.avg_hz = (self.avg_hz * (self.samples - 1) as f64 + hz as f64) / self.samples as f64;
    }
}

impl Default for PollingStats {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polling_stats_new() {
        let stats = PollingStats::new();
        assert_eq!(stats.current_hz, 0);
        assert_eq!(stats.min_hz, u32::MAX);
        assert_eq!(stats.max_hz, 0);
        assert_eq!(stats.avg_hz, 0.0);
        assert_eq!(stats.samples, 0);
    }

    #[test]
    fn test_polling_stats_default() {
        let stats = PollingStats::default();
        assert_eq!(stats.current_hz, 0);
        assert_eq!(stats.min_hz, u32::MAX);
    }

    #[test]
    fn test_polling_stats_update_single() {
        let mut stats = PollingStats::new();
        stats.update(1000);

        assert_eq!(stats.current_hz, 1000);
        assert_eq!(stats.min_hz, 1000);
        assert_eq!(stats.max_hz, 1000);
        assert_eq!(stats.avg_hz, 1000.0);
        assert_eq!(stats.samples, 1);
    }

    #[test]
    fn test_polling_stats_update_multiple() {
        let mut stats = PollingStats::new();
        stats.update(500);
        stats.update(1000);
        stats.update(1500);

        assert_eq!(stats.current_hz, 1500);
        assert_eq!(stats.min_hz, 500);
        assert_eq!(stats.max_hz, 1500);
        assert_eq!(stats.samples, 3);
        // Average: (500 + 1000 + 1500) / 3 = 1000
        assert!((stats.avg_hz - 1000.0).abs() < 0.001);
    }

    #[test]
    fn test_polling_stats_update_ignores_zero() {
        let mut stats = PollingStats::new();
        stats.update(1000);
        stats.update(0); // Should be ignored

        assert_eq!(stats.current_hz, 1000);
        assert_eq!(stats.samples, 1);
        assert_eq!(stats.avg_hz, 1000.0);
    }

    #[test]
    fn test_polling_stats_min_max_tracking() {
        let mut stats = PollingStats::new();
        stats.update(800);
        stats.update(1200);
        stats.update(600);
        stats.update(1400);

        assert_eq!(stats.min_hz, 600);
        assert_eq!(stats.max_hz, 1400);
    }

    #[test]
    fn test_polling_stats_average_calculation() {
        let mut stats = PollingStats::new();

        // Add several samples and verify running average
        stats.update(100);
        assert_eq!(stats.avg_hz, 100.0);

        stats.update(200);
        // (100 * 1 + 200) / 2 = 150
        assert!((stats.avg_hz - 150.0).abs() < 0.001);

        stats.update(300);
        // (150 * 2 + 300) / 3 = 200
        assert!((stats.avg_hz - 200.0).abs() < 0.001);
    }

    #[test]
    fn test_polling_stats_large_values() {
        let mut stats = PollingStats::new();
        stats.update(8000); // High polling rate mouse

        assert_eq!(stats.current_hz, 8000);
        assert_eq!(stats.min_hz, 8000);
        assert_eq!(stats.max_hz, 8000);
    }

    #[test]
    fn test_polling_stats_consistent_samples() {
        let mut stats = PollingStats::new();
        for _ in 0..100 {
            stats.update(1000);
        }

        assert_eq!(stats.samples, 100);
        assert_eq!(stats.min_hz, 1000);
        assert_eq!(stats.max_hz, 1000);
        assert!((stats.avg_hz - 1000.0).abs() < 0.001);
    }
}
