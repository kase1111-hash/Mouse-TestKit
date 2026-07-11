//! Polling rate statistics
//!
//! Tracks min/max/avg polling rate from a stream of Hz samples.

/// Running statistics for polling rate measurements.
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
        if hz == 0 {
            return;
        }
        self.current_hz = hz;
        self.min_hz = self.min_hz.min(hz);
        self.max_hz = self.max_hz.max(hz);
        self.samples += 1;
        self.avg_hz = (self.avg_hz * (self.samples - 1) as f64 + hz as f64) / self.samples as f64;
    }
}

impl Default for PollingStats {
    fn default() -> Self {
        Self::new()
    }
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
