/// USB Conflict Scanner
/// Shows devices connected to the same USB controller/hub

use std::fs;
use std::path::Path;

pub fn scan() {
    println!("\n=== USB Conflict Scanner ===");
    println!("Scanning for devices on shared USB buses...\n");

    let devices = enumerate_usb_devices();

    if devices.is_empty() {
        println!("No USB devices found or unable to read USB information.");
        println!("Note: This feature requires read access to /sys/bus/usb/devices");
        println!("\nPress Enter to return to menu...");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        return;
    }

    // Group devices by bus
    let mut buses: std::collections::HashMap<u8, Vec<&UsbDevice>> = std::collections::HashMap::new();
    for device in &devices {
        buses.entry(device.bus_id).or_default().push(device);
    }

    // Find HID devices (mice, keyboards)
    let hid_devices: Vec<&UsbDevice> = devices.iter()
        .filter(|d| matches!(d.device_class, UsbClass::Hid))
        .collect();

    println!("Found {} USB devices on {} buses\n", devices.len(), buses.len());

    // Show HID devices and potential conflicts
    if !hid_devices.is_empty() {
        println!("┌─────────────────────────────────────────────────────────┐");
        println!("│ HID Devices (Mice/Keyboards)                            │");
        println!("├─────────────────────────────────────────────────────────┤");

        for device in &hid_devices {
            println!("│ Bus {:02}: {} ", device.bus_id, truncate_name(&device.name, 45));
            println!("│        VID:{:04x} PID:{:04x}", device.vendor_id, device.product_id);
        }
        println!("└─────────────────────────────────────────────────────────┘\n");
    }

    // Check for potential conflicts
    let mut has_conflicts = false;

    for (bus_id, bus_devices) in &buses {
        let hid_count = bus_devices.iter()
            .filter(|d| matches!(d.device_class, UsbClass::Hid))
            .count();

        let high_bandwidth_count = bus_devices.iter()
            .filter(|d| matches!(d.device_class, UsbClass::MassStorage | UsbClass::Audio | UsbClass::Video))
            .count();

        if hid_count > 0 && high_bandwidth_count > 0 {
            has_conflicts = true;
            println!("⚠ Potential conflict on Bus {}:", bus_id);
            println!("  {} HID device(s) sharing bus with {} high-bandwidth device(s)",
                hid_count, high_bandwidth_count);

            for device in bus_devices {
                let class_str = match device.device_class {
                    UsbClass::Hid => "[HID]",
                    UsbClass::MassStorage => "[Storage]",
                    UsbClass::Audio => "[Audio]",
                    UsbClass::Video => "[Video]",
                    UsbClass::Hub => "[Hub]",
                    UsbClass::Other => "[Other]",
                };
                println!("    {} {}", class_str, device.name);
            }
            println!();
        }
    }

    if !has_conflicts {
        println!("✓ No USB conflicts detected.");
        println!("  HID devices are not sharing buses with high-bandwidth devices.");
    }

    // Show all devices by bus
    println!("\n─────────────────────────────────────────────────────────────");
    println!("All USB Devices by Bus:");
    println!("─────────────────────────────────────────────────────────────");

    let mut sorted_buses: Vec<_> = buses.iter().collect();
    sorted_buses.sort_by_key(|(id, _)| *id);

    for (bus_id, bus_devices) in sorted_buses {
        println!("\nBus {}:", bus_id);
        for device in bus_devices {
            let class_str = match device.device_class {
                UsbClass::Hid => "HID     ",
                UsbClass::MassStorage => "Storage ",
                UsbClass::Audio => "Audio   ",
                UsbClass::Video => "Video   ",
                UsbClass::Hub => "Hub     ",
                UsbClass::Other => "Other   ",
            };
            println!("  [{}] {:04x}:{:04x} {}",
                class_str, device.vendor_id, device.product_id, device.name);
        }
    }

    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}

fn truncate_name(name: &str, max_len: usize) -> String {
    if name.len() <= max_len {
        format!("{:width$}", name, width = max_len)
    } else {
        format!("{}...", &name[..max_len - 3])
    }
}

fn enumerate_usb_devices() -> Vec<UsbDevice> {
    let mut devices = Vec::new();
    let usb_path = Path::new("/sys/bus/usb/devices");

    if !usb_path.exists() {
        return devices;
    }

    if let Ok(entries) = fs::read_dir(usb_path) {
        for entry in entries.flatten() {
            let path = entry.path();

            // Skip non-device entries (like usb1, usb2 root hubs without -)
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.contains('-') && !name.starts_with("usb") {
                continue;
            }

            if let Some(device) = parse_usb_device(&path) {
                devices.push(device);
            }
        }
    }

    devices
}

fn parse_usb_device(path: &Path) -> Option<UsbDevice> {
    let vendor_id = read_hex_file(&path.join("idVendor"))?;
    let product_id = read_hex_file(&path.join("idProduct"))?;

    let name = read_string_file(&path.join("product"))
        .or_else(|| read_string_file(&path.join("manufacturer")))
        .unwrap_or_else(|| "Unknown Device".to_string());

    let device_class = read_hex_file(&path.join("bDeviceClass")).unwrap_or(0);
    let interface_class = read_hex_file(&path.join("bInterfaceClass")).unwrap_or(0);

    // Determine device class
    let class = match (device_class, interface_class) {
        (0x03, _) | (_, 0x03) => UsbClass::Hid,
        (0x08, _) | (_, 0x08) => UsbClass::MassStorage,
        (0x01, _) | (_, 0x01) => UsbClass::Audio,
        (0x0e, _) | (_, 0x0e) => UsbClass::Video,
        (0x09, _) | (_, 0x09) => UsbClass::Hub,
        _ => UsbClass::Other,
    };

    // Extract bus ID from path name (e.g., "1-2" -> bus 1)
    let bus_id = path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.split('-').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);

    Some(UsbDevice {
        name,
        vendor_id,
        product_id,
        bus_id,
        device_class: class,
    })
}

fn read_hex_file(path: &Path) -> Option<u16> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim(), 16).ok())
}

fn read_string_file(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub struct UsbDevice {
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus_id: u8,
    pub device_class: UsbClass,
}

pub enum UsbClass {
    Hid,          // Human Interface Device (mice, keyboards)
    MassStorage,
    Audio,
    Video,
    Hub,
    Other,
}

pub struct UsbBus {
    pub id: u8,
    pub devices: Vec<UsbDevice>,
}

impl UsbBus {
    pub fn has_conflicts(&self) -> bool {
        let has_hid = self.devices.iter().any(|d| matches!(d.device_class, UsbClass::Hid));
        let has_high_bandwidth = self.devices.iter().any(|d|
            matches!(d.device_class, UsbClass::MassStorage | UsbClass::Audio | UsbClass::Video)
        );
        has_hid && has_high_bandwidth
    }
}
