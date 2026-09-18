//! Angle snapping analysis
//!
//! Angle snapping ("prediction") firmware straightens near-horizontal or
//! near-vertical hand movements into perfectly axis-aligned lines. A human
//! hand cannot draw a perfectly straight line: a freehand stroke of a hundred
//! or more counts always wanders off its fitted line by at least a count or
//! two. A snapped stroke, by contrast, lies *exactly* on the axis.
//!
//! The analysis therefore looks at whole strokes (see
//! [`crate::analysis::strokes`]), fits a straight line to each one, and
//! measures how far the path wanders from that line. Near-axis strokes that
//! are perfectly straight are the signature of snapping.

/// Geometry of one stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeShape {
    /// Path length in counts.
    pub length: f64,
    /// Direction of the net displacement in degrees, -180..180 (screen
    /// coordinates: positive y is down).
    pub angle_deg: f64,
    /// Angular distance to the nearest axis, 0..45 degrees.
    pub axis_deviation_deg: f64,
    /// Root-mean-square perpendicular distance of the path from its fitted line.
    pub rms_residual: f64,
    /// Largest perpendicular distance of any point from the fitted line.
    pub max_residual: f64,
    pub events: usize,
}

/// Verdict for a set of strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct AngleSnapAnalysis {
    pub has_snapping: bool,
    /// False when too few near-axis strokes were drawn to judge.
    pub conclusive: bool,
    pub total_strokes: usize,
    /// Strokes within [`NEAR_AXIS_DEG`] of horizontal or vertical.
    pub near_axis_strokes: usize,
    /// Near-axis strokes that were perfectly straight.
    pub snapped_strokes: usize,
    /// Fraction of near-axis strokes that were snapped (0..1).
    pub snap_strength: f64,
    /// Axes (0 = horizontal, 90 = vertical) on which snapping was seen.
    pub dominant_angles: Vec<f64>,
}

/// Strokes shorter than this (counts) are too short to judge straightness.
pub const MIN_STROKE_LENGTH: f64 = 100.0;
/// Strokes need at least this many reports to be judged.
pub const MIN_STROKE_EVENTS: usize = 20;
/// A stroke this close to an axis is one that snapping firmware would act on.
pub const NEAR_AXIS_DEG: f64 = 15.0;
/// A near-axis stroke this close to the axis and this straight is "snapped".
pub const SNAPPED_AXIS_DEG: f64 = 1.0;
pub const SNAPPED_RMS_RESIDUAL: f64 = 0.3;
pub const SNAPPED_MAX_RESIDUAL: f64 = 1.5;
/// Minimum near-axis strokes before a verdict is given.
pub const MIN_NEAR_AXIS_STROKES: usize = 3;
/// Fraction of near-axis strokes that must be snapped to report snapping.
pub const SNAP_FRACTION_THRESHOLD: f64 = 0.5;

/// Fit a line to the cumulative path of a stroke and measure its shape.
///
/// Returns `None` for strokes with fewer than [`MIN_STROKE_EVENTS`] reports
/// or shorter than [`MIN_STROKE_LENGTH`] counts.
pub fn stroke_shape(deltas: &[(i32, i32)]) -> Option<StrokeShape> {
    if deltas.len() < MIN_STROKE_EVENTS {
        return None;
    }
    // Cumulative positions, starting at the origin.
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(deltas.len() + 1);
    let (mut x, mut y, mut length) = (0.0f64, 0.0f64, 0.0f64);
    points.push((x, y));
    for &(dx, dy) in deltas {
        x += dx as f64;
        y += dy as f64;
        length += ((dx as f64).powi(2) + (dy as f64).powi(2)).sqrt();
        points.push((x, y));
    }
    if length < MIN_STROKE_LENGTH {
        return None;
    }

    let angle_deg = y.atan2(x).to_degrees();
    let axis_deviation_deg = {
        let a = angle_deg.abs() % 90.0;
        a.min(90.0 - a)
    };

    // Principal direction of the point cloud (2x2 covariance eigenvector).
    let n = points.len() as f64;
    let (cx, cy) = points
        .iter()
        .fold((0.0, 0.0), |(ax, ay), &(px, py)| (ax + px, ay + py));
    let (cx, cy) = (cx / n, cy / n);
    let (mut sxx, mut syy, mut sxy) = (0.0f64, 0.0f64, 0.0f64);
    for &(px, py) in &points {
        let (ex, ey) = (px - cx, py - cy);
        sxx += ex * ex;
        syy += ey * ey;
        sxy += ex * ey;
    }
    // Direction of the largest eigenvector: angle = 0.5 * atan2(2*sxy, sxx - syy)
    let theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (ux, uy) = (theta.cos(), theta.sin());

    let mut sum_sq = 0.0f64;
    let mut max_residual = 0.0f64;
    for &(px, py) in &points {
        let (ex, ey) = (px - cx, py - cy);
        let perp = (ex * uy - ey * ux).abs();
        sum_sq += perp * perp;
        max_residual = max_residual.max(perp);
    }
    let rms_residual = (sum_sq / n).sqrt();

    Some(StrokeShape {
        length,
        angle_deg,
        axis_deviation_deg,
        rms_residual,
        max_residual,
        events: deltas.len(),
    })
}

fn is_snapped(shape: &StrokeShape) -> bool {
    shape.axis_deviation_deg < SNAPPED_AXIS_DEG
        && shape.rms_residual < SNAPPED_RMS_RESIDUAL
        && shape.max_residual < SNAPPED_MAX_RESIDUAL
}

fn is_near_axis(shape: &StrokeShape) -> bool {
    shape.axis_deviation_deg < NEAR_AXIS_DEG
}

fn nearest_axis(shape: &StrokeShape) -> f64 {
    let a = shape.angle_deg.abs() % 180.0;
    if !(45.0..=135.0).contains(&a) {
        0.0
    } else {
        90.0
    }
}

/// Judge a set of stroke shapes.
pub fn analyze_shapes(shapes: &[StrokeShape]) -> AngleSnapAnalysis {
    let near_axis: Vec<&StrokeShape> = shapes.iter().filter(|s| is_near_axis(s)).collect();
    let snapped: Vec<&StrokeShape> = near_axis
        .iter()
        .copied()
        .filter(|s| is_snapped(s))
        .collect();

    let conclusive = near_axis.len() >= MIN_NEAR_AXIS_STROKES;
    let snap_strength = if near_axis.is_empty() {
        0.0
    } else {
        snapped.len() as f64 / near_axis.len() as f64
    };
    let has_snapping = conclusive && snap_strength >= SNAP_FRACTION_THRESHOLD;

    let mut dominant_angles: Vec<f64> = Vec::new();
    if has_snapping {
        for s in &snapped {
            let axis = nearest_axis(s);
            if !dominant_angles.contains(&axis) {
                dominant_angles.push(axis);
            }
        }
        dominant_angles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }

    AngleSnapAnalysis {
        has_snapping,
        conclusive,
        total_strokes: shapes.len(),
        near_axis_strokes: near_axis.len(),
        snapped_strokes: snapped.len(),
        snap_strength,
        dominant_angles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random wobble in -1..=1 (no external crates).
    fn wobble(seed: &mut u64) -> i32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        ((*seed % 3) as i32) - 1
    }

    /// A freehand near-horizontal stroke: mostly (2,0) with occasional +-1 in y.
    fn human_stroke(seed: u64, events: usize) -> Vec<(i32, i32)> {
        let mut s = seed.max(1);
        (0..events)
            .map(|i| {
                let dy = if i % 4 == 0 { wobble(&mut s) } else { 0 };
                (2, dy)
            })
            .collect()
    }

    #[test]
    fn test_shape_rejects_short_strokes() {
        assert!(stroke_shape(&[(50, 0); 5]).is_none());
        assert!(stroke_shape(&[(1, 0); 30]).is_none()); // only 30 counts long
    }

    #[test]
    fn test_snapped_horizontal_stroke_is_perfectly_straight() {
        let deltas: Vec<(i32, i32)> = vec![(2, 0); 100];
        let shape = stroke_shape(&deltas).unwrap();
        assert!(shape.axis_deviation_deg < 1e-9);
        assert!(shape.rms_residual < 1e-9);
        assert!(shape.max_residual < 1e-9);
        assert!((shape.length - 200.0).abs() < 1e-9);
        assert!(is_snapped(&shape));
    }

    #[test]
    fn test_snapped_vertical_stroke() {
        let deltas: Vec<(i32, i32)> = vec![(0, -3); 60];
        let shape = stroke_shape(&deltas).unwrap();
        assert!((shape.angle_deg - -90.0).abs() < 1e-9);
        assert!(shape.axis_deviation_deg < 1e-9);
        assert!(is_snapped(&shape));
    }

    #[test]
    fn test_human_near_axis_stroke_is_not_snapped() {
        let shape = stroke_shape(&human_stroke(42, 150)).unwrap();
        assert!(shape.axis_deviation_deg < NEAR_AXIS_DEG);
        assert!(
            shape.rms_residual > SNAPPED_RMS_RESIDUAL || shape.max_residual > SNAPPED_MAX_RESIDUAL,
            "freehand wobble must register as residual: rms={} max={}",
            shape.rms_residual,
            shape.max_residual
        );
        assert!(!is_snapped(&shape));
    }

    #[test]
    fn test_diagonal_stroke_geometry() {
        let deltas: Vec<(i32, i32)> = vec![(2, 2); 60];
        let shape = stroke_shape(&deltas).unwrap();
        assert!((shape.angle_deg - 45.0).abs() < 1e-9);
        assert!((shape.axis_deviation_deg - 45.0).abs() < 1e-9);
        assert!(!is_near_axis(&shape));
    }

    #[test]
    fn test_residual_measures_perpendicular_wander() {
        // Straight in x, but y bulges out by 10 in the middle and returns.
        let mut deltas: Vec<(i32, i32)> = Vec::new();
        for i in 0..100 {
            let dy = if i < 25 || (50..75).contains(&i) {
                1
            } else {
                -1
            };
            deltas.push((2, dy));
        }
        let shape = stroke_shape(&deltas).unwrap();
        assert!(shape.max_residual > 5.0);
        assert!(shape.rms_residual > 1.0);
    }

    #[test]
    fn test_analyze_detects_snapping_on_axis_strokes() {
        let shapes: Vec<StrokeShape> = (0..4)
            .map(|_| stroke_shape(&vec![(2, 0); 100]).unwrap())
            .chain((0..2).map(|_| stroke_shape(&vec![(2, 2); 100]).unwrap()))
            .collect();
        let a = analyze_shapes(&shapes);
        assert!(a.conclusive);
        assert!(a.has_snapping);
        assert_eq!(a.near_axis_strokes, 4);
        assert_eq!(a.snapped_strokes, 4);
        assert_eq!(a.dominant_angles, vec![0.0]);
        assert!((a.snap_strength - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_analyze_human_strokes_no_snapping() {
        let shapes: Vec<StrokeShape> = (1..=6)
            .map(|seed| stroke_shape(&human_stroke(seed * 7919, 150)).unwrap())
            .collect();
        let a = analyze_shapes(&shapes);
        assert!(a.conclusive, "six near-axis strokes should be conclusive");
        assert!(!a.has_snapping);
        assert_eq!(a.snapped_strokes, 0);
        assert!(a.dominant_angles.is_empty());
    }

    #[test]
    fn test_analyze_inconclusive_without_near_axis_strokes() {
        let shapes: Vec<StrokeShape> = (0..5)
            .map(|_| stroke_shape(&vec![(2, 2); 100]).unwrap())
            .collect();
        let a = analyze_shapes(&shapes);
        assert!(!a.conclusive);
        assert!(!a.has_snapping);
        assert_eq!(a.total_strokes, 5);
        assert_eq!(a.near_axis_strokes, 0);
    }

    #[test]
    fn test_analyze_reports_both_axes() {
        let shapes: Vec<StrokeShape> = (0..2)
            .map(|_| stroke_shape(&vec![(2, 0); 100]).unwrap())
            .chain((0..2).map(|_| stroke_shape(&vec![(0, 2); 100]).unwrap()))
            .collect();
        let a = analyze_shapes(&shapes);
        assert!(a.has_snapping);
        assert_eq!(a.dominant_angles, vec![0.0, 90.0]);
    }
}
