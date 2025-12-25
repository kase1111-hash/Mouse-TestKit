/// Industry Standard Tests
/// Runs all standard mouse testing protocols

pub fn run_all() {
    println!("\n=== Running All Standard Tests ===");
    println!();

    println!("Running stutter detection...");
    super::stutter::run();

    println!("Running polling rate test...");
    super::polling::run();

    println!("Running click response test...");
    super::click_response::run();

    println!("Running click stickiness test...");
    super::click_sticky::run();

    println!("Running lift-off jump test...");
    super::liftoff::run();

    println!();
    println!("=== All Tests Complete ===");
}

pub struct TestSuite {
    pub stutter_passed: bool,
    pub polling_passed: bool,
    pub click_response_passed: bool,
    pub click_sticky_passed: bool,
    pub liftoff_passed: bool,
}

impl TestSuite {
    pub fn all_passed(&self) -> bool {
        self.stutter_passed
            && self.polling_passed
            && self.click_response_passed
            && self.click_sticky_passed
            && self.liftoff_passed
    }
}
