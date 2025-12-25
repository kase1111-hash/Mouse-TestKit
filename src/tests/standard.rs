/// Industry Standard Tests
/// Runs all standard mouse testing protocols

use std::io::{self, Write};

pub fn run_all() {
    println!("\n=== Industry Standard Mouse Test Suite ===\n");
    println!("This will run a series of tests to evaluate your mouse.");
    println!("Each test can be skipped by pressing 'q'.\n");
    println!("Tests included:");
    println!("  1. Polling Rate Monitor");
    println!("  2. Stutter Detection");
    println!("  3. Click Response Test");
    println!("  4. Click Stickiness Test");
    println!("  5. Lift-Off Jump Test\n");

    print!("Press Enter to begin, or 'q' to cancel: ");
    io::stdout().flush().ok();

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();

    if input.trim().to_lowercase() == "q" {
        println!("Test suite cancelled.");
        return;
    }

    let mut results = TestSuite {
        polling_passed: false,
        stutter_passed: false,
        click_response_passed: false,
        click_sticky_passed: false,
        liftoff_passed: false,
    };

    // Run each test
    println!("\n──────────────────────────────────────────────────────────");
    println!("TEST 1/5: Polling Rate");
    println!("──────────────────────────────────────────────────────────");
    super::polling::run();
    results.polling_passed = true;

    println!("\n──────────────────────────────────────────────────────────");
    println!("TEST 2/5: Stutter Detection");
    println!("──────────────────────────────────────────────────────────");
    super::stutter::run();
    results.stutter_passed = true;

    println!("\n──────────────────────────────────────────────────────────");
    println!("TEST 3/5: Click Response");
    println!("──────────────────────────────────────────────────────────");
    super::click_response::run();
    results.click_response_passed = true;

    println!("\n──────────────────────────────────────────────────────────");
    println!("TEST 4/5: Click Stickiness");
    println!("──────────────────────────────────────────────────────────");
    super::click_sticky::run();
    results.click_sticky_passed = true;

    println!("\n──────────────────────────────────────────────────────────");
    println!("TEST 5/5: Lift-Off Jump");
    println!("──────────────────────────────────────────────────────────");
    super::liftoff::run();
    results.liftoff_passed = true;

    // Final summary
    println!("\n══════════════════════════════════════════════════════════");
    println!("              TEST SUITE COMPLETE");
    println!("══════════════════════════════════════════════════════════\n");

    println!("Results:");
    println!("  Polling Rate:    {}", if results.polling_passed { "✓ Complete" } else { "✗ Skipped" });
    println!("  Stutter:         {}", if results.stutter_passed { "✓ Complete" } else { "✗ Skipped" });
    println!("  Click Response:  {}", if results.click_response_passed { "✓ Complete" } else { "✗ Skipped" });
    println!("  Click Sticky:    {}", if results.click_sticky_passed { "✓ Complete" } else { "✗ Skipped" });
    println!("  Lift-Off:        {}", if results.liftoff_passed { "✓ Complete" } else { "✗ Skipped" });

    if results.all_passed() {
        println!("\n✓ All tests completed successfully!");
    } else {
        println!("\nSome tests were skipped. Run individually for detailed results.");
    }

    println!("\nPress Enter to return to menu...");
    io::stdin().read_line(&mut input).ok();
}

pub struct TestSuite {
    pub polling_passed: bool,
    pub stutter_passed: bool,
    pub click_response_passed: bool,
    pub click_sticky_passed: bool,
    pub liftoff_passed: bool,
}

impl TestSuite {
    pub fn all_passed(&self) -> bool {
        self.polling_passed
            && self.stutter_passed
            && self.click_response_passed
            && self.click_sticky_passed
            && self.liftoff_passed
    }
}
