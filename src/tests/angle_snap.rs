//! Angle Snapping Detection
//! Detects if mouse has angle snapping/prediction enabled
//!
//! CLI front-end for `mouse_testkit::analysis::angle_snap`, the analysis the
//! GUI uses. Snapping firmware straightens near-horizontal and near-vertical
//! movement onto the axis; it leaves diagonals alone, so the user draws
//! slightly tilted near-axis lines. Motion (one move per hardware report) is
//! split into lines at pauses, a straight line is fitted to each, and lines
//! that come out exactly on the axis with no wobble at all - which a hand
//! cannot draw - are the signature of snapping.

use super::standard::{self, TestOutcome};
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use mouse_testkit::analysis::angle_snap::{
    analyze_shapes, stroke_shape, AngleSnapAnalysis, StrokeShape, MIN_NEAR_AXIS_STROKES,
    MIN_STROKE_EVENTS, MIN_STROKE_LENGTH, NEAR_AXIS_DEG,
};
use mouse_testkit::analysis::strokes::{Stroke, StrokeSegmenter};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

/// A pause longer than this ends one line (same as the GUI).
const STROKE_GAP_S: f64 = 0.25;

pub fn run() -> TestOutcome {
    println!("\n=== Angle Snapping Detection ===");
    println!("This test detects if your mouse has angle snapping (prediction) enabled.");
    println!("Snapping forces almost-horizontal or almost-vertical movement onto a");
    println!("perfectly straight axis line. A hand cannot draw a perfectly straight line,");
    println!("so a slightly tilted freehand line that comes out exactly on the axis with");
    println!("zero wobble is the signature of snapping.");
    println!("\nInstructions:");
    println!("  1. Draw freehand lines that are ALMOST horizontal or vertical,");
    println!("     tilted just a little (about 5 degrees). Do not use a ruler or pad edge");
    println!("  2. Make each line a few centimetres long, at a steady moderate speed");
    println!("  3. Pause briefly between lines; draw 5 or more, then press 'q'");
    println!(
        "  Lines must be within {:.0} degrees of an axis; {} such lines give a verdict.\n",
        NEAR_AXIS_DEG, MIN_NEAR_AXIS_STROKES
    );

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return TestOutcome::NoDevice;
        }
    };

    terminal::grab_device(&mut device);

    let start = Instant::now();
    let secs = |t: Instant| t.saturating_duration_since(start).as_secs_f64();
    let mut recorder = LineRecorder::new();
    let mut line_no = 0usize;
    let mut last_print = Instant::now();
    let mut device_lost = false;

    let _guard = terminal::TerminalGuard::new();

    println!("Draw lines... Press 'r' to start over, 'q' to finish.\n");

    loop {
        // The wait happens on the mouse below
        if event::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('r') => {
                            recorder.reset();
                            line_no = 0;
                            println!("\r\x1B[KReset. Draw new lines...");
                        }
                        _ => {}
                    }
                }
            }
        }

        let Ok(batch) = input::read_events(&mut device, terminal::READ_WAIT) else {
            device_lost = true;
            break;
        };
        if batch.lost_data && recorder.drop_line() {
            // Dropped reports cut a piece out of the line
            println!("\r\x1B[KSome movement was lost (the computer fell behind) - that line was dropped.");
        }
        let mut finished = Vec::new();
        for timed in batch.events {
            if let MouseEvent::Move { dx, dy } = timed.event {
                finished.extend(recorder.feed(secs(timed.time), dx, dy));
            }
        }
        finished.extend(recorder.tick(secs(Instant::now())));
        for line in finished {
            line_no += 1;
            println!("\r\x1B[KLine {}: {}", line_no, describe(&line));
        }

        if last_print.elapsed() >= Duration::from_millis(100) {
            print!("\r\x1B[K");
            print!(
                "Lines: {} | near an axis: {} | too short: {} | 'q' to finish",
                recorder.lines_seen(),
                recorder.near_axis(),
                recorder.too_short
            );
            io::stdout().flush().ok();
            last_print = Instant::now();
        }
    }

    // A line cut off by the mouse disappearing is not judged
    if !device_lost {
        if let Some(line) = recorder.flush() {
            println!("\r\x1B[KLine {}: {}", line_no + 1, describe(&line));
        }
    }

    drop(_guard);

    println!("\n\n=== Angle Snapping Detection Complete ===\n");

    let result = recorder.analyze();
    println!(
        "Lines analysed: {} | near an axis: {} | perfectly straight: {} | too short to judge: {}",
        result.total_strokes, result.near_axis_strokes, result.snapped_strokes, recorder.too_short
    );

    let outcome = if !result.conclusive {
        println!(
            "\n? Inconclusive - {} near-axis line(s) recorded, at least {} are needed.",
            result.near_axis_strokes, MIN_NEAR_AXIS_STROKES
        );
        println!(
            "  Draw more lines that are almost horizontal or vertical (within {:.0} degrees),",
            NEAR_AXIS_DEG
        );
        println!(
            "  each at least {:.0} counts long. Diagonal lines cannot show snapping.",
            MIN_STROKE_LENGTH
        );
        TestOutcome::Inconclusive
    } else if result.has_snapping {
        println!(
            "\n⚠ ANGLE SNAPPING DETECTED ({:.0}% of near-axis lines were perfectly straight)",
            result.snap_strength * 100.0
        );
        for angle in &result.dominant_angles {
            println!(
                "  Snapped axis: {:.0}° ({})",
                angle,
                if *angle == 0.0 {
                    "horizontal"
                } else {
                    "vertical"
                }
            );
        }
        println!("  This can affect precision in games and design work.");
        println!("  Turn off angle snapping / prediction in the mouse software.");
        TestOutcome::IssueFound
    } else if device_lost {
        println!("\n{}", standard::CUT_SHORT_VERDICT);
        TestOutcome::Inconclusive
    } else {
        println!(
            "\n✓ No angle snapping detected ({:.0}% of near-axis lines were perfectly straight)",
            result.snap_strength * 100.0
        );
        if result.snapped_strokes == 0 {
            println!("  Your near-axis lines kept their natural hand wobble.");
        } else {
            println!(
                "  Most near-axis lines kept their natural hand wobble, but {} came out",
                result.snapped_strokes
            );
            println!("  perfectly straight. If that happens again on another run, the mouse may");
            println!("  be snapping some of your lines.");
        }
        TestOutcome::NoIssues
    };
    let outcome = if device_lost {
        standard::after_device_lost(outcome)
    } else {
        outcome
    };

    if device_lost {
        terminal::print_device_lost();
    }
    terminal::wait_for_enter();
    outcome
}

/// What one finished line turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum LineResult {
    /// Fewer than [`MIN_STROKE_EVENTS`] reports or [`MIN_STROKE_LENGTH`] counts.
    TooShort {
        events: usize,
        length: f64,
    },
    Shape(StrokeShape),
}

/// How the shared analysis sees a single line.
fn classify(shape: &StrokeShape) -> (bool, bool) {
    let one = analyze_shapes(std::slice::from_ref(shape));
    (one.near_axis_strokes == 1, one.snapped_strokes == 1)
}

fn describe(line: &LineResult) -> String {
    match line {
        LineResult::TooShort { events, length } => format!(
            "too short to judge ({:.0} counts, {} reports; need {:.0} counts and {} reports)",
            length, events, MIN_STROKE_LENGTH, MIN_STROKE_EVENTS
        ),
        LineResult::Shape(shape) => {
            let a = shape.angle_deg.abs() % 180.0;
            let axis = if (45.0..=135.0).contains(&a) {
                "vertical"
            } else {
                "horizontal"
            };
            let (near, snapped) = classify(shape);
            let verdict = if !near {
                "too diagonal - not used (snapping only acts near an axis)"
            } else if snapped {
                "perfectly straight on the axis (snapped)"
            } else {
                "natural wobble"
            };
            format!(
                "{:.1}° from {}, wobble {:.2} counts rms - {}",
                shape.axis_deviation_deg, axis, shape.rms_residual, verdict
            )
        }
    }
}

/// Splits motion into lines and measures each with the shared analysis.
pub struct LineRecorder {
    segmenter: StrokeSegmenter,
    pub shapes: Vec<StrokeShape>,
    pub too_short: usize,
}

impl Default for LineRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl LineRecorder {
    pub fn new() -> Self {
        Self {
            segmenter: StrokeSegmenter::new(STROKE_GAP_S),
            shapes: Vec::new(),
            too_short: 0,
        }
    }

    fn record(&mut self, stroke: Stroke) -> LineResult {
        match stroke_shape(&stroke.deltas) {
            Some(shape) => {
                self.shapes.push(shape.clone());
                LineResult::Shape(shape)
            }
            None => {
                self.too_short += 1;
                LineResult::TooShort {
                    events: stroke.events(),
                    length: stroke.path_length(),
                }
            }
        }
    }

    /// Feed one motion report at `t` seconds; returns the line it ended.
    pub fn feed(&mut self, t: f64, dx: i32, dy: i32) -> Option<LineResult> {
        let stroke = self.segmenter.feed(t, dx, dy)?;
        Some(self.record(stroke))
    }

    /// Advance time; returns the current line once the pause ends it.
    pub fn tick(&mut self, now: f64) -> Option<LineResult> {
        let stroke = self.segmenter.tick(now)?;
        Some(self.record(stroke))
    }

    /// Finish the line in progress, if any.
    pub fn flush(&mut self) -> Option<LineResult> {
        let stroke = self.segmenter.flush()?;
        Some(self.record(stroke))
    }

    pub fn reset(&mut self) {
        self.segmenter.reset();
        self.shapes.clear();
        self.too_short = 0;
    }

    /// Drop the line in progress without judging it; returns whether there
    /// was one.
    pub fn drop_line(&mut self) -> bool {
        self.segmenter.flush().is_some()
    }

    /// Lines finished so far, judged or not.
    pub fn lines_seen(&self) -> usize {
        self.shapes.len() + self.too_short
    }

    pub fn near_axis(&self) -> usize {
        analyze_shapes(&self.shapes).near_axis_strokes
    }

    pub fn analyze(&self) -> AngleSnapAnalysis {
        analyze_shapes(&self.shapes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `lines` (each a list of per-report deltas) at 1000 Hz with a
    /// 0.5 s pause between them.
    fn draw(rec: &mut LineRecorder, lines: &[Vec<(i32, i32)>]) -> Vec<LineResult> {
        let mut out = Vec::new();
        let mut t = 0.0;
        for line in lines {
            for &(dx, dy) in line {
                out.extend(rec.feed(t, dx, dy));
                t += 0.001;
            }
            t += 0.5;
            out.extend(rec.tick(t));
        }
        out.extend(rec.flush());
        out
    }

    /// Deterministic hand wobble in -1..=1.
    fn wobble(seed: &mut u64) -> i32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        ((*seed % 3) as i32) - 1
    }

    /// A freehand line about 4 degrees off horizontal with natural wobble.
    fn freehand(seed: u64) -> Vec<(i32, i32)> {
        let mut s = seed.max(1);
        (0..150)
            .map(|i| {
                let drift = if i % 7 == 0 { 1 } else { 0 };
                let dy = if i % 3 == 0 { wobble(&mut s) } else { 0 };
                (2, drift + dy)
            })
            .collect()
    }

    #[test]
    fn test_snapped_near_axis_lines_are_detected() {
        // With snapping on, every slightly tilted line arrives exactly on
        // the axis.
        let mut rec = LineRecorder::new();
        let lines = vec![
            vec![(2, 0); 150],
            vec![(-3, 0); 120],
            vec![(0, 2); 150],
            vec![(2, 0); 150],
        ];
        let results = draw(&mut rec, &lines);
        assert_eq!(results.len(), 4);
        let a = rec.analyze();
        assert!(a.conclusive);
        assert!(a.has_snapping);
        assert_eq!(a.snapped_strokes, 4);
    }

    #[test]
    fn test_freehand_near_axis_lines_are_not_snapping() {
        let mut rec = LineRecorder::new();
        let lines: Vec<Vec<(i32, i32)>> = (1..=5).map(|s| freehand(s * 7919)).collect();
        draw(&mut rec, &lines);
        let a = rec.analyze();
        assert!(a.conclusive, "{:?}", a);
        assert!(!a.has_snapping, "{:?}", a);
        assert_eq!(a.near_axis_strokes, 5);
    }

    #[test]
    fn test_diagonal_lines_are_inconclusive_not_clean() {
        // The old test asked for diagonals and then reported "no snapping";
        // diagonals cannot show snapping at all.
        let mut rec = LineRecorder::new();
        let lines: Vec<Vec<(i32, i32)>> = (0..5).map(|_| vec![(2, 2); 100]).collect();
        draw(&mut rec, &lines);
        let a = rec.analyze();
        assert!(!a.conclusive);
        assert!(!a.has_snapping);
        assert_eq!(a.total_strokes, 5);
    }

    #[test]
    fn test_short_lines_are_counted_but_not_judged() {
        let mut rec = LineRecorder::new();
        let results = draw(&mut rec, &[vec![(2, 0); 10], vec![(1, 0); 40]]);
        assert_eq!(rec.too_short, 2);
        assert!(rec.shapes.is_empty());
        assert!(matches!(
            results[0],
            LineResult::TooShort { events: 10, .. }
        ));
        assert!(!rec.analyze().conclusive);
    }

    #[test]
    fn test_pause_splits_lines_and_reset_clears() {
        let mut rec = LineRecorder::new();
        draw(&mut rec, &[vec![(2, 0); 100], vec![(0, 2); 100]]);
        assert_eq!(rec.lines_seen(), 2);
        rec.reset();
        assert_eq!(rec.lines_seen(), 0);
        assert!(rec.flush().is_none());
    }

    #[test]
    fn test_a_line_cut_by_dropped_reports_is_not_judged() {
        let mut rec = LineRecorder::new();
        draw(&mut rec, &[freehand(7)]);
        // Half a line, then reports are dropped
        for i in 0..80 {
            rec.feed(10.0 + i as f64 * 0.001, 2, 0);
        }
        assert!(rec.drop_line());
        assert!(!rec.drop_line(), "nothing left in progress");
        assert_eq!(rec.lines_seen(), 1, "finished lines are kept");
        assert!(rec.flush().is_none());
    }

    #[test]
    fn test_describe_names_the_verdict() {
        let snapped = stroke_shape(&[(2, 0); 100]).unwrap();
        assert!(describe(&LineResult::Shape(snapped)).contains("snapped"));
        let diagonal = stroke_shape(&[(2, 2); 100]).unwrap();
        assert!(describe(&LineResult::Shape(diagonal)).contains("too diagonal"));
        let human = stroke_shape(&freehand(42)).unwrap();
        assert!(describe(&LineResult::Shape(human)).contains("natural wobble"));
    }
}
