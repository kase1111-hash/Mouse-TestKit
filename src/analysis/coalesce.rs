//! Relative-axis event coalescing
//!
//! Linux evdev reports a single hardware poll as separate `REL_X` and `REL_Y`
//! events that share one kernel timestamp. Treating them as two reports would
//! double the apparent polling rate and split every motion vector in half.
//! [`EventCoalescer`] merges events that share a timestamp into one `(dx, dy)`
//! pair. Windows Raw Input already delivers combined reports and does not need it.

use std::time::SystemTime;

/// Coalesces separate REL_X and REL_Y evdev events that share the
/// same kernel timestamp into a single (dx, dy) pair.
#[derive(Debug, Default)]
pub struct EventCoalescer {
    pending_dx: i32,
    pending_dy: i32,
    last_ts: Option<SystemTime>,
}

impl EventCoalescer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a relative-axis event at the given kernel timestamp.
    /// Returns `Some((dx, dy))` if a previous batch needs flushing
    /// because the timestamp has changed.
    pub fn accumulate(&mut self, ts: SystemTime, dx: i32, dy: i32) -> Option<(i32, i32)> {
        let flushed = match self.last_ts {
            Some(prev_ts) if ts != prev_ts => self.flush(),
            _ => None,
        };
        self.last_ts = Some(ts);
        self.pending_dx += dx;
        self.pending_dy += dy;
        flushed
    }

    /// Flush any remaining accumulated movement.
    /// Call after processing a batch of events.
    pub fn flush(&mut self) -> Option<(i32, i32)> {
        if self.pending_dx != 0 || self.pending_dy != 0 {
            let result = (self.pending_dx, self.pending_dy);
            self.pending_dx = 0;
            self.pending_dy = 0;
            Some(result)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ts(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn test_single_x_then_flush() {
        let mut c = EventCoalescer::new();
        assert_eq!(c.accumulate(ts(1), 10, 0), None);
        assert_eq!(c.flush(), Some((10, 0)));
        assert_eq!(c.flush(), None);
    }

    #[test]
    fn test_merge_x_and_y_same_timestamp() {
        let mut c = EventCoalescer::new();
        assert_eq!(c.accumulate(ts(1), 5, 0), None);
        assert_eq!(c.accumulate(ts(1), 0, -3), None);
        assert_eq!(c.flush(), Some((5, -3)));
    }

    #[test]
    fn test_flush_on_timestamp_change() {
        let mut c = EventCoalescer::new();
        assert_eq!(c.accumulate(ts(1), 5, 0), None);
        assert_eq!(c.accumulate(ts(1), 0, -3), None);
        // New timestamp flushes the previous batch
        assert_eq!(c.accumulate(ts(2), 10, 0), Some((5, -3)));
        assert_eq!(c.flush(), Some((10, 0)));
    }

    #[test]
    fn test_multiple_sequential_batches() {
        let mut c = EventCoalescer::new();
        assert_eq!(c.accumulate(ts(1), 3, 0), None);
        assert_eq!(c.accumulate(ts(1), 0, 4), None);
        assert_eq!(c.accumulate(ts(2), -1, 0), Some((3, 4)));
        assert_eq!(c.accumulate(ts(2), 0, 2), None);
        assert_eq!(c.accumulate(ts(3), 7, 0), Some((-1, 2)));
        assert_eq!(c.accumulate(ts(3), 0, -5), None);
        assert_eq!(c.flush(), Some((7, -5)));
    }

    #[test]
    fn test_no_flush_when_all_zero() {
        let mut c = EventCoalescer::new();
        assert_eq!(c.accumulate(ts(1), 0, 0), None);
        assert_eq!(c.flush(), None);
    }

    #[test]
    fn test_accumulates_multiple_same_axis_same_ts() {
        let mut c = EventCoalescer::new();
        assert_eq!(c.accumulate(ts(1), 3, 0), None);
        assert_eq!(c.accumulate(ts(1), 4, 0), None);
        assert_eq!(c.flush(), Some((7, 0)));
    }

    #[test]
    fn test_negative_deltas() {
        let mut c = EventCoalescer::new();
        assert_eq!(c.accumulate(ts(1), -5, 0), None);
        assert_eq!(c.accumulate(ts(1), 0, 3), None);
        assert_eq!(c.flush(), Some((-5, 3)));
    }

    #[test]
    fn test_zero_batch_between_real_batches_does_not_flush_zero() {
        let mut c = EventCoalescer::new();
        assert_eq!(c.accumulate(ts(1), 0, 0), None);
        // Timestamp changed but nothing pending: no spurious (0, 0) report
        assert_eq!(c.accumulate(ts(2), 1, 0), None);
        assert_eq!(c.flush(), Some((1, 0)));
    }
}
