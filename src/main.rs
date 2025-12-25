mod tests;
mod usb;
mod display;
mod input;

use std::io::{self, Write};

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
            "7" => tests::standard::run_all(),
            "0" => {
                println!("Exiting Mouse-TestKit. Goodbye!");
                break;
            }
            _ => println!("Invalid option. Please try again."),
        }
        println!();
    }
}

fn print_menu() {
    println!("┌────────────────────────────────────┐");
    println!("│              Main Menu             │");
    println!("├────────────────────────────────────┤");
    println!("│  1. Stutter Detection Test         │");
    println!("│  2. Polling Rate Monitor           │");
    println!("│  3. USB Conflict Scanner           │");
    println!("│  4. Click Response Test            │");
    println!("│  5. Click Stickiness Test          │");
    println!("│  6. Lift-Off Jump Test             │");
    println!("│  7. Run All Standard Tests         │");
    println!("│  0. Exit                           │");
    println!("└────────────────────────────────────┘");
}

fn get_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input
}
