//! Acceleration analysis
//!
//! Mouse acceleration means the number of counts reported for a fixed
//! physical distance grows with the speed of the movement. The only way to
//! detect it without knowing the physical distance is to have the user repeat
//! the *same* physical stroke at different speeds and compare the counts each
//! stroke produced. This module takes per-stroke samples (counts, speed) and
//! compares the slow half against the fast half, by medians so that a stray
//! stroke (moving the pointer to a button, two passes run together) cannot
//! decide the verdict on its own.

use crate::analysis::polling::median_in_place;
use crate::analysis::strokes::Stroke;

/// Summary of one stroke: how many counts it produced and how fast it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeSample {
    /// Counts travelled along the stroke.
    pub counts: f64,
    /// Average speed in counts per second.
    pub velocity: f64,
}

/// What the comparison of slow and fast strokes shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccelVerdict {
    /// The fast strokes were not clearly faster than the slow ones.
    Inconclusive,
    /// Fast and slow strokes produced the same counts.
    NoAcceleration,
    /// Fast strokes produced more counts: acceleration.
    Acceleration,
    /// Fast strokes produced fewer counts. That is almost always the sensor
    /// losing tracking at speed (or the pointer hitting the screen edge),
    /// not an acceleration setting.
    LosesCounts,
}

impl AccelVerdict {
    /// Short machine-friendly name, for exports.
    pub fn label(self) -> &'static str {
        match self {
            AccelVerdict::Inconclusive => "inconclusive",
            AccelVerdict::NoAcceleration => "no acceleration",
            AccelVerdict::Acceleration => "acceleration",
            AccelVerdict::LosesCounts => "fast passes lose counts",
        }
    }
}

/// Result of comparing slow strokes against fast strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct AccelAnalysis {
    pub verdict: AccelVerdict,
    /// True when the fast strokes produced materially more counts than the
    /// slow strokes and the speed difference was large enough to judge
    /// (`verdict == Acceleration`).
    pub has_acceleration: bool,
    /// Whether the data allowed a judgement at all (enough strokes, enough
    /// speed contrast). When false, `has_acceleration` is always false.
    pub conclusive: bool,
    /// Median fast-stroke counts divided by median slow-stroke counts.
    /// 1.0 = no acceleration.
    pub factor: f64,
    /// Median fast velocity divided by median slow velocity.
    pub speed_ratio: f64,
    pub slow_count: usize,
    pub fast_count: usize,
    pub slow_median_counts: f64,
    pub fast_median_counts: f64,
    pub slow_median_velocity: f64,
    pub fast_median_velocity: f64,
    /// 0-100, based on the number of strokes and the speed contrast.
    pub confidence_percent: f32,
}

/// Strokes shorter than this (in counts) are noise, not a deliberate pass.
pub const MIN_STROKE_COUNTS: f64 = 50.0;
/// Minimum strokes in each speed group before a verdict is given.
pub const MIN_STROKES_PER_GROUP: usize = 3;
/// Fast strokes must be at least this many times faster than slow ones.
pub const MIN_SPEED_RATIO: f64 = 1.5;
/// Count ratios above 1 + this are reported as acceleration, below 1 - this
/// as lost counts.
pub const ACCEL_TOLERANCE: f64 = 0.15;
/// A stroke whose straight-line displacement is less than this fraction of
/// the path it travelled turned back on itself (by more than a quarter of
/// its length): two passes run together without the pause between them, or
/// a correction at the end. A hand's pass across the pad, curved and wobbly
/// as it is, stays well above it.
pub const MIN_STRAIGHTNESS: f64 = 0.6;

/// Why a stroke is not used as a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAPass {
    /// Under [`MIN_STROKE_COUNTS`]: noise, not a deliberate pass.
    TooShort,
    /// It turned back on itself (see [`MIN_STRAIGHTNESS`]).
    TurnsBack,
}

/// The pass a stroke represents, or why it is not one.
pub fn stroke_sample(stroke: &Stroke) -> Result<StrokeSample, NotAPass> {
    let counts = stroke.path_length();
    if counts < MIN_STROKE_COUNTS {
        return Err(NotAPass::TooShort);
    }
    let (nx, ny) = stroke.net_displacement();
    if nx.hypot(ny) < MIN_STRAIGHTNESS * counts {
        return Err(NotAPass::TurnsBack);
    }
    Ok(StrokeSample {
        counts,
        velocity: stroke.velocity(),
    })
}

fn median(values: &[f64]) -> f64 {
    median_in_place(&mut values.to_vec()).unwrap_or(0.0)
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

    // Medians throughout: robust to one wild stroke in either group.
    let slow_median_counts = median(&slow_counts);
    let fast_median_counts = median(&fast_counts);
    let slow_median_velocity = median(&slow_vel);
    let fast_median_velocity = median(&fast_vel);
    let speed_ratio = if slow_median_velocity > 0.0 {
        fast_median_velocity / slow_median_velocity
    } else {
        f64::INFINITY
    };

    let factor = if slow_median_counts > 0.0 {
        fast_median_counts / slow_median_counts
    } else {
        1.0
    };

    let conclusive = speed_ratio >= MIN_SPEED_RATIO;
    let verdict = if !conclusive {
        AccelVerdict::Inconclusive
    } else if factor > 1.0 + ACCEL_TOLERANCE {
        AccelVerdict::Acceleration
    } else if factor < 1.0 - ACCEL_TOLERANCE {
        AccelVerdict::LosesCounts
    } else {
        AccelVerdict::NoAcceleration
    };
    let has_acceleration = verdict == AccelVerdict::Acceleration;

    let group_confidence = (half as f32 / 6.0).min(1.0);
    let contrast_confidence = if conclusive {
        (((speed_ratio - 1.0) / 2.0) as f32).clamp(0.5, 1.0)
    } else {
        0.25
    };
    let confidence_percent = group_confidence * contrast_confidence * 100.0;

    Some(AccelAnalysis {
        verdict,
        has_acceleration,
        conclusive,
        factor,
        speed_ratio,
        slow_count: slow.len(),
        fast_count: fast.len(),
        slow_median_counts,
        fast_median_counts,
        slow_median_velocity,
        fast_median_velocity,
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
        // Lost counts, not acceleration
        assert_eq!(a.verdict, AccelVerdict::LosesCounts);
        assert!(!a.has_acceleration);
        assert!(a.factor < 0.85);
    }

    #[test]
    fn test_stray_strokes_do_not_decide_the_verdict() {
        // A linear mouse: 5 slow and 5 fast passes of about 19000 counts,
        // plus a slow 3000-count move to the Stop button.
        let mut s = strokes(&[
            (19_000.0, 9_000.0),
            (19_100.0, 10_000.0),
            (18_900.0, 11_000.0),
            (19_050.0, 12_000.0),
            (18_950.0, 13_000.0),
            (19_000.0, 60_000.0),
            (19_100.0, 70_000.0),
            (18_900.0, 80_000.0),
            (19_050.0, 90_000.0),
            (18_950.0, 100_000.0),
        ]);
        s.push(StrokeSample {
            counts: 3_000.0,
            velocity: 4_000.0,
        });
        let a = analyze_strokes(&s).unwrap();
        assert_eq!(a.verdict, AccelVerdict::NoAcceleration, "{}", a.factor);
        assert!((a.factor - 1.0).abs() < 0.02);

        // Two fast passes run together (out and back as one stroke)
        let mut s = strokes(&[(800.0, 400.0); 10]);
        s.extend(strokes(&[(800.0, 2_000.0); 6]));
        s.extend(strokes(&[(1_600.0, 1_900.0); 2]));
        let a = analyze_strokes(&s).unwrap();
        assert_eq!(a.verdict, AccelVerdict::NoAcceleration, "{}", a.factor);
    }

    fn stroke(deltas: Vec<(i32, i32)>) -> Stroke {
        Stroke {
            deltas,
            start: 0.0,
            end: 0.5,
        }
    }

    #[test]
    fn test_strokes_that_turn_back_are_not_passes() {
        // Out and back without a pause: twice the counts of one pass
        let mut out_and_back = vec![(8, 0); 100];
        out_and_back.extend(vec![(-8, 0); 100]);
        assert_eq!(
            stroke_sample(&stroke(out_and_back)),
            Err(NotAPass::TurnsBack)
        );
        // A curved, wobbly pass with a small correction at the end is one
        let mut pass: Vec<(i32, i32)> = (0..200).map(|i| (3, [1, 0, -1, 0][i % 4])).collect();
        pass.extend((0..100).map(|i| (1, if i < 50 { 1 } else { 2 })));
        pass.extend(vec![(-1, 0); 30]);
        let sample = stroke_sample(&stroke(pass)).unwrap();
        assert!(sample.counts > 700.0);
        assert_eq!(
            stroke_sample(&stroke(vec![(1, 0); 20])),
            Err(NotAPass::TooShort)
        );
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
