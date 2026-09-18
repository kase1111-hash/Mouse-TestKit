//! Test panel modules for Mouse TRAP GUI
//!
//! Each panel provides a self-contained UI and logic for a specific mouse test.
//! Panels handle user interaction, data collection, analysis, and result export.
//!
//! # Panel Types
//!
//! - [`PollingPanel`] - Real-time polling rate measurement
//! - [`StutterPanel`] - Movement stutter and timing irregularity detection
//! - [`ClickPanel`] - Click response, stickiness, and lift-off tests
//! - [`JitterPanel`] - Sensor jitter analysis when mouse is stationary
//! - [`DpiPanel`] - DPI accuracy verification
//! - [`AccelPanel`] - Acceleration and angle snapping detection
//! - [`DoubleClickPanel`] - Switch health and double-click detection
//! - [`ScrollPanel`] - Scroll wheel consistency testing
//! - [`AutoTestPanel`] - Guided automated diagnostics that look for faults

mod accel;
mod auto_test;
mod click;
mod double_click;
mod dpi;
mod jitter;
mod polling;
mod scroll;
mod stutter;

pub use accel::AccelPanel;
pub use auto_test::AutoTestPanel;
pub use click::ClickPanel;
pub use double_click::DoubleClickPanel;
pub use dpi::DpiPanel;
pub use jitter::JitterPanel;
pub use polling::PollingPanel;
pub use scroll::ScrollPanel;
pub use stutter::StutterPanel;

/// Headless rendering smoke tests.
///
/// Every panel is driven through a real `egui::Context` for several frames
/// with no display attached, so the UI code paths (layout, plots, result
/// tables) are exercised on every `cargo test` run. Synthetic raw input is
/// fed to the panels that consume it, to reach the "results" branches.
#[cfg(test)]
mod smoke_tests {
    use super::*;
    use crate::input_bridge::{RawButton, RawInputEvent, RawInputKind};
    use eframe::egui;
    use std::time::{Duration, Instant};

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        // Provide fonts/screen so text layout works headless
        ctx.set_fonts(egui::FontDefinitions::default());
        ctx
    }

    fn frame(ctx: &egui::Context, mut body: impl FnMut(&mut egui::Ui, &egui::Context)) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            body(ui, &ctx);
        });
    }

    fn moves(n: usize, dx: i32, dy: i32, interval: Duration, start: Instant) -> Vec<RawInputEvent> {
        (0..n)
            .map(|i| RawInputEvent {
                kind: RawInputKind::Move { dx, dy },
                timestamp: start + interval * i as u32,
            })
            .collect()
    }

    fn click(start: Instant, hold: Duration) -> Vec<RawInputEvent> {
        vec![
            RawInputEvent {
                kind: RawInputKind::ButtonPress(RawButton::Left),
                timestamp: start,
            },
            RawInputEvent {
                kind: RawInputKind::ButtonRelease(RawButton::Left),
                timestamp: start + hold,
            },
        ]
    }

    #[test]
    fn all_panels_render_idle_with_and_without_bridge() {
        let ctx = ctx();
        let mut polling = PollingPanel::new();
        let mut stutter = StutterPanel::new();
        let mut click = ClickPanel::new();
        let mut jitter = JitterPanel::new();
        let mut dpi = DpiPanel::new();
        let mut accel = AccelPanel::new();
        let mut double_click = DoubleClickPanel::new();
        let mut scroll = ScrollPanel::new();
        let mut auto = AutoTestPanel::new();
        for has_bridge in [true, false] {
            for _ in 0..2 {
                frame(&ctx, |ui, ctx| {
                    polling.ui(ui, ctx, &[], has_bridge);
                    stutter.ui(ui, ctx, &[], has_bridge);
                    click.ui_response(ui, ctx, &[], has_bridge);
                    click.ui_sticky(ui, ctx, &[], has_bridge);
                    click.ui_liftoff(ui, ctx, &[], has_bridge);
                    jitter.ui(ui, ctx, &[], has_bridge);
                    dpi.ui(ui, ctx, &[], has_bridge);
                    accel.ui_accel(ui, ctx, &[], has_bridge);
                    accel.ui_angle(ui, ctx, &[], has_bridge);
                    double_click.ui(ui, ctx, &[], has_bridge);
                    scroll.ui(ui, ctx, &[], has_bridge);
                    auto.ui(ui, ctx, has_bridge);
                });
            }
        }
        assert!(polling.export().is_none());
        assert!(auto.report().is_none());
    }

    #[test]
    fn polling_panel_reports_median_rate_from_raw_events() {
        let ctx = ctx();
        let mut polling = PollingPanel::new();
        // Press Start programmatically by rendering with a click is awkward;
        // use the internal start path through the public API instead.
        polling.start_for_test();
        let start = Instant::now() - Duration::from_millis(600);
        let events = moves(500, 1, 0, Duration::from_millis(1), start);
        frame(&ctx, |ui, ctx| polling.ui(ui, ctx, &events, true));
        let export = polling.export().expect("a Hz sample after 500 reports");
        assert!(
            (990..=1010).contains(&export.current_hz),
            "expected ~1000 Hz, got {}",
            export.current_hz
        );
        // A partially filled window must not drag min_hz down
        assert!(export.min_hz >= 990, "min_hz polluted: {}", export.min_hz);
    }

    #[test]
    fn auto_test_panel_runs_to_a_report_with_synthetic_input() {
        let ctx = ctx();
        let mut auto = AutoTestPanel::new();
        auto.start_for_test(true);
        assert!(auto.is_running());
        // Virtual clock: every timestamp is `epoch + t` with `t` in seconds.
        let epoch = auto.epoch_for_test();
        let at = |t: f64| epoch + Duration::from_secs_f64(t);

        // Rest phase: nothing happens, wait it out
        auto.tick_for_test(6.2);
        frame(&ctx, |ui, ctx| {
            auto.process_input(ctx, &[], true);
            auto.ui(ui, ctx, true);
        });

        // Movement: 1000 Hz with a stall every 25th report (4% stutters)
        let mut t = 6.3;
        let mut events = Vec::new();
        for i in 0..3100 {
            t += if i % 25 == 0 { 0.008 } else { 0.001 };
            events.push(RawInputEvent {
                kind: RawInputKind::Move { dx: 2, dy: 1 },
                timestamp: at(t),
            });
        }
        frame(&ctx, |ui, ctx| {
            auto.process_input(ctx, &events, true);
            auto.ui(ui, ctx, true);
        });

        // Clicks: 20 left with a bounce after every 5th, 5 right
        let mut events = Vec::new();
        for i in 0..20 {
            t += 0.2;
            events.extend(click(at(t), Duration::from_millis(60)));
            t += 0.06;
            if i % 5 == 0 {
                t += 0.005;
                events.extend(click(at(t), Duration::from_millis(2)));
                t += 0.002;
            }
        }
        for _ in 0..5 {
            t += 0.2;
            events.push(RawInputEvent {
                kind: RawInputKind::ButtonPress(RawButton::Right),
                timestamp: at(t),
            });
            t += 0.07;
            events.push(RawInputEvent {
                kind: RawInputKind::ButtonRelease(RawButton::Right),
                timestamp: at(t),
            });
        }
        frame(&ctx, |ui, ctx| {
            auto.process_input(ctx, &events, true);
            auto.ui(ui, ctx, true);
        });

        // Scroll: 10 down, 10 up
        let mut events = Vec::new();
        for delta in [-1; 10].into_iter().chain([1; 10]) {
            t += 0.09;
            events.push(RawInputEvent {
                kind: RawInputKind::Scroll { delta },
                timestamp: at(t),
            });
        }
        frame(&ctx, |ui, ctx| {
            auto.process_input(ctx, &events, true);
            auto.ui(ui, ctx, true);
        });

        // Lift: wait it out
        auto.tick_for_test(t + 10.5);
        frame(&ctx, |ui, ctx| {
            auto.process_input(ctx, &[], true);
            auto.ui(ui, ctx, true);
        });

        assert!(!auto.is_running());
        let report = auto.report().expect("report after all phases");
        assert_eq!(report.metrics.polling_hz, Some(1000));
        assert_eq!(report.metrics.left_clicks, 24);
        assert_eq!(report.metrics.right_clicks, 5);
        assert!(report.metrics.bounce_events >= 4);
        assert!(report.metrics.stutter_count > 0);
        assert_eq!(report.metrics.scroll_up, 10);
        assert_eq!(report.metrics.scroll_down, 10);
        assert_eq!(report.phases_completed.len(), 5);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.title.starts_with("Switch bounce")),
            "{:?}",
            report.findings.iter().map(|f| &f.title).collect::<Vec<_>>()
        );

        // Report view (with metrics expanded) renders
        auto.set_show_metrics_for_test(true);
        frame(&ctx, |ui, ctx| auto.ui(ui, ctx, true));
    }

    #[test]
    fn accel_panel_detects_acceleration_from_synthetic_passes() {
        let ctx = ctx();
        let mut accel = AccelPanel::new();
        accel.start_accel_for_test();
        let mut t = Instant::now();
        let mut events = Vec::new();
        // 5 slow passes of 800 counts (4 counts/ms over 200 ms), 5 fast passes of
        // 1200 counts (12 counts/ms over 100 ms): 1.5x counts -> acceleration
        for pass in 0..10 {
            let (per_report, reports) = if pass < 5 { (4, 200) } else { (12, 100) };
            for _ in 0..reports {
                t += Duration::from_millis(1);
                events.push(RawInputEvent {
                    kind: RawInputKind::Move {
                        dx: per_report,
                        dy: 0,
                    },
                    timestamp: t,
                });
            }
            t += Duration::from_millis(400); // pause between passes
        }
        frame(&ctx, |ui, ctx| accel.ui_accel(ui, ctx, &events, true));
        accel.stop_accel_for_test();
        frame(&ctx, |ui, ctx| accel.ui_accel(ui, ctx, &[], true));
        let export = accel.export_accel().expect("verdict after 10 passes");
        assert!(export.conclusive);
        assert!(export.has_acceleration, "factor {}", export.accel_factor);
        assert!((export.accel_factor - 1.5).abs() < 0.05);
        assert_eq!(export.slow_sample_count, 5);
        assert_eq!(export.fast_sample_count, 5);
    }

    #[test]
    fn accel_panel_no_acceleration_on_a_linear_mouse() {
        let ctx = ctx();
        let mut accel = AccelPanel::new();
        accel.start_accel_for_test();
        let mut t = Instant::now();
        let mut events = Vec::new();
        // Same 800 counts per pass whether slow (4/ms) or fast (16/ms)
        for pass in 0..10 {
            let (per_report, reports) = if pass < 5 { (4, 200) } else { (16, 50) };
            for _ in 0..reports {
                t += Duration::from_millis(1);
                events.push(RawInputEvent {
                    kind: RawInputKind::Move {
                        dx: per_report,
                        dy: 1,
                    },
                    timestamp: t,
                });
            }
            t += Duration::from_millis(400);
        }
        frame(&ctx, |ui, ctx| accel.ui_accel(ui, ctx, &events, true));
        accel.stop_accel_for_test();
        let export = accel.export_accel().unwrap();
        assert!(export.conclusive);
        assert!(!export.has_acceleration, "factor {}", export.accel_factor);
    }

    #[test]
    fn angle_panel_flags_snapped_strokes_only() {
        let ctx = ctx();
        let mut accel = AccelPanel::new();
        accel.start_angle_for_test();
        let mut t = Instant::now();
        let mut events = Vec::new();
        // 4 perfectly horizontal 300-count strokes
        for _ in 0..4 {
            for _ in 0..150 {
                t += Duration::from_millis(1);
                events.push(RawInputEvent {
                    kind: RawInputKind::Move { dx: 2, dy: 0 },
                    timestamp: t,
                });
            }
            t += Duration::from_millis(400);
        }
        frame(&ctx, |ui, ctx| accel.ui_angle(ui, ctx, &events, true));
        accel.stop_angle_for_test();
        frame(&ctx, |ui, ctx| accel.ui_angle(ui, ctx, &[], true));
        let export = accel.export_angle().unwrap();
        assert!(export.conclusive);
        assert!(export.has_snapping);
        assert_eq!(export.snapped_stroke_count, 4);
        assert_eq!(export.dominant_angles, vec![0.0]);

        // Freehand strokes with wobble: no snapping
        let mut accel = AccelPanel::new();
        accel.start_angle_for_test();
        let mut events = Vec::new();
        for s in 0..5u32 {
            for i in 0..150u32 {
                t += Duration::from_millis(1);
                // Wander up for the first half of the stroke, back down after
                let dy = if i % 9 == s % 9 {
                    if i < 75 {
                        1
                    } else {
                        -1
                    }
                } else {
                    0
                };
                events.push(RawInputEvent {
                    kind: RawInputKind::Move { dx: 2, dy },
                    timestamp: t,
                });
            }
            t += Duration::from_millis(400);
        }
        frame(&ctx, |ui, ctx| accel.ui_angle(ui, ctx, &events, true));
        accel.stop_angle_for_test();
        let export = accel.export_angle().unwrap();
        assert!(export.conclusive);
        assert!(!export.has_snapping);
        assert_eq!(export.snapped_stroke_count, 0);
    }

    #[test]
    fn double_click_panel_counts_bounce_from_raw_presses() {
        let ctx = ctx();
        let mut panel = DoubleClickPanel::new();
        panel.start_for_test();
        let t = Instant::now();
        let mut events = click(t, Duration::from_millis(60));
        events.extend(click(
            t + Duration::from_millis(65),
            Duration::from_millis(2),
        ));
        events.extend(click(
            t + Duration::from_millis(400),
            Duration::from_millis(60),
        ));
        frame(&ctx, |ui, ctx| panel.ui(ui, ctx, &events, true));
        let export = panel.export().unwrap();
        assert_eq!(export.total_clicks, 3);
        assert_eq!(export.accidental_double_clicks, 1);
    }

    #[test]
    fn scroll_and_stutter_panels_consume_raw_events() {
        let ctx = ctx();
        let mut scroll = ScrollPanel::new();
        let mut stutter = StutterPanel::new();
        scroll.start_for_test();
        stutter.start_for_test();
        let start = Instant::now() - Duration::from_millis(300);
        let mut events = moves(200, 1, 1, Duration::from_millis(1), start);
        for i in 0..25 {
            events.push(RawInputEvent {
                kind: RawInputKind::Scroll {
                    delta: if i < 15 { -1 } else { 1 },
                },
                timestamp: start + Duration::from_millis(10 * i),
            });
        }
        frame(&ctx, |ui, ctx| {
            scroll.ui(ui, ctx, &events, true);
            stutter.ui(ui, ctx, &events, true);
        });
        let s = scroll.export().unwrap();
        assert_eq!(s.total_steps, 25);
        assert_eq!(s.direction_changes, 1);
        let st = stutter.export().unwrap();
        assert_eq!(st.total_samples, 199);
        assert!((st.polling_rate_hz - 1000.0).abs() < 20.0);
    }
}
