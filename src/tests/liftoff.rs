/// Lift-Off Distance Jump Test
/// Tests for cursor jump during mouse lift

pub fn run() {
    println!("\n=== Lift-Off Jump Test ===");
    println!("Slowly lift your mouse off the surface...");
    println!();

    // TODO: Monitor mouse position during lift
    // TODO: Detect sudden position changes (jumps)
    // TODO: Report jump distance and direction

    println!("[Placeholder] Lift-off jump test not yet implemented.");
    println!("Press Enter to return to menu...");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}

pub struct LiftEvent {
    pub jump_x: i32,
    pub jump_y: i32,
    pub distance: f64,
}

impl LiftEvent {
    pub fn new(jump_x: i32, jump_y: i32) -> Self {
        let distance = ((jump_x.pow(2) + jump_y.pow(2)) as f64).sqrt();
        Self { jump_x, jump_y, distance }
    }
}

/// Threshold in pixels - movement larger than this during lift is a jump
pub const JUMP_THRESHOLD_PX: f64 = 5.0;

pub fn is_jump(distance: f64) -> bool {
    distance > JUMP_THRESHOLD_PX
}
