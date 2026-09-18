//! Test results export module
//!
//! Provides data structures and serialization for exporting test results
//! to JSON and CSV formats. Each test type has a corresponding export struct
//! containing all relevant metrics and raw data.

use chrono::{DateTime, Local};
use mouse_testkit::analysis::diagnostics::Report as AutoDiagnosticsReport;
use serde::Serialize;

/// Complete export of all test results.
///
/// Contains optional results from each test type. Tests that haven't been
/// run will have `None` values. Can be serialized to JSON or CSV format.
#[derive(Serialize)]
pub struct TestResultsExport {
    pub export_info: ExportInfo,
    pub polling_rate: Option<PollingRateExport>,
    pub stutter: Option<StutterExport>,
    pub click_response: Option<ClickResponseExport>,
    pub click_sticky: Option<ClickStickyExport>,
    pub liftoff: Option<LiftOffExport>,
    pub jitter: Option<JitterExport>,
    pub double_click: Option<DoubleClickExport>,
    pub dpi: Option<DpiExport>,
    pub acceleration: Option<AccelerationExport>,
    pub angle_snap: Option<AngleSnapExport>,
    pub scroll: Option<ScrollExport>,
    /// Report from the automated diagnostics run, if one was completed.
    pub auto_diagnostics: Option<AutoDiagnosticsReport>,
}

/// Metadata about the export including app version and timestamp.
#[derive(Serialize)]
pub struct ExportInfo {
    pub app_name: String,
    pub app_version: String,
    pub export_time: String,
    pub platform: String,
    pub arch: String,
}

impl ExportInfo {
    pub fn new() -> Self {
        let now: DateTime<Local> = Local::now();
        Self {
            app_name: "Mouse TRAP".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            export_time: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            platform: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

/// Polling rate test results with statistics and history.
#[derive(Serialize)]
pub struct PollingRateExport {
    pub current_hz: u32,
    pub min_hz: u32,
    pub max_hz: u32,
    pub avg_hz: f64,
    pub samples: u32,
    pub history: Vec<f64>,
}

/// Stutter detection results with timing deltas and stutter events.
#[derive(Serialize)]
pub struct StutterExport {
    pub total_stutter_count: usize,
    pub total_samples: usize,
    pub avg_delta_ms: f64,
    pub min_delta_ms: f64,
    pub max_delta_ms: f64,
    pub polling_rate_hz: f64,
    pub threshold_multiplier: f64,
    pub stutter_rate_percent: f64,
    pub deltas: Vec<f64>,
}

/// Click response test results for left and right buttons.
#[derive(Serialize)]
pub struct ClickResponseExport {
    pub left: ClickButtonExport,
    pub right: ClickButtonExport,
}

/// Click metrics for a single button (CPS, hold times).
#[derive(Serialize)]
pub struct ClickButtonExport {
    pub click_count: usize,
    pub cps: f64,
    pub avg_hold_ms: f64,
    pub min_hold_ms: f64,
    pub max_hold_ms: f64,
    pub hold_times: Vec<f64>,
}

/// Click stickiness test results (buttons failing to release properly).
#[derive(Serialize)]
pub struct ClickStickyExport {
    pub left: StickyButtonExport,
    pub right: StickyButtonExport,
}

/// Stickiness metrics for a single button.
#[derive(Serialize)]
pub struct StickyButtonExport {
    pub click_count: usize,
    pub sticky_count: usize,
    pub avg_hold_ms: f64,
    pub max_hold_ms: f64,
    pub hold_times: Vec<f64>,
}

/// Lift-off distance test results (cursor jump when lifting mouse).
#[derive(Serialize)]
pub struct LiftOffExport {
    pub jump_count: usize,
    pub avg_distance_px: f64,
    pub max_distance_px: f64,
    pub jump_distances: Vec<f64>,
}

/// Jitter test results (sensor noise when mouse is stationary).
#[derive(Serialize)]
pub struct JitterExport {
    pub sample_count: usize,
    pub avg_events: f64,
    pub avg_distance_px: f64,
    pub max_jitter_px: f64,
    pub rating: String,
    pub samples: Vec<JitterSampleExport>,
}

/// Individual jitter sample measurement.
#[derive(Serialize)]
pub struct JitterSampleExport {
    pub events: usize,
    pub total_distance: f64,
    pub max_single: f64,
}

/// Double-click test results (detecting switch issues).
#[derive(Serialize)]
pub struct DoubleClickExport {
    pub total_clicks: usize,
    pub double_click_count: u32,
    pub accidental_double_clicks: u32,
    pub avg_interval_ms: f64,
    pub min_interval_ms: f64,
    pub max_interval_ms: f64,
    pub threshold_ms: f64,
    pub consistency_percent: f64,
    pub intervals: Vec<f64>,
}

/// DPI accuracy test results.
#[derive(Serialize)]
pub struct DpiExport {
    pub target_dpi: u32,
    pub samples: Vec<DpiSampleExport>,
    pub avg_accuracy_percent: f32,
}

/// Individual DPI measurement sample.
#[derive(Serialize)]
pub struct DpiSampleExport {
    pub target_dpi: u32,
    pub measured_dpi: f32,
    pub accuracy_percent: f32,
}

/// Acceleration detection results (pointer acceleration enabled).
#[derive(Serialize)]
pub struct AccelerationExport {
    pub has_acceleration: bool,
    /// False when the slow and fast passes were too similar in speed to judge.
    pub conclusive: bool,
    /// Mean counts of fast passes divided by mean counts of slow passes.
    pub accel_factor: f64,
    /// Median speed of fast passes divided by median speed of slow passes.
    pub speed_ratio: f64,
    pub confidence_percent: f32,
    pub slow_sample_count: usize,
    pub fast_sample_count: usize,
}

/// Angle snapping detection results (prediction/smoothing enabled).
#[derive(Serialize)]
pub struct AngleSnapExport {
    pub has_snapping: bool,
    /// False when too few near-axis strokes were drawn to judge.
    pub conclusive: bool,
    pub snap_strength_percent: f64,
    pub dominant_angles: Vec<f64>,
    pub stroke_count: usize,
    pub near_axis_stroke_count: usize,
    pub snapped_stroke_count: usize,
}

/// Scroll wheel test results.
#[derive(Serialize)]
pub struct ScrollExport {
    pub total_steps: usize,
    pub scroll_up: f32,
    pub scroll_down: f32,
    pub direction_changes: usize,
    pub avg_speed_sps: f64,
    pub consistency_percent: u32,
}

/// Escape a string value for safe CSV embedding (RFC 4180).
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

impl TestResultsExport {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn to_csv(&self) -> String {
        let mut csv = String::new();

        // Header
        csv.push_str("Test,Metric,Value\n");

        // Export info — string values are escaped
        csv.push_str(&format!(
            "Info,App Name,{}\n",
            csv_escape(&self.export_info.app_name)
        ));
        csv.push_str(&format!(
            "Info,Version,{}\n",
            csv_escape(&self.export_info.app_version)
        ));
        csv.push_str(&format!(
            "Info,Export Time,{}\n",
            csv_escape(&self.export_info.export_time)
        ));
        csv.push_str(&format!(
            "Info,Platform,{}\n",
            csv_escape(&self.export_info.platform)
        ));

        // Polling rate
        if let Some(ref p) = self.polling_rate {
            csv.push_str(&format!("Polling Rate,Current Hz,{}\n", p.current_hz));
            csv.push_str(&format!("Polling Rate,Min Hz,{}\n", p.min_hz));
            csv.push_str(&format!("Polling Rate,Max Hz,{}\n", p.max_hz));
            csv.push_str(&format!("Polling Rate,Avg Hz,{:.1}\n", p.avg_hz));
            csv.push_str(&format!("Polling Rate,Samples,{}\n", p.samples));
        }

        // Stutter
        if let Some(ref s) = self.stutter {
            csv.push_str(&format!(
                "Stutter,Total Stutters,{}\n",
                s.total_stutter_count
            ));
            csv.push_str(&format!("Stutter,Total Samples,{}\n", s.total_samples));
            csv.push_str(&format!("Stutter,Avg Delta (ms),{:.2}\n", s.avg_delta_ms));
            csv.push_str(&format!("Stutter,Min Delta (ms),{:.2}\n", s.min_delta_ms));
            csv.push_str(&format!("Stutter,Max Delta (ms),{:.2}\n", s.max_delta_ms));
            csv.push_str(&format!(
                "Stutter,Polling Rate (Hz),{:.0}\n",
                s.polling_rate_hz
            ));
            csv.push_str(&format!(
                "Stutter,Stutter Rate (%),{:.1}\n",
                s.stutter_rate_percent
            ));
        }

        // Click Response
        if let Some(ref c) = self.click_response {
            csv.push_str(&format!(
                "Click Response (Left),Click Count,{}\n",
                c.left.click_count
            ));
            csv.push_str(&format!("Click Response (Left),CPS,{:.1}\n", c.left.cps));
            csv.push_str(&format!(
                "Click Response (Left),Avg Hold (ms),{:.1}\n",
                c.left.avg_hold_ms
            ));
            csv.push_str(&format!(
                "Click Response (Right),Click Count,{}\n",
                c.right.click_count
            ));
            csv.push_str(&format!("Click Response (Right),CPS,{:.1}\n", c.right.cps));
            csv.push_str(&format!(
                "Click Response (Right),Avg Hold (ms),{:.1}\n",
                c.right.avg_hold_ms
            ));
        }

        // Click Sticky
        if let Some(ref c) = self.click_sticky {
            csv.push_str(&format!(
                "Click Sticky (Left),Click Count,{}\n",
                c.left.click_count
            ));
            csv.push_str(&format!(
                "Click Sticky (Left),Sticky Count,{}\n",
                c.left.sticky_count
            ));
            csv.push_str(&format!(
                "Click Sticky (Right),Click Count,{}\n",
                c.right.click_count
            ));
            csv.push_str(&format!(
                "Click Sticky (Right),Sticky Count,{}\n",
                c.right.sticky_count
            ));
        }

        // Lift-off
        if let Some(ref l) = self.liftoff {
            csv.push_str(&format!("Lift-Off,Jump Count,{}\n", l.jump_count));
            csv.push_str(&format!(
                "Lift-Off,Avg Distance (px),{:.1}\n",
                l.avg_distance_px
            ));
            csv.push_str(&format!(
                "Lift-Off,Max Distance (px),{:.1}\n",
                l.max_distance_px
            ));
        }

        // Jitter
        if let Some(ref j) = self.jitter {
            csv.push_str(&format!("Jitter,Sample Count,{}\n", j.sample_count));
            csv.push_str(&format!("Jitter,Avg Events,{:.1}\n", j.avg_events));
            csv.push_str(&format!(
                "Jitter,Avg Distance (px),{:.2}\n",
                j.avg_distance_px
            ));
            csv.push_str(&format!("Jitter,Max Jitter (px),{:.2}\n", j.max_jitter_px));
            csv.push_str(&format!("Jitter,Rating,{}\n", csv_escape(&j.rating)));
        }

        // Double-click
        if let Some(ref d) = self.double_click {
            csv.push_str(&format!("Double-Click,Total Clicks,{}\n", d.total_clicks));
            csv.push_str(&format!(
                "Double-Click,Double-Clicks,{}\n",
                d.double_click_count
            ));
            csv.push_str(&format!(
                "Double-Click,Accidental,{}\n",
                d.accidental_double_clicks
            ));
            csv.push_str(&format!(
                "Double-Click,Avg Interval (ms),{:.1}\n",
                d.avg_interval_ms
            ));
            csv.push_str(&format!(
                "Double-Click,Consistency (%),{:.0}\n",
                d.consistency_percent
            ));
        }

        // DPI
        if let Some(ref d) = self.dpi {
            csv.push_str(&format!("DPI,Target DPI,{}\n", d.target_dpi));
            csv.push_str(&format!(
                "DPI,Avg Accuracy (%),{:.1}\n",
                d.avg_accuracy_percent
            ));
            for (i, sample) in d.samples.iter().enumerate() {
                csv.push_str(&format!(
                    "DPI,Sample {} Measured,{:.0}\n",
                    i + 1,
                    sample.measured_dpi
                ));
                csv.push_str(&format!(
                    "DPI,Sample {} Accuracy (%),{:.1}\n",
                    i + 1,
                    sample.accuracy_percent
                ));
            }
        }

        // Acceleration
        if let Some(ref a) = self.acceleration {
            csv.push_str(&format!("Acceleration,Detected,{}\n", a.has_acceleration));
            csv.push_str(&format!("Acceleration,Conclusive,{}\n", a.conclusive));
            csv.push_str(&format!("Acceleration,Factor,{:.2}\n", a.accel_factor));
            csv.push_str(&format!("Acceleration,Speed Ratio,{:.2}\n", a.speed_ratio));
            csv.push_str(&format!(
                "Acceleration,Confidence (%),{:.0}\n",
                a.confidence_percent
            ));
        }

        // Angle Snap
        if let Some(ref a) = self.angle_snap {
            csv.push_str(&format!("Angle Snap,Detected,{}\n", a.has_snapping));
            csv.push_str(&format!("Angle Snap,Conclusive,{}\n", a.conclusive));
            csv.push_str(&format!(
                "Angle Snap,Strength (%),{:.1}\n",
                a.snap_strength_percent
            ));
            csv.push_str(&format!("Angle Snap,Strokes,{}\n", a.stroke_count));
        }

        // Scroll
        if let Some(ref s) = self.scroll {
            csv.push_str(&format!("Scroll,Total Steps,{}\n", s.total_steps));
            csv.push_str(&format!("Scroll,Up,{:.0}\n", s.scroll_up));
            csv.push_str(&format!("Scroll,Down,{:.0}\n", s.scroll_down));
            csv.push_str(&format!(
                "Scroll,Direction Changes,{}\n",
                s.direction_changes
            ));
            csv.push_str(&format!("Scroll,Avg Speed (sps),{:.1}\n", s.avg_speed_sps));
            csv.push_str(&format!(
                "Scroll,Consistency (%),{}\n",
                s.consistency_percent
            ));
        }

        // Auto diagnostics
        if let Some(ref r) = self.auto_diagnostics {
            csv.push_str(&format!(
                "Auto Diagnostics,Verdict,{}\n",
                csv_escape(r.verdict.label())
            ));
            csv.push_str(&format!("Auto Diagnostics,Aborted,{}\n", r.aborted));
            csv.push_str(&format!(
                "Auto Diagnostics,Duration (s),{:.1}\n",
                r.duration_secs
            ));
            if let Some(hz) = r.metrics.polling_hz {
                csv.push_str(&format!("Auto Diagnostics,Polling Rate (Hz),{}\n", hz));
            }
            csv.push_str(&format!(
                "Auto Diagnostics,Stutter Rate (%),{:.2}\n",
                r.metrics.stutter_rate_percent
            ));
            csv.push_str(&format!(
                "Auto Diagnostics,Rest Movement (counts),{:.1}\n",
                r.metrics.rest_distance_counts
            ));
            csv.push_str(&format!(
                "Auto Diagnostics,Switch Bounces,{}\n",
                r.metrics.bounce_events
            ));
            csv.push_str(&format!(
                "Auto Diagnostics,Scroll Reversals,{}\n",
                r.metrics.scroll_reversals
            ));
            csv.push_str(&format!(
                "Auto Diagnostics,Lift Jumps,{}\n",
                r.metrics.lift_jumps
            ));
            for f in &r.findings {
                csv.push_str(&format!(
                    "Auto Diagnostics,{},{}\n",
                    csv_escape(&format!("{}: {}", f.severity.label(), f.title)),
                    csv_escape(&f.detail)
                ));
            }
        }

        csv
    }
}

/// Helper to save export to file
#[allow(dead_code)]
pub fn save_to_file(content: &str, path: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_escape_plain_string() {
        assert_eq!(csv_escape("hello"), "hello");
    }

    #[test]
    fn test_csv_escape_empty_string() {
        assert_eq!(csv_escape(""), "");
    }

    #[test]
    fn test_csv_escape_contains_comma() {
        assert_eq!(csv_escape("one,two"), "\"one,two\"");
    }

    #[test]
    fn test_csv_escape_contains_double_quote() {
        // Input: say "hi"  →  Output: "say ""hi"""
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn test_csv_escape_contains_newline() {
        assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn test_csv_escape_comma_and_quote_combined() {
        // Input: a,b"c  →  Output: "a,b""c"
        assert_eq!(csv_escape("a,b\"c"), "\"a,b\"\"c\"");
    }

    #[test]
    fn test_csv_includes_auto_diagnostics_findings_escaped() {
        use mouse_testkit::analysis::diagnostics::{
            Finding, Metrics, Phase, Report, Severity, Verdict,
        };
        let export = TestResultsExport {
            export_info: ExportInfo::new(),
            polling_rate: None,
            stutter: None,
            click_response: None,
            click_sticky: None,
            liftoff: None,
            jitter: None,
            double_click: None,
            dpi: None,
            acceleration: None,
            angle_snap: None,
            scroll: None,
            auto_diagnostics: Some(Report {
                verdict: Verdict::Attention,
                findings: vec![Finding {
                    severity: Severity::Warning,
                    phase: Phase::Clicks,
                    title: "Possible switch bounce".to_string(),
                    detail: "1 press came within 25 ms, re-run".to_string(),
                }],
                metrics: Metrics::default(),
                phases_completed: vec![Phase::Rest],
                aborted: false,
                timing_reliable: true,
                duration_secs: 42.0,
            }),
        };
        let csv = export.to_csv();
        assert!(csv.contains("Auto Diagnostics,Verdict,Needs attention\n"));
        assert!(csv.contains(
            "Auto Diagnostics,WARNING: Possible switch bounce,\"1 press came within 25 ms, re-run\"\n"
        ));
        assert!(export
            .to_json()
            .unwrap()
            .contains("\"verdict\": \"Attention\""));
    }

    #[test]
    fn test_csv_escape_carriage_return_not_handled() {
        // Documents that bare \r does NOT trigger quoting (current behavior)
        assert_eq!(csv_escape("a\rb"), "a\rb");
    }

    #[test]
    fn test_csv_escape_formula_injection_not_handled() {
        // Documents that formula-injection prefixes are NOT quoted (current behavior).
        // A future hardening pass may choose to address this.
        assert_eq!(csv_escape("=cmd"), "=cmd");
        assert_eq!(csv_escape("+cmd"), "+cmd");
        assert_eq!(csv_escape("-cmd"), "-cmd");
        assert_eq!(csv_escape("@cmd"), "@cmd");
    }
}
