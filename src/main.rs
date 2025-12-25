#[cfg(target_os = "linux")]
mod tests;
mod usb;
mod display;
#[cfg(target_os = "linux")]
mod input;

#[cfg(target_os = "linux")]
use std::io::{self, Write};

#[cfg(target_os = "linux")]
fn main() {
    println!("╔════════════════════════════════════╗");
    println!("║        Mouse-TestKit v0.1.0        ║");
    println!("║     Mouse Testing Utility          ║");
    println!("╚════════════════════════════════════╝");
    println!();

    loop {
        print_menu();

        let choice = get_input("Select option: ");

        match choice.trim() {
            "1" => tests::stutter::run(),
            "2" => tests::polling::run(),
            "3" => usb::conflicts::scan(),
            "4" => tests::click_response::run(),
            "5" => tests::click_sticky::run(),
            "6" => tests::liftoff::run(),
            "7" => tests::dpi::run(),
            "8" => tests::angle_snap::run(),
            "9" => tests::acceleration::run(),
            "10" => tests::double_click::run(),
            "11" => tests::jitter::run(),
            "12" => tests::standard::run_all(),
            "0" => {
                println!("Exiting Mouse-TestKit. Goodbye!");
                break;
            }
            _ => println!("Invalid option. Please try again."),
        }
        println!();
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("╔════════════════════════════════════╗");
    println!("║        Mouse-TestKit v0.1.0        ║");
    println!("║     Mouse Testing Utility          ║");
    println!("╚════════════════════════════════════╝");
    println!();
    println!("The CLI version requires Linux for raw input access.");
    println!("Please use mouse-testkit-gui instead for the graphical interface.");
    println!();
    println!("Run: cargo run --bin mouse-testkit-gui");
}

#[cfg(target_os = "linux")]
fn print_menu() {
    println!("┌─────────────────────────────────────┐");
    println!("│              Main Menu              │");
    println!("├─────────────────────────────────────┤");
    println!("│  1. Stutter Detection Test          │");
    println!("│  2. Polling Rate Monitor            │");
    println!("│  3. USB Conflict Scanner            │");
    println!("│  4. Click Response Test             │");
    println!("│  5. Click Stickiness Test           │");
    println!("│  6. Lift-Off Jump Test              │");
    println!("├─────────────────────────────────────┤");
    println!("│  7. DPI Accuracy Test               │");
    println!("│  8. Angle Snapping Detection        │");
    println!("│  9. Acceleration Detection          │");
    println!("│ 10. Double-Click Test               │");
    println!("│ 11. Jitter Test                     │");
    println!("├─────────────────────────────────────┤");
    println!("│ 12. Run All Standard Tests          │");
    println!("│  0. Exit                            │");
    println!("└─────────────────────────────────────┘");
}

#[cfg(target_os = "linux")]
fn get_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input
}
