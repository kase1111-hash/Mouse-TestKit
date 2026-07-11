//! Industry Standard Tests
//! Runs all standard mouse testing protocols

use std::io::{self, Write};

pub fn run_all() {
    println!("\n=== Industry Standard Mouse Test Suite ===\n");
    println!("This will run a comprehensive series of tests.");
    println!("Each test can be exited by pressing 'q'.\n");
    println!("Tests included:");
    println!("  1. Polling Rate Monitor");
    println!("  2. Stutter Detection");
    println!("  3. Click Response Test");
    println!("  4. Click Stickiness Test");
    println!("  5. Lift-Off Jump Test");
    println!("  6. DPI Accuracy Test");
    println!("  7. Angle Snapping Detection");
    println!("  8. Acceleration Detection");
    println!("  9. Double-Click Test");
    println!(" 10. Jitter Test\n");

    print!("Press Enter to begin, or 'q' to cancel: ");
    io::stdout().flush().ok();

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();

    if input.trim().to_lowercase() == "q" {
        println!("Test suite cancelled.");
        return;
    }

    let mut results = TestSuite::new();

    // Core Tests
    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 1/10: Polling Rate");
    println!("══════════════════════════════════════════════════════════");
    super::polling::run();
    results.polling = true;

    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 2/10: Stutter Detection");
    println!("══════════════════════════════════════════════════════════");
    super::stutter::run();
    results.stutter = true;

    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 3/10: Click Response");
    println!("══════════════════════════════════════════════════════════");
    super::click_response::run();
    results.click_response = true;

    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 4/10: Click Stickiness");
    println!("══════════════════════════════════════════════════════════");
    super::click_sticky::run();
    results.click_sticky = true;

    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 5/10: Lift-Off Jump");
    println!("══════════════════════════════════════════════════════════");
    super::liftoff::run();
    results.liftoff = true;

    // Advanced Tests
    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 6/10: DPI Accuracy");
    println!("══════════════════════════════════════════════════════════");
    super::dpi::run();
    results.dpi = true;

    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 7/10: Angle Snapping Detection");
    println!("══════════════════════════════════════════════════════════");
    super::angle_snap::run();
    results.angle_snap = true;

    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 8/10: Acceleration Detection");
    println!("══════════════════════════════════════════════════════════");
    super::acceleration::run();
    results.acceleration = true;

    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 9/10: Double-Click Test");
    println!("══════════════════════════════════════════════════════════");
    super::double_click::run();
    results.double_click = true;

    println!("\n══════════════════════════════════════════════════════════");
    println!("TEST 10/10: Jitter Test");
    println!("══════════════════════════════════════════════════════════");
    super::jitter::run();
    results.jitter = true;

    // Final summary
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║              FULL TEST SUITE COMPLETE                    ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    println!("Results Summary:");
    println!("─────────────────────────────────────────────");
    println!("  Polling Rate:       {}", status(results.polling));
    println!("  Stutter Detection:  {}", status(results.stutter));
    println!("  Click Response:     {}", status(results.click_response));
    println!("  Click Stickiness:   {}", status(results.click_sticky));
    println!("  Lift-Off Jump:      {}", status(results.liftoff));
    println!("  DPI Accuracy:       {}", status(results.dpi));
    println!("  Angle Snapping:     {}", status(results.angle_snap));
    println!("  Acceleration:       {}", status(results.acceleration));
    println!("  Double-Click:       {}", status(results.double_click));
    println!("  Jitter:             {}", status(results.jitter));
    println!("─────────────────────────────────────────────");

    let completed = results.count_completed();
    println!("\nCompleted: {}/10 tests", completed);

    if completed == 10 {
        println!("\n✓ All tests completed successfully!");
    }

    println!("\nPress Enter to return to menu...");
    io::stdin().read_line(&mut input).ok();
}

fn status(completed: bool) -> &'static str {
    if completed {
        "✓ Complete"
    } else {
        "✗ Skipped"
    }
}

pub struct TestSuite {
    pub polling: bool,
    pub stutter: bool,
    pub click_response: bool,
    pub click_sticky: bool,
    pub liftoff: bool,
    pub dpi: bool,
    pub angle_snap: bool,
    pub acceleration: bool,
    pub double_click: bool,
    pub jitter: bool,
}

impl TestSuite {
    pub fn new() -> Self {
        Self {
            polling: false,
            stutter: false,
            click_response: false,
            click_sticky: false,
            liftoff: false,
            dpi: false,
            angle_snap: false,
            acceleration: false,
            double_click: false,
            jitter: false,
        }
    }

    pub fn count_completed(&self) -> usize {
        let mut count = 0;
        if self.polling {
            count += 1;
        }
        if self.stutter {
            count += 1;
        }
        if self.click_response {
            count += 1;
        }
        if self.click_sticky {
            count += 1;
        }
        if self.liftoff {
            count += 1;
        }
        if self.dpi {
            count += 1;
        }
        if self.angle_snap {
            count += 1;
        }
        if self.acceleration {
            count += 1;
        }
        if self.double_click {
            count += 1;
        }
        if self.jitter {
            count += 1;
        }
        count
    }
}

impl Default for TestSuite {
    fn default() -> Self {
        Self::new()
    }
}
