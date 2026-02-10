//! Lift-Off Distance Jump Test
//! Tests for cursor jump during mouse lift

use std::time::{Duration, Instant};
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crate::terminal;

#[cfg(target_os = "linux")]
use crate::input::{self, MouseEvent};
#[cfg(target_os = "windows")]
use crate::input_windows::{self as input, MouseEvent};

pub const JUMP_THRESHOLD_PX: f64 = 50.0;
pub const IDLE_THRESHOLD_MS: u64 = 100;

pub fn run() {
    println!("\n=== Lift-Off Jump Test ===");
    println!("This test detects cursor jumps when lifting the mouse.");
    println!("Move your mouse, then slowly lift it off the surface.");
    println!("Press 'q' to quit.\n");

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            terminal::wait_for_enter();
            return;
        }
    };

    #[cfg(target_os = "linux")]
    device.grab().ok();

    let mut last_event_time = Instant::now();
    let mut accumulated_dx: i32 = 0;
    let mut accumulated_dy: i32 = 0;
    let mut jump_events: Vec<LiftEvent> = Vec::new();
    let mut last_print = Instant::now();
    let mut was_idle = false;
    let mut position = (0i64, 0i64);

    let _guard = terminal::TerminalGuard::new();

    println!("\nMonitoring... (press 'q' to quit)\n");

    loop {
        if event::poll(Duration::from_millis(1)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent { code: KeyCode::Char('q'), .. })) = event::read() {
                break;
            }
        }

        let now = Instant::now();
        let time_since_last = now.duration_since(last_event_time).as_millis() as u64;

        if let Ok(events) = device.fetch_events() {
            for ev in events {
                #[cfg(target_os = "linux")]
                let parsed = input::parse_event(&ev);
                #[cfg(target_os = "windows")]
                let parsed = Some(ev);

                if let Some(MouseEvent::Move { dx, dy }) = parsed {
                    accumulated_dx += dx;
                    accumulated_dy += dy;
                    position.0 += dx as i64;
                    position.1 += dy as i64;

                    let distance = ((accumulated_dx.pow(2) + accumulated_dy.pow(2)) as f64).sqrt();

                    if was_idle && distance > JUMP_THRESHOLD_PX {
                        let event = LiftEvent::new(accumulated_dx, accumulated_dy);
                        println!("\r\x1B[K⚠ JUMP DETECTED! Distance: {:.1}px (dx: {}, dy: {})",
                            event.distance, accumulated_dx, accumulated_dy);
                        jump_events.push(event);
                    }

                    last_event_time = now;
                    accumulated_dx = 0;
                    accumulated_dy = 0;
                    was_idle = false;
                }
            }
        }

        // Check for idle state (potential lift)
        if time_since_last > IDLE_THRESHOLD_MS && !was_idle {
            was_idle = true;
        }

        if last_print.elapsed() >= Duration::from_millis(200) {
            print!("\r\x1B[K");
            print!("Position: ({}, {}) | ", position.0, position.1);
            print!("Jumps detected: {} | ", jump_events.len());
            print!("Status: {}", if was_idle { "IDLE (lift detected)" } else { "Moving" });

            use std::io::Write;
            std::io::stdout().flush().ok();
            last_print = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    drop(_guard);

    println!("\n\n=== Lift-Off Jump Test Complete ===\n");
    println!("Total jumps detected: {}", jump_events.len());

    if !jump_events.is_empty() {
        let avg = jump_events.iter().map(|j| j.distance).sum::<f64>() / jump_events.len() as f64;
        let max = jump_events.iter().map(|j| j.distance).fold(f64::MIN, f64::max);

        println!("\nJump statistics:");
        println!("  Average distance: {:.1} px", avg);
        println!("  Maximum distance: {:.1} px", max);

        println!("\nJump events:");
        for (i, event) in jump_events.iter().enumerate() {
            println!("  #{}: {:.1}px (dx: {}, dy: {})", i + 1, event.distance, event.jump_x, event.jump_y);
        }

        println!("\n⚠ Jumps during lift may indicate:");
        println!("  - High lift-off distance (LOD) setting");
        println!("  - Sensor tracking issues");
        println!("  - Surface compatibility problems");
    } else {
        println!("\n✓ No jumps detected. Lift-off behavior appears normal.");
    }

    terminal::wait_for_enter();
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

#[allow(dead_code)]
pub fn is_jump(distance: f64) -> bool {
    distance > JUMP_THRESHOLD_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lift_event_new_basic() {
        let event = LiftEvent::new(3, 4);
        assert_eq!(event.jump_x, 3);
        assert_eq!(event.jump_y, 4);
        // Pythagorean: sqrt(9 + 16) = 5.0
        assert!((event.distance - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_lift_event_new_zero() {
        let event = LiftEvent::new(0, 0);
        assert_eq!(event.jump_x, 0);
        assert_eq!(event.jump_y, 0);
        assert_eq!(event.distance, 0.0);
    }

    #[test]
    fn test_lift_event_new_negative() {
        let event = LiftEvent::new(-3, -4);
        assert_eq!(event.jump_x, -3);
        assert_eq!(event.jump_y, -4);
        // Distance should still be positive
        assert!((event.distance - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_lift_event_horizontal_only() {
        let event = LiftEvent::new(100, 0);
        assert!((event.distance - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_lift_event_vertical_only() {
        let event = LiftEvent::new(0, 100);
        assert!((event.distance - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_is_jump_below_threshold() {
        // JUMP_THRESHOLD_PX = 50.0
        assert!(!is_jump(0.0));
        assert!(!is_jump(25.0));
        assert!(!is_jump(49.9));
        assert!(!is_jump(50.0)); // Exactly at threshold is NOT a jump
    }

    #[test]
    fn test_is_jump_above_threshold() {
        assert!(is_jump(50.1));
        assert!(is_jump(51.0));
        assert!(is_jump(100.0));
        assert!(is_jump(1000.0));
    }

    #[test]
    fn test_is_jump_with_lift_event() {
        // Test integration of LiftEvent and is_jump
        let small_event = LiftEvent::new(3, 4); // distance = 5.0
        let large_event = LiftEvent::new(30, 40); // distance = 50.0
        let jump_event = LiftEvent::new(40, 40); // distance = ~56.57

        assert!(!is_jump(small_event.distance));
        assert!(!is_jump(large_event.distance)); // Exactly 50 is NOT a jump
        assert!(is_jump(jump_event.distance));
    }

    #[test]
    fn test_jump_threshold_constant() {
        // Verify the threshold is what we expect
        assert_eq!(JUMP_THRESHOLD_PX, 50.0);
    }

    #[test]
    fn test_idle_threshold_constant() {
        // Verify the idle threshold is what we expect
        assert_eq!(IDLE_THRESHOLD_MS, 100);
    }

    #[test]
    fn test_lift_event_large_values() {
        let event = LiftEvent::new(1000, 1000);
        let expected_distance = (1000.0_f64.powi(2) + 1000.0_f64.powi(2)).sqrt();
        assert!((event.distance - expected_distance).abs() < 0.001);
        assert!(is_jump(event.distance));
    }

    #[test]
    fn test_lift_event_asymmetric() {
        let event = LiftEvent::new(100, 1);
        // Distance should be dominated by x
        assert!(event.distance > 100.0);
        assert!(event.distance < 101.0);
    }
}
