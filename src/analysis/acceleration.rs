//! Acceleration analysis
//!
//! Mouse acceleration means the number of counts reported for a fixed
//! physical distance grows with the speed of the movement. The only way to
//! detect it without knowing the physical distance is to have the user repeat
//! the *same* physical stroke at different speeds and compare the counts each
//! stroke produced. This module takes per-stroke samples (counts, speed) and
//! compares the slow half against the fast half.

/// Summary of one stroke: how many counts it produced and how fast it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeSample {
    /// Counts travelled along the stroke.
    pub counts: f64,
    /// Average speed in counts per second.
    pub velocity: f64,
}

/// Result of comparing slow strokes against fast strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct AccelAnalysis {
    /// True when the fast strokes produced a materially different count
    /// than the slow strokes and the speed difference was large enough to judge.
    pub has_acceleration: bool,
    /// Whether the data allowed a judgement at all (enough strokes, enough
    /// speed contrast). When false, `has_acceleration` is always false.
    pub conclusive: bool,
    /// Mean fast-stroke counts divided by mean slow-stroke counts. 1.0 = no acceleration.
    pub factor: f64,
    /// Median fast velocity divided by median slow velocity.
    pub speed_ratio: f64,
    pub slow_count: usize,
    pub fast_count: usize,
    pub slow_avg_counts: f64,
    pub fast_avg_counts: f64,
    pub slow_avg_velocity: f64,
    pub fast_avg_velocity: f64,
    /// 0-100, based on the number of strokes and the speed contrast.
    pub confidence_percent: f32,
}

/// Strokes shorter than this (in counts) are noise, not a deliberate pass.
pub const MIN_STROKE_COUNTS: f64 = 50.0;
/// Minimum strokes in each speed group before a verdict is given.
pub const MIN_STROKES_PER_GROUP: usize = 3;
/// Fast strokes must be at least this many times faster than slow ones.
pub const MIN_SPEED_RATIO: f64 = 1.5;
/// Count ratios outside 1 +/- this are reported as acceleration.
pub const ACCEL_TOLERANCE: f64 = 0.15;

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// Compare the slower half of the strokes against the faster half.
///
/// Returns `None` when there are not enough usable strokes to form two groups.
pub fn analyze_strokes(samples: &[StrokeSample]) -> Option<AccelAnalysis> {
    let mut usable: Vec<StrokeSample> = samples
        .iter()
        .copied()
        .filter(|s| s.counts >= MIN_STROKE_COUNTS && s.velocity > 0.0)
        .collect();
    if usable.len() < MIN_STROKES_PER_GROUP * 2 {
        return None;
    }
    usable.sort_by(|a, b| {
        a.velocity
            .partial_cmp(&b.velocity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let half = usable.len() / 2;
    let slow = &usable[..half];
    let fast = &usable[usable.len() - half..];

    let slow_counts: Vec<f64> = slow.iter().map(|s| s.counts).collect();
    let fast_counts: Vec<f64> = fast.iter().map(|s| s.counts).collect();
    let slow_vel: Vec<f64> = slow.iter().map(|s| s.velocity).collect();
    let fast_vel: Vec<f64> = fast.iter().map(|s| s.velocity).collect();

    let slow_avg_counts = mean(&slow_counts);
    let fast_avg_counts = mean(&fast_counts);
    let slow_avg_velocity = mean(&slow_vel);
    let fast_avg_velocity = mean(&fast_vel);

    // Medians for the speed contrast: robust to one wild stroke.
    let slow_median_vel = slow_vel[slow_vel.len() / 2];
    let fast_median_vel = fast_vel[fast_vel.len() / 2];
    let speed_ratio = if slow_median_vel > 0.0 {
        fast_median_vel / slow_median_vel
    } else {
        f64::INFINITY
    };

    let factor = if slow_avg_counts > 0.0 {
        fast_avg_counts / slow_avg_counts
    } else {
        1.0
    };

    let conclusive = speed_ratio >= MIN_SPEED_RATIO;
    let has_acceleration = conclusive && (factor - 1.0).abs() > ACCEL_TOLERANCE;

    let group_confidence = (half as f32 / 6.0).min(1.0);
    let contrast_confidence = if conclusive {
        (((speed_ratio - 1.0) / 2.0) as f32).clamp(0.5, 1.0)
    } else {
        0.25
    };
    let confidence_percent = group_confidence * contrast_confidence * 100.0;

    Some(AccelAnalysis {
        has_acceleration,
        conclusive,
        factor,
        speed_ratio,
        slow_count: slow.len(),
        fast_count: fast.len(),
        slow_avg_counts,
        fast_avg_counts,
        slow_avg_velocity,
        fast_avg_velocity,
        confidence_percent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strokes(pairs: &[(f64, f64)]) -> Vec<StrokeSample> {
        pairs
            .iter()
            .map(|&(counts, velocity)| StrokeSample { counts, velocity })
            .collect()
    }

    #[test]
    fn test_not_enough_strokes() {
        let s = strokes(&[(800.0, 500.0), (810.0, 2000.0), (790.0, 600.0)]);
        assert!(analyze_strokes(&s).is_none());
    }

    #[test]
    fn test_tiny_strokes_are_ignored() {
        let mut s = strokes(&[(800.0, 500.0), (810.0, 2000.0), (790.0, 600.0)]);
        s.extend(strokes(&[(5.0, 100.0), (8.0, 3000.0), (3.0, 50.0)]));
        assert!(analyze_strokes(&s).is_none());
    }

    #[test]
    fn test_no_acceleration_same_counts_at_all_speeds() {
        let s = strokes(&[
            (800.0, 400.0),
            (790.0, 450.0),
            (805.0, 500.0),
            (798.0, 1800.0),
            (812.0, 2000.0),
            (795.0, 2200.0),
        ]);
        let a = analyze_strokes(&s).unwrap();
        assert!(a.conclusive);
        assert!(!a.has_acceleration);
        assert!((a.factor - 1.0).abs() < 0.05);
        assert_eq!(a.slow_count, 3);
        assert_eq!(a.fast_count, 3);
        assert!(a.speed_ratio > 3.0);
    }

    #[test]
    fn test_positive_acceleration_detected() {
        let s = strokes(&[
            (800.0, 400.0),
            (790.0, 450.0),
            (805.0, 500.0),
            (1200.0, 1800.0),
            (1250.0, 2000.0),
            (1180.0, 2200.0),
        ]);
        let a = analyze_strokes(&s).unwrap();
        assert!(a.conclusive);
        assert!(a.has_acceleration);
        assert!(a.factor > 1.4);
    }

    #[test]
    fn test_negative_acceleration_detected() {
        let s = strokes(&[
            (800.0, 400.0),
            (790.0, 450.0),
            (805.0, 500.0),
            (600.0, 1800.0),
            (620.0, 2000.0),
            (590.0, 2200.0),
        ]);
        let a = analyze_strokes(&s).unwrap();
        assert!(a.has_acceleration);
        assert!(a.factor < 0.85);
    }

    #[test]
    fn test_inconclusive_when_speeds_too_similar() {
        // All strokes at nearly the same speed: cannot judge, must not claim acceleration
        let s = strokes(&[
            (800.0, 1000.0),
            (900.0, 1010.0),
            (700.0, 1020.0),
            (1000.0, 1030.0),
            (850.0, 1040.0),
            (950.0, 1050.0),
        ]);
        let a = analyze_strokes(&s).unwrap();
        assert!(!a.conclusive);
        assert!(!a.has_acceleration);
        assert!(a.confidence_percent < 50.0);
    }

    #[test]
    fn test_confidence_grows_with_strokes() {
        let few = strokes(&[
            (800.0, 400.0),
            (800.0, 450.0),
            (800.0, 500.0),
            (800.0, 1800.0),
            (800.0, 2000.0),
            (800.0, 2200.0),
        ]);
        let mut many = few.clone();
        many.extend(few.clone());
        many.extend(few.clone());
        let a_few = analyze_strokes(&few).unwrap();
        let a_many = analyze_strokes(&many).unwrap();
        assert!(a_many.confidence_percent > a_few.confidence_percent);
        assert!(a_many.confidence_percent <= 100.0);
    }
}
