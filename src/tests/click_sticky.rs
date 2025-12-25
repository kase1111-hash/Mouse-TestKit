/// Click Stickiness Test
/// Tests for stuck or delayed click release

pub fn run() {
    println!("\n=== Click Stickiness Test ===");
    println!("Click and release rapidly to test for stickiness...");
    println!();

    // TODO: Capture mouse down events
    // TODO: Capture mouse up events
    // TODO: Calculate hold duration
    // TODO: Detect abnormally long holds (stickiness)

    println!("[Placeholder] Click stickiness test not yet implemented.");
    println!("Press Enter to return to menu...");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}

pub struct ClickHold {
    pub button: super::click_response::MouseButton,
    pub duration_ms: f64,
    pub is_sticky: bool,
}

/// Threshold in ms - holds longer than this are considered sticky
pub const STICKY_THRESHOLD_MS: f64 = 50.0;

pub fn is_sticky(duration_ms: f64) -> bool {
    duration_ms > STICKY_THRESHOLD_MS
}
