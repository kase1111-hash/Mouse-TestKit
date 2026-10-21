//! Industry Standard Tests
//! Runs all standard mouse testing protocols one after another.
//!
//! The summary only reports what each test actually found. Tests that return
//! a [`TestOutcome`] are summarised with it; the others are listed as "ran"
//! with their result shown above, unless they say no mouse was measured. A
//! test that was skipped, stopped or had no mouse is never counted as
//! completed.

use std::io::{self, Write};

#[cfg(target_os = "linux")]
use crate::input;
#[cfg(target_os = "windows")]
use crate::input_windows as input;

/// What a CLI test found, so the batch runner can summarise honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    /// No mouse was selected, so nothing was measured.
    NoDevice,
    /// The test ran but collected too little data for a verdict.
    Inconclusive,
    /// A measurement with no pass/fail meaning (e.g. reaction time).
    Measured,
    /// Enough data was collected and nothing wrong was found.
    NoIssues,
    /// Something was seen once; a re-test decides whether it is real.
    PossibleIssue,
    /// A problem was found.
    IssueFound,
}

/// Status of one test in the suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    /// Skipped by the user, or the suite was stopped before it.
    NotRun,
    /// Ran, but the test does not report a verdict to the suite.
    Ran,
    /// Ran and reported this outcome.
    Finished(TestOutcome),
}

impl TestStatus {
    fn label(self) -> &'static str {
        match self {
            TestStatus::NotRun => "- Not run",
            TestStatus::Ran => "Ran - see its result above",
            TestStatus::Finished(outcome) => match outcome {
                TestOutcome::NoDevice => "✗ Not run (no mouse selected)",
                TestOutcome::Inconclusive => "? Inconclusive (not enough data)",
                TestOutcome::Measured => "Measured (no pass/fail)",
                TestOutcome::NoIssues => "✓ No problem found",
                TestOutcome::PossibleIssue => "⚠ Possible problem - re-test",
                TestOutcome::IssueFound => "⚠ Problem found - see above",
            },
        }
    }
}

/// How a test is started from the suite.
#[derive(Clone, Copy)]
enum Runner {
    /// The test reports what it found.
    Reports(fn() -> TestOutcome),
    /// The test only prints its result, and says whether a mouse was
    /// measured at all.
    Plain(fn() -> bool),
}

const SUITE: [(&str, Runner); 10] = [
    ("Polling Rate", Runner::Plain(super::polling::run)),
    ("Stutter Detection", Runner::Plain(super::stutter::run)),
    (
        "Click Reaction Time",
        Runner::Reports(super::click_response::run),
    ),
    (
        "Click Stickiness",
        Runner::Reports(super::click_sticky::run),
    ),
    ("Lift-Off Jump", Runner::Reports(super::liftoff::run)),
    ("DPI Accuracy", Runner::Reports(super::dpi::run)),
    ("Angle Snapping", Runner::Reports(super::angle_snap::run)),
    ("Acceleration", Runner::Plain(super::acceleration::run)),
    ("Double-Click", Runner::Reports(super::double_click::run)),
    ("Jitter", Runner::Plain(super::jitter::run)),
];

/// What the user chose at the prompt before a test.
enum NextStep {
    Run,
    Skip,
    Stop,
}

pub fn run_all() {
    println!("\n=== Industry Standard Mouse Test Suite ===\n");
    println!("This runs the tests below one after another.");
    println!("Each test can be exited by pressing 'q'; before each one you can skip it");
    println!("or stop the suite.\n");
    println!("Tests included:");
    for (i, (name, _)) in SUITE.iter().enumerate() {
        println!(" {:2}. {}", i + 1, name);
    }
    println!();

    if !mouse_present() {
        println!("No mouse device is available, so no test can run.");
        println!("Check the connection and permissions (see the message above), then try again.");
        println!("\nPress Enter to return to menu...");
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        return;
    }

    print!("Press Enter to begin, or 'q' to cancel: ");
    io::stdout().flush().ok();

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) | Err(_) => return,
        Ok(_) => {}
    }
    if input.trim().eq_ignore_ascii_case("q") {
        println!("Test suite cancelled.");
        return;
    }

    let total = SUITE.len();
    let mut statuses = [TestStatus::NotRun; SUITE.len()];
    let mut stopped = false;

    for (i, (name, runner)) in SUITE.iter().enumerate() {
        // The first test was just confirmed with Enter above.
        if i > 0 {
            match ask_next(i + 1, total, name) {
                NextStep::Run => {}
                NextStep::Skip => continue,
                NextStep::Stop => {
                    stopped = true;
                    break;
                }
            }
        }

        println!("\n══════════════════════════════════════════════════════════");
        println!("TEST {}/{}: {}", i + 1, total, name);
        println!("══════════════════════════════════════════════════════════");
        statuses[i] = match runner {
            Runner::Reports(run) => TestStatus::Finished(run()),
            Runner::Plain(run) => plain_status(run()),
        };
    }

    println!("\n╔══════════════════════════════════════════════════════════╗");
    if stopped {
        println!("║              TEST SUITE STOPPED                          ║");
    } else {
        println!("║              TEST SUITE FINISHED                         ║");
    }
    println!("╚══════════════════════════════════════════════════════════╝\n");

    println!("Results Summary:");
    println!("─────────────────────────────────────────────");
    for ((name, _), status) in SUITE.iter().zip(statuses.iter()) {
        println!("  {:<20} {}", format!("{}:", name), status.label());
    }
    println!("─────────────────────────────────────────────");
    println!("\n{}", summary(&statuses));

    println!("\nPress Enter to return to menu...");
    input.clear();
    io::stdin().read_line(&mut input).ok();
}

/// Status of a test that only prints its result, from whether it measured
/// a mouse at all.
fn plain_status(measured: bool) -> TestStatus {
    if measured {
        TestStatus::Ran
    } else {
        TestStatus::Finished(TestOutcome::NoDevice)
    }
}

/// Whether any mouse device can be opened at all.
fn mouse_present() -> bool {
    !input::find_mouse_devices().is_empty()
}

/// Ask before the next test whether to run it, skip it or stop the suite.
fn ask_next(number: usize, total: usize, name: &str) -> NextStep {
    print!(
        "\nNext: test {}/{} - {}. Press Enter to start, 's' to skip, 'q' to stop the suite: ",
        number, total, name
    );
    io::stdout().flush().ok();
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        // Closed or unreadable stdin: stop instead of running unattended.
        Ok(0) | Err(_) => NextStep::Stop,
        Ok(_) => match input.trim().to_ascii_lowercase().as_str() {
            "q" => NextStep::Stop,
            "s" => NextStep::Skip,
            _ => NextStep::Run,
        },
    }
}

/// One-paragraph summary of the suite. Never claims success for tests that
/// were not run, had no mouse, or gave no verdict.
pub fn summary(statuses: &[TestStatus]) -> String {
    let count = |f: &dyn Fn(&TestStatus) -> bool| statuses.iter().filter(|s| f(s)).count();
    let problems = count(&|s| *s == TestStatus::Finished(TestOutcome::IssueFound));
    let possible = count(&|s| *s == TestStatus::Finished(TestOutcome::PossibleIssue));
    let clean = count(&|s| *s == TestStatus::Finished(TestOutcome::NoIssues));
    let measured = count(&|s| *s == TestStatus::Finished(TestOutcome::Measured));
    let ran_unjudged = count(&|s| *s == TestStatus::Ran);
    let inconclusive = count(&|s| *s == TestStatus::Finished(TestOutcome::Inconclusive));
    let not_run = count(&|s| {
        matches!(
            s,
            TestStatus::NotRun | TestStatus::Finished(TestOutcome::NoDevice)
        )
    });

    let mut lines = Vec::new();
    if problems + possible + clean + measured + ran_unjudged + inconclusive == 0 {
        lines.push("No test produced a result.".to_string());
    } else {
        if problems > 0 {
            lines.push(format!("⚠ Problems found in {} test(s).", problems));
        }
        if possible > 0 {
            lines.push(format!(
                "⚠ Possible problems in {} test(s); re-run them to confirm.",
                possible
            ));
        }
        if problems == 0 && possible == 0 && clean > 0 {
            lines.push(format!(
                "✓ No problems found by the {} test(s) that gave a verdict.",
                clean
            ));
        }
        if ran_unjudged + measured > 0 {
            lines.push(format!(
                "{} test(s) show a measurement or result above without a verdict here.",
                ran_unjudged + measured
            ));
        }
        if inconclusive > 0 {
            lines.push(format!(
                "{} test(s) were inconclusive (too little data).",
                inconclusive
            ));
        }
    }
    if not_run > 0 {
        lines.push(format!("{} test(s) did not run.", not_run));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_mouse_or_quit_is_not_reported_as_success() {
        let statuses = [
            TestStatus::Finished(TestOutcome::NoDevice),
            TestStatus::NotRun,
            TestStatus::NotRun,
        ];
        let s = summary(&statuses);
        assert!(s.contains("No test produced a result"), "{}", s);
        assert!(s.contains("3 test(s) did not run"), "{}", s);
        assert!(!s.contains('✓'), "{}", s);
        assert!(!s.to_lowercase().contains("success"), "{}", s);
    }

    #[test]
    fn test_plain_test_without_a_mouse_did_not_run() {
        // Device selection cancelled in the polling, stutter, acceleration
        // or jitter test
        let statuses = [plain_status(false), plain_status(true)];
        assert_eq!(statuses[0], TestStatus::Finished(TestOutcome::NoDevice));
        let s = summary(&statuses);
        assert!(s.contains("1 test(s) did not run"), "{}", s);
        assert!(s.contains("1 test(s) show a measurement"), "{}", s);
    }

    #[test]
    fn test_problem_outranks_clean_results() {
        let statuses = [
            TestStatus::Finished(TestOutcome::NoIssues),
            TestStatus::Finished(TestOutcome::IssueFound),
            TestStatus::Ran,
        ];
        let s = summary(&statuses);
        assert!(s.contains("Problems found in 1 test(s)"), "{}", s);
        assert!(!s.contains("No problems found"), "{}", s);
        assert!(s.contains("1 test(s) show a measurement"), "{}", s);
    }

    #[test]
    fn test_clean_run_mentions_only_what_gave_a_verdict() {
        let statuses = [
            TestStatus::Finished(TestOutcome::NoIssues),
            TestStatus::Finished(TestOutcome::NoIssues),
            TestStatus::Finished(TestOutcome::Inconclusive),
            TestStatus::Finished(TestOutcome::Measured),
            TestStatus::NotRun,
        ];
        let s = summary(&statuses);
        assert!(s.contains("No problems found by the 2 test(s)"), "{}", s);
        assert!(s.contains("1 test(s) were inconclusive"), "{}", s);
        assert!(s.contains("1 test(s) did not run"), "{}", s);
    }

    #[test]
    fn test_status_labels_distinguish_not_run_from_finished() {
        assert_ne!(
            TestStatus::NotRun.label(),
            TestStatus::Finished(TestOutcome::NoIssues).label()
        );
        assert!(TestStatus::Finished(TestOutcome::NoDevice)
            .label()
            .contains("no mouse"));
        assert!(!TestStatus::Ran.label().contains('✓'));
    }
}
