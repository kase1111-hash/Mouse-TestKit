//! Lift-Off Jump Test
//! Tests for cursor jump during mouse lift
//!
//! A sensor that keeps tracking while it goes out of focus makes the cursor
//! jump when the mouse is lifted or set back down. The user lifts the mouse
//! straight up and sets it down several times; any motion that starts out of
//! stillness is a *burst*. A jump is not one report - at 1000 Hz it is
//! spread over many - so the motion in the first [`JUMP_WINDOW_MS`] of each
//! burst is summed, and the furthest the cursor got from where the burst
//! started is compared with [`JUMP_THRESHOLD_MM`], converted to counts with
//! the mouse's DPI so the verdict does not depend on the DPI setting.
//! A burst still going after [`MAX_LIFT_BURST_MS`] is the hand sliding the
//! mouse, not a lift, and is not judged.

use super::standard::TestOutcome;
use crate::terminal;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

/// Window over which the mouse counts as still or moving: a burst starts
/// when motion within it reaches [`STILL_MM`] and ends once a whole window
/// passes with less.
pub const IDLE_THRESHOLD_MS: u64 = 100;
/// Lifting or setting down takes the sensor through its 1-2 mm focus range
/// in a few tens of ms, so a lift artefact is over within this window.
pub const JUMP_WINDOW_MS: f64 = 100.0;
/// Motion still going this long after it started is a deliberate slide.
pub const MAX_LIFT_BURST_MS: f64 = 200.0;
/// A lift or set-down that moves the cursor further than this is a jump.
/// Lifting straight up or setting down usually moves the mouse sideways by
/// well under a millimetre; 2 mm is ~63 counts at 800 DPI, a clearly visible
/// jump. A single careless set-down can exceed it, so one jump alone is only
/// reported as a possible problem.
pub const JUMP_THRESHOLD_MM: f64 = 2.0;
/// Less motion than this within [`IDLE_THRESHOLD_MS`] is the mouse at rest:
/// a resting hand at high DPI gives a stray count here and there, which must
/// neither start a burst nor keep one open and swallow a later jump.
pub const STILL_MM: f64 = 0.25;
/// Used when the user does not enter the mouse's DPI (the usual default).
pub const DEFAULT_DPI: f64 = 800.0;
/// Two or more jumps are a pattern; a single one may be a hand slip.
const CONFIRMED_JUMPS: usize = 2;

const MM_PER_INCH: f64 = 25.4;

pub fn mm_to_counts(mm: f64, dpi: f64) -> f64 {
    mm / MM_PER_INCH * dpi
}

pub fn counts_to_mm(counts: f64, dpi: f64) -> f64 {
    counts / dpi * MM_PER_INCH
}

pub fn run() -> TestOutcome {
    println!("\n=== Lift-Off Jump Test ===");
    println!("This test detects cursor jumps when the mouse is lifted or set down.");
    println!("\nInstructions:");
    println!("  1. Rest the mouse on the pad and keep it still");
    println!("  2. Lift it straight up about 1 cm, hold it there a moment,");
    println!("     then set it straight back down - try not to slide it");
    println!("  3. Wait about a second and repeat 5-10 times");
    println!("  4. Press 'q' when done\n");

    print!("Enter your mouse's DPI so jumps can be measured in mm [Enter = 800]: ");
    io::stdout().flush().ok();
    let mut dpi_input = String::new();
    io::stdin().read_line(&mut dpi_input).ok();
    let dpi = match super::dpi::parse_dpi(&dpi_input) {
        Some(d) => d,
        None => {
            println!(
                "Assuming {:.0} DPI. If your mouse is set differently, the distances below",
                DEFAULT_DPI
            );
            println!("are off by the same factor.");
            DEFAULT_DPI
        }
    };
    println!(
        "Jump threshold: {:.1} mm ({:.0} counts at {:.0} DPI)",
        JUMP_THRESHOLD_MM,
        mm_to_counts(JUMP_THRESHOLD_MM, dpi),
        dpi
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
    let mut detector = LiftDetector::new(dpi);
    let mut jumps: Vec<Burst> = Vec::new();
    let mut slides: Vec<Burst> = Vec::new();
    let mut largest_mm = 0.0f64;
    let mut last_print = Instant::now();

    let _guard = terminal::TerminalGuard::new();

    println!("\nMonitoring... (press 'q' to quit)\n");

    let handle = |burst: Burst,
                  jumps: &mut Vec<Burst>,
                  slides: &mut Vec<Burst>,
                  largest_mm: &mut f64| {
        let mm = counts_to_mm(burst.peak_counts, dpi);
        match burst.classify(dpi) {
            BurstKind::Jump => {
                *largest_mm = largest_mm.max(mm);
                println!(
                    "\r\x1B[K⚠ JUMP: cursor moved {:.1} mm (dx {:+}, dy {:+} counts) within {:.0} ms",
                    mm,
                    burst.peak_dx,
                    burst.peak_dy,
                    burst.duration_ms.min(JUMP_WINDOW_MS)
                );
                jumps.push(burst);
            }
            BurstKind::Slide => {
                println!(
                    "\r\x1B[K  Sliding movement ({:.0} ms) ignored - lift straight up for this test",
                    burst.duration_ms
                );
                slides.push(burst);
            }
            BurstKind::Quiet => *largest_mm = largest_mm.max(mm),
        }
    };

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                kind: KeyEventKind::Press,
                ..
            })) = event::read()
            {
                break;
            }
        }

        for timed in input::fetch_timed_events(&mut device) {
            if let MouseEvent::Move { dx, dy } = timed.event {
                if let Some(burst) = detector.feed(secs(timed.time), dx, dy) {
                    handle(burst, &mut jumps, &mut slides, &mut largest_mm);
                }
            }
        }
        if let Some(burst) = detector.tick(secs(Instant::now())) {
            handle(burst, &mut jumps, &mut slides, &mut largest_mm);
        }

        if last_print.elapsed() >= Duration::from_millis(200) {
            print!("\r\x1B[K");
            print!("Jumps: {} | ", jumps.len());
            print!("Largest lift/set-down motion: {:.1} mm | ", largest_mm);
            print!("Slides ignored: {} | ", slides.len());
            print!(
                "Status: {}",
                if detector.in_burst() {
                    "moving"
                } else {
                    "still"
                }
            );
            io::stdout().flush().ok();
            last_print = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    if let Some(burst) = detector.flush() {
        handle(burst, &mut jumps, &mut slides, &mut largest_mm);
    }

    drop(_guard);

    println!("\n\n=== Lift-Off Jump Test Complete ===\n");
    println!(
        "DPI used: {:.0} | jump threshold: {:.1} mm ({:.0} counts)",
        dpi,
        JUMP_THRESHOLD_MM,
        mm_to_counts(JUMP_THRESHOLD_MM, dpi)
    );
    println!("Total jumps detected: {}", jumps.len());

    if !jumps.is_empty() {
        let dists: Vec<f64> = jumps
            .iter()
            .map(|j| counts_to_mm(j.peak_counts, dpi))
            .collect();
        let avg = dists.iter().sum::<f64>() / dists.len() as f64;
        let max = dists.iter().copied().fold(0.0, f64::max);

        println!("\nJump statistics:");
        println!("  Average distance: {:.1} mm", avg);
        println!("  Maximum distance: {:.1} mm", max);

        println!("\nJump events:");
        for (i, (jump, mm)) in jumps.iter().zip(&dists).enumerate() {
            println!(
                "  #{}: {:.1} mm (dx: {}, dy: {} counts)",
                i + 1,
                mm,
                jump.peak_dx,
                jump.peak_dy
            );
        }
    }

    let outcome = if jumps.len() >= CONFIRMED_JUMPS {
        println!("\n⚠ The cursor jumps when the mouse is lifted or set down.");
        println!("  Likely causes:");
        println!("  - High lift-off distance (LOD) setting - lower it in the mouse software");
        println!("  - Sensor tracking issues");
        println!("  - Surface compatibility problems");
        TestOutcome::IssueFound
    } else if jumps.len() == 1 {
        println!("\n⚠ One jump. A single jump is often the hand sliding the mouse on set-down;");
        println!("  repeat the test - a second jump points to a high lift-off distance.");
        TestOutcome::PossibleIssue
    } else {
        println!("\n✓ No jumps detected. Lift-off behavior appears normal.");
        println!("  (A clean lift produces little or no motion at all, so make sure you");
        println!("  actually lifted the mouse several times.)");
        TestOutcome::NoIssues
    };

    if !slides.is_empty() {
        println!(
            "\n{} longer movement(s) were treated as sliding and not judged. If the mouse",
            slides.len()
        );
        println!("was in the air at those moments, the sensor is tracking while lifted");
        println!("(lift-off distance too high).");
    }

    terminal::wait_for_enter();
    outcome
}

/// Motion that started after the mouse had been still.
#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    /// Net counts at the point furthest from the start, within the window.
    pub peak_dx: i64,
    pub peak_dy: i64,
    /// That furthest distance, in counts.
    pub peak_counts: f64,
    /// Time from when the motion began to its last report (ms).
    pub duration_ms: f64,
}

/// What a burst was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BurstKind {
    /// Short burst that moved the cursor further than the jump threshold.
    Jump,
    /// Short burst within the threshold: a clean lift or set-down.
    Quiet,
    /// Kept going: the hand slid the mouse.
    Slide,
}

impl Burst {
    pub fn classify(&self, dpi: f64) -> BurstKind {
        if self.duration_ms > MAX_LIFT_BURST_MS {
            BurstKind::Slide
        } else if self.peak_counts > mm_to_counts(JUMP_THRESHOLD_MM, dpi) {
            BurstKind::Jump
        } else {
            BurstKind::Quiet
        }
    }
}

#[derive(Debug)]
struct OpenBurst {
    start: f64,
    last: f64,
    x: i64,
    y: i64,
    peak: (i64, i64, f64),
}

impl OpenBurst {
    fn finish(self) -> Burst {
        Burst {
            peak_dx: self.peak.0,
            peak_dy: self.peak.1,
            peak_counts: self.peak.2,
            duration_ms: (self.last - self.start) * 1000.0,
        }
    }
}

/// Splits motion into bursts that start from stillness. Times are seconds.
#[derive(Debug)]
pub struct LiftDetector {
    idle_s: f64,
    window_s: f64,
    still_counts: f64,
    /// Reports (time, dx, dy) within the last idle window.
    recent: VecDeque<(f64, i32, i32)>,
    current: Option<OpenBurst>,
}

impl LiftDetector {
    pub fn new(dpi: f64) -> Self {
        Self {
            idle_s: IDLE_THRESHOLD_MS as f64 / 1000.0,
            window_s: JUMP_WINDOW_MS / 1000.0,
            still_counts: mm_to_counts(STILL_MM, dpi),
            recent: VecDeque::new(),
            current: None,
        }
    }

    /// Drop reports that are a whole idle window older than `t`.
    fn prune(&mut self, t: f64) {
        while self
            .recent
            .front()
            .is_some_and(|&(rt, _, _)| t - rt >= self.idle_s)
        {
            self.recent.pop_front();
        }
    }

    /// Path length of the reports in the last idle window.
    fn recent_path(&self) -> f64 {
        self.recent
            .iter()
            .map(|&(_, dx, dy)| (dx as f64).hypot(dy as f64))
            .sum()
    }

    /// The open burst ends once it has lasted at least one idle window and
    /// the last idle window was still.
    fn try_finish(&mut self, now: f64) -> Option<Burst> {
        let started = self.current.as_ref()?.start;
        self.prune(now);
        if now - started >= self.idle_s && self.recent_path() < self.still_counts {
            self.current.take().map(OpenBurst::finish)
        } else {
            None
        }
    }

    /// Feed one motion report. Returns the previous burst if it had ended.
    pub fn feed(&mut self, t: f64, dx: i32, dy: i32) -> Option<Burst> {
        if dx == 0 && dy == 0 {
            return None;
        }
        let finished = self.try_finish(t);
        self.prune(t);
        self.recent.push_back((t, dx, dy));

        match &mut self.current {
            None => {
                // Motion out of stillness: the reports that built it up are
                // the start of the burst.
                if self.recent_path() >= self.still_counts {
                    let (x, y) = self
                        .recent
                        .iter()
                        .fold((0i64, 0i64), |(x, y), &(_, dx, dy)| {
                            (x + dx as i64, y + dy as i64)
                        });
                    self.current = Some(OpenBurst {
                        start: t,
                        last: t,
                        x,
                        y,
                        peak: (x, y, (x as f64).hypot(y as f64)),
                    });
                }
            }
            Some(b) => {
                if t - b.start <= self.window_s {
                    b.x += dx as i64;
                    b.y += dy as i64;
                    let d = (b.x as f64).hypot(b.y as f64);
                    if d > b.peak.2 {
                        b.peak = (b.x, b.y, d);
                    }
                }
                b.last = t;
            }
        }
        finished
    }

    /// Advance time without a report; returns the burst if it has ended.
    pub fn tick(&mut self, now: f64) -> Option<Burst> {
        self.try_finish(now)
    }

    /// End the test: return whatever burst is still open.
    pub fn flush(&mut self) -> Option<Burst> {
        self.current.take().map(OpenBurst::finish)
    }

    pub fn in_burst(&self) -> bool {
        self.current.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed reports `(t_ms, dx, dy)`, then let time run on; collect bursts.
    fn run(det: &mut LiftDetector, reports: &[(f64, i32, i32)]) -> Vec<Burst> {
        let mut out = Vec::new();
        let mut last = 0.0;
        for &(t_ms, dx, dy) in reports {
            out.extend(det.tick(t_ms / 1000.0));
            out.extend(det.feed(t_ms / 1000.0, dx, dy));
            last = t_ms;
        }
        out.extend(det.tick(last / 1000.0 + 1.0));
        out
    }

    fn kinds(bursts: &[Burst], dpi: f64) -> Vec<BurstKind> {
        bursts.iter().map(|b| b.classify(dpi)).collect()
    }

    #[test]
    fn test_jump_spread_over_many_reports_is_detected() {
        // 200 counts at 10 counts/ms (1000 Hz) after stillness. Each report
        // is only 10 counts, so the old per-report check never saw it.
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let reports: Vec<(f64, i32, i32)> = (0..20).map(|i| (500.0 + i as f64, 10, 0)).collect();
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump]);
        assert_eq!(bursts[0].peak_dx, 200);
        let mm = counts_to_mm(bursts[0].peak_counts, dpi);
        assert!((mm - 3.175).abs() < 0.01, "{} mm", mm);
    }

    #[test]
    fn test_normal_flick_at_high_dpi_is_not_a_jump() {
        // 1600 DPI, 125 Hz: the first report of an ordinary movement is
        // already 60 counts, which the old check called a jump.
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let reports: Vec<(f64, i32, i32)> =
            (0..50).map(|i| (300.0 + i as f64 * 8.0, 60, 5)).collect();
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Slide]);
    }

    #[test]
    fn test_small_set_down_wobble_is_quiet() {
        // Setting the mouse down shifts it a few counts: 0.5 mm at 800 DPI.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let bursts = run(
            &mut det,
            &[
                (400.0, 3, 1),
                (401.0, 4, 0),
                (402.0, 3, -1),
                (404.0, 2, 0),
                (407.0, 3, 1),
            ],
        );
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Quiet]);
        assert!(counts_to_mm(bursts[0].peak_counts, dpi) < 1.0);
    }

    #[test]
    fn test_threshold_follows_dpi() {
        // The same 60-count burst is a 3.8 mm jump at 400 DPI but only
        // 0.5 mm at 3200 DPI.
        let reports = [(200.0, 30, 0), (201.0, 30, 0)];
        let at_400 = run(&mut LiftDetector::new(400.0), &reports);
        assert_eq!(kinds(&at_400, 400.0), vec![BurstKind::Jump]);
        let at_3200 = run(&mut LiftDetector::new(3200.0), &reports);
        assert_eq!(kinds(&at_3200, 3200.0), vec![BurstKind::Quiet]);
    }

    #[test]
    fn test_back_and_forth_jump_uses_peak_distance() {
        // Out 100 counts and straight back: net zero, but the cursor jumped.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports: Vec<(f64, i32, i32)> =
            (0..10).map(|i| (300.0 + i as f64, 0, -10)).collect();
        reports.extend((10..20).map(|i| (300.0 + i as f64, 0, 10)));
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump]);
        assert_eq!((bursts[0].peak_dx, bursts[0].peak_dy), (0, -100));
    }

    #[test]
    fn test_hand_tremor_does_not_hide_a_jump() {
        // At 1600 DPI a resting hand produces a stray count every ~30 ms.
        // The mouse is still "still", so the jump is still found.
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports: Vec<(f64, i32, i32)> = (0..30).map(|i| (i as f64 * 30.0, 1, 0)).collect();
        reports.extend((0..10).map(|i| (905.0 + i as f64, 0, 15)));
        reports.extend((0..10).map(|i| (1000.0 + i as f64 * 30.0, 0, 1)));
        let bursts = run(&mut det, &reports);
        // Tremor alone never opens a burst; the jump is the only one.
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump], "{:?}", bursts);
        assert!((150..=152).contains(&bursts[0].peak_dy), "{:?}", bursts);
    }

    #[test]
    fn test_motion_after_the_window_is_not_added() {
        // Set-down shift of 20 counts, then small corrections in the same
        // burst. Only the first 100 ms is judged: 50 counts (1.6 mm). All of
        // it would be 70 counts (2.2 mm) and a "jump".
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports: Vec<(f64, i32, i32)> =
            (0..4).map(|i| (300.0 + i as f64 * 2.0, 5, 0)).collect();
        reports.extend((0..5).map(|i| (360.0 + i as f64 * 20.0, 10, 0)));
        let bursts = run(&mut det, &reports);
        assert_eq!(bursts.len(), 1);
        assert_eq!(bursts[0].peak_dx, 50);
        assert_eq!(bursts[0].classify(dpi), BurstKind::Quiet);
    }

    #[test]
    fn test_tremor_alone_is_not_a_burst() {
        let mut det = LiftDetector::new(1600.0);
        let reports: Vec<(f64, i32, i32)> = (0..100)
            .map(|i| (i as f64 * 25.0, if i % 2 == 0 { 1 } else { -1 }, 0))
            .collect();
        assert!(run(&mut det, &reports).is_empty());
    }

    #[test]
    fn test_repeated_lifts_give_separate_bursts() {
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports = Vec::new();
        for lift in 0..3 {
            let t0 = 500.0 + lift as f64 * 1000.0;
            reports.extend((0..8).map(|i| (t0 + i as f64, 12, 4)));
        }
        let bursts = run(&mut det, &reports);
        assert_eq!(
            kinds(&bursts, dpi),
            vec![BurstKind::Jump, BurstKind::Jump, BurstKind::Jump]
        );
    }

    #[test]
    fn test_flush_returns_open_burst() {
        let mut det = LiftDetector::new(800.0);
        assert!(det.feed(0.5, 40, 0).is_none());
        assert!(det.in_burst());
        let b = det.flush().unwrap();
        assert_eq!(b.peak_dx, 40);
        assert!(!det.in_burst());
    }

    #[test]
    fn test_unit_conversion() {
        assert!((mm_to_counts(25.4, 800.0) - 800.0).abs() < 1e-9);
        assert!((counts_to_mm(1600.0, 1600.0) - 25.4).abs() < 1e-9);
    }
}
