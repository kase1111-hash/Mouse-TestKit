//! Stutter analysis
//!
//! Detects movement irregularities by analyzing deviation from the mean
//! inter-event delta time.

/// Analyze a slice of delta times (ms) and return detected stutter events.
///
/// A stutter is any delta that deviates from the mean by more than 2 ms.
/// Severity is classified as Minor (>2ms), Moderate (>4ms), or Severe (>8ms).
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

/// A single detected stutter event.
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

/// Severity classification for a stutter event.
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
