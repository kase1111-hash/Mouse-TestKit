//! Stroke segmentation
//!
//! Splits a stream of relative motion reports into "strokes": runs of
//! continuous movement separated by pauses. Several tests (acceleration,
//! angle snapping, the automated diagnostics) reason about whole strokes
//! rather than individual reports, because a single report at 1000 Hz is
//! only one or two counts and carries almost no shape or speed information.

/// A run of continuous movement.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    /// Per-report (dx, dy) counts in order of arrival.
    pub deltas: Vec<(i32, i32)>,
    /// Timestamp (seconds) of the first report.
    pub start: f64,
    /// Timestamp (seconds) of the last report.
    pub end: f64,
}

impl Stroke {
    /// Sum of per-report magnitudes (counts travelled along the path).
    pub fn path_length(&self) -> f64 {
        self.deltas
            .iter()
            .map(|&(dx, dy)| ((dx as f64).powi(2) + (dy as f64).powi(2)).sqrt())
            .sum()
    }

    /// Straight-line displacement from start to end, in counts.
    pub fn net_displacement(&self) -> (f64, f64) {
        let (sx, sy) = self
            .deltas
            .iter()
            .fold((0i64, 0i64), |(ax, ay), &(dx, dy)| {
                (ax + dx as i64, ay + dy as i64)
            });
        (sx as f64, sy as f64)
    }

    /// Duration in seconds between the first and last report.
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    /// Average speed in counts per second. A stroke shorter than one
    /// millisecond is treated as one millisecond long to avoid division by zero.
    pub fn velocity(&self) -> f64 {
        self.path_length() / self.duration().max(0.001)
    }

    pub fn events(&self) -> usize {
        self.deltas.len()
    }
}

/// Groups motion reports into strokes separated by pauses longer than `gap_s`.
#[derive(Debug)]
pub struct StrokeSegmenter {
    gap_s: f64,
    current: Option<Stroke>,
}

impl StrokeSegmenter {
    /// `gap_s` is the pause (in seconds) that ends a stroke.
    pub fn new(gap_s: f64) -> Self {
        Self {
            gap_s,
            current: None,
        }
    }

    /// Feed one motion report. Returns the previous stroke when this report
    /// starts a new one (i.e. a pause longer than the gap preceded it).
    pub fn feed(&mut self, t: f64, dx: i32, dy: i32) -> Option<Stroke> {
        if dx == 0 && dy == 0 {
            return None;
        }
        let finished = match &self.current {
            Some(cur) if t - cur.end > self.gap_s => self.current.take(),
            _ => None,
        };
        match &mut self.current {
            Some(cur) => {
                cur.deltas.push((dx, dy));
                cur.end = t;
            }
            None => {
                self.current = Some(Stroke {
                    deltas: vec![(dx, dy)],
                    start: t,
                    end: t,
                });
            }
        }
        finished
    }

    /// Advance time without a report. Returns the current stroke if the
    /// pause since its last report now exceeds the gap.
    pub fn tick(&mut self, now: f64) -> Option<Stroke> {
        match &self.current {
            Some(cur) if now - cur.end > self.gap_s => self.current.take(),
            _ => None,
        }
    }

    /// The stroke currently being recorded, if any.
    pub fn in_progress(&self) -> Option<&Stroke> {
        self.current.as_ref()
    }

    /// Force-finish and return whatever stroke is in progress.
    pub fn flush(&mut self) -> Option<Stroke> {
        self.current.take()
    }

    pub fn reset(&mut self) {
        self.current = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stroke_geometry() {
        let s = Stroke {
            deltas: vec![(3, 4), (3, 4)],
            start: 1.0,
            end: 1.5,
        };
        assert!((s.path_length() - 10.0).abs() < 1e-9);
        assert_eq!(s.net_displacement(), (6.0, 8.0));
        assert!((s.duration() - 0.5).abs() < 1e-9);
        assert!((s.velocity() - 20.0).abs() < 1e-9);
        assert_eq!(s.events(), 2);
    }

    #[test]
    fn test_velocity_of_instant_stroke_is_finite() {
        let s = Stroke {
            deltas: vec![(5, 0)],
            start: 2.0,
            end: 2.0,
        };
        assert!(s.velocity().is_finite());
        assert!(s.velocity() > 0.0);
    }

    #[test]
    fn test_segmenter_splits_on_gap() {
        let mut seg = StrokeSegmenter::new(0.2);
        assert_eq!(seg.feed(0.000, 1, 0), None);
        assert_eq!(seg.feed(0.001, 1, 0), None);
        assert_eq!(seg.feed(0.002, 1, 1), None);
        // 0.5 s later -> new stroke, previous one returned
        let first = seg.feed(0.502, 2, 0).expect("first stroke should finish");
        assert_eq!(first.deltas, vec![(1, 0), (1, 0), (1, 1)]);
        assert_eq!(first.start, 0.0);
        assert_eq!(first.end, 0.002);
        assert_eq!(seg.in_progress().map(|s| s.deltas.len()), Some(1));
        let last = seg.flush().unwrap();
        assert_eq!(last.deltas, vec![(2, 0)]);
        assert!(seg.in_progress().is_none());
    }

    #[test]
    fn test_segmenter_tick_finishes_stroke_after_pause() {
        let mut seg = StrokeSegmenter::new(0.2);
        seg.feed(0.0, 1, 0);
        assert_eq!(seg.tick(0.1), None);
        let s = seg.tick(0.3).expect("stroke should end after gap");
        assert_eq!(s.deltas, vec![(1, 0)]);
        assert_eq!(seg.tick(0.4), None);
    }

    #[test]
    fn test_segmenter_ignores_zero_motion() {
        let mut seg = StrokeSegmenter::new(0.2);
        assert_eq!(seg.feed(0.0, 0, 0), None);
        assert!(seg.in_progress().is_none());
    }

    #[test]
    fn test_segmenter_reset() {
        let mut seg = StrokeSegmenter::new(0.2);
        seg.feed(0.0, 1, 0);
        seg.reset();
        assert!(seg.in_progress().is_none());
        assert_eq!(seg.flush(), None);
    }
}
