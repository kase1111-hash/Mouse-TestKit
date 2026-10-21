//! USB Conflict Scanner
//!
//! Shows how each mouse is connected: the USB bus (controller) and hubs it
//! sits behind, the speed it runs at, and what else shares that path.
//!
//! Device types come from the *interface* descriptors
//! (`/sys/bus/usb/devices/1-2:1.0/bInterfaceClass`): almost every device
//! leaves the device-level `bDeviceClass` at 0, meaning "defined per
//! interface". A mouse is a HID interface (class 3) with boot protocol 2, a
//! keyboard one with protocol 1.
//!
//! Sharing a bus or controller with other devices is normal and is shown as
//! information only: the host controller reserves bandwidth for each
//! mouse's interrupt polling, so a busy disk or webcam on the same bus does
//! not delay it. Only what the connection really limits is pointed out.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

const SYSFS_USB_DEVICES: &str = "/sys/bus/usb/devices";

// USB class codes (usb.org "Defined Class Codes")
const CLASS_AUDIO: u8 = 0x01;
const CLASS_HID: u8 = 0x03;
const CLASS_MASS_STORAGE: u8 = 0x08;
const CLASS_HUB: u8 = 0x09;
const CLASS_VIDEO: u8 = 0x0e;
// HID boot interface protocols
const HID_PROTOCOL_KEYBOARD: u8 = 1;
const HID_PROTOCOL_MOUSE: u8 = 2;

/// USB low speed in Mbit/s. Low-speed interrupt endpoints may be polled at
/// most every 10 ms by the USB 2.0 spec (hosts round it to 8 ms), so a
/// low-speed mouse cannot report much faster than 125 Hz.
const LOW_SPEED_MBPS: f64 = 1.5;
/// USB full speed (USB 1.1) in Mbit/s: polling down to 1 ms (1000 Hz).
const FULL_SPEED_MBPS: f64 = 12.0;

pub fn scan() {
    println!("\n=== USB Conflict Scanner ===");
    println!("Shows how each mouse is connected and what shares its USB bus.\n");

    let devices = enumerate_usb_devices(Path::new(SYSFS_USB_DEVICES));

    if devices.is_empty() {
        if cfg!(target_os = "linux") {
            println!("No USB devices found or unable to read USB information.");
            println!(
                "Note: This feature requires read access to {}",
                SYSFS_USB_DEVICES
            );
        } else {
            println!("The USB scanner reads Linux sysfs and is only available on Linux.");
        }
    } else {
        print!("{}", render(&devices));
    }

    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}

/// The full scanner output for a set of devices.
fn render(devices: &[UsbDevice]) -> String {
    let mut out = String::new();
    let mut sorted: Vec<&UsbDevice> = devices.iter().collect();
    // Each bus starts with its root hub
    sorted.sort_by_key(|d| (d.bus_id, !d.is_root_hub(), d.sysfs_name.clone()));
    let mut buses: Vec<u8> = sorted.iter().map(|d| d.bus_id).collect();
    buses.dedup();

    out.push_str(&format!(
        "Found {} USB devices on {} buses\n\n",
        devices.len(),
        buses.len()
    ));

    let mice: Vec<&UsbDevice> = sorted.iter().copied().filter(|d| d.is_mouse()).collect();
    if mice.is_empty() {
        out.push_str("No device declares a USB boot-mouse interface. Mice on Bluetooth or PS/2,\n");
        out.push_str("and the few USB mice without the boot protocol, are not listed here.\n");
    }
    for mouse in mice {
        let report = assess_mouse(devices, mouse);
        out.push_str(&format!(
            "Mouse: {} ({:04x}:{:04x}, port {})\n",
            mouse.name, mouse.vendor_id, mouse.product_id, mouse.sysfs_name
        ));
        for note in &report.notes {
            out.push_str(&format!("  - {}\n", note));
        }
        for issue in &report.issues {
            out.push_str(&format!("  ! Possible issue: {}\n", issue));
        }
        if report.issues.is_empty() {
            out.push_str("  ✓ No USB connection problems found for this mouse.\n");
        }
        out.push('\n');
    }

    out.push_str(
        "Sharing a USB bus or controller with other devices is normal and not a problem\n",
    );
    out.push_str("by itself: the controller reserves bandwidth for each mouse's polling.\n");

    out.push_str("\n─────────────────────────────────────────────────────────────\n");
    out.push_str("All USB Devices by Bus:\n");
    out.push_str("─────────────────────────────────────────────────────────────\n");
    for bus in buses {
        out.push_str(&format!("\nBus {}:\n", bus));
        for device in sorted.iter().filter(|d| d.bus_id == bus) {
            out.push_str(&format!(
                "  [{:<9}] {:04x}:{:04x} {:>11} {} ({})\n",
                device.kind_label(),
                device.vendor_id,
                device.product_id,
                device.speed_label(),
                truncate_name(&device.name, 40).trim_end(),
                device.sysfs_name
            ));
        }
    }
    out
}

/// What the scanner says about one mouse.
pub struct MouseReport {
    /// Facts about the connection: bus, hubs, speed, neighbours.
    pub notes: Vec<String>,
    /// Things in the connection that may really affect the mouse.
    pub issues: Vec<String>,
}

/// Describe how `mouse` is connected, given every device on the system.
pub fn assess_mouse(devices: &[UsbDevice], mouse: &UsbDevice) -> MouseReport {
    let by_name: HashMap<&str, &UsbDevice> =
        devices.iter().map(|d| (d.sysfs_name.as_str(), d)).collect();
    let mut notes = Vec::new();
    let mut issues = Vec::new();

    // Hubs between the mouse and the controller, nearest first
    let mut hubs = Vec::new();
    let mut root = None;
    let mut parent = mouse.parent_name();
    while let Some(name) = parent {
        let Some(&hub) = by_name.get(name.as_str()) else {
            break;
        };
        if hub.is_root_hub() {
            root = Some(hub);
            break;
        }
        hubs.push(hub);
        parent = hub.parent_name();
    }

    let controller = match root {
        Some(r) if r.speed_mbps.is_some() => format!(" ({}, {})", r.name, r.speed_label()),
        Some(r) => format!(" ({})", r.name),
        None => String::new(),
    };
    if hubs.is_empty() {
        notes.push(format!(
            "Bus {}{}, plugged straight into the computer",
            mouse.bus_id, controller
        ));
    } else {
        let chain: Vec<String> = hubs
            .iter()
            .map(|h| match h.speed_mbps {
                Some(_) => format!("{} ({})", h.name, h.speed_label()),
                None => h.name.clone(),
            })
            .collect();
        notes.push(format!(
            "Bus {}{}, through hub: {}",
            mouse.bus_id,
            controller,
            chain.join(" <- ")
        ));
    }

    match mouse.speed_mbps {
        Some(s) if s <= LOW_SPEED_MBPS => notes.push(
            "Runs at USB low speed (1.5 Mbit/s), which limits polling to about 125 Hz. \
             Normal for basic office mice; a mouse sold as 500 Hz or faster should not run at \
             low speed, so check what sits between it and the computer (KVM switch, adapter)."
                .to_string(),
        ),
        Some(s) if s <= FULL_SPEED_MBPS => {
            notes.push("Runs at USB full speed (12 Mbit/s): polling up to 1000 Hz.".to_string())
        }
        Some(s) => notes.push(format!(
            "Runs at {} Mbit/s (USB high speed or faster): polling up to 8000 Hz.",
            s
        )),
        None => {}
    }

    // A USB 1.1 (full-speed) hub gives everything behind it one shared
    // 12 Mbit/s link. Streaming or storage traffic behind the same hub is
    // the one sharing case worth a look.
    for hub in hubs
        .iter()
        .filter(|h| h.speed_mbps.is_some_and(|s| s <= FULL_SPEED_MBPS))
    {
        let busy: Vec<String> = devices
            .iter()
            .filter(|d| d.sysfs_name != mouse.sysfs_name && d.is_behind(hub) && d.is_busy())
            .map(|d| format!("[{}] {}", d.kind_label(), d.name))
            .collect();
        if !busy.is_empty() {
            issues.push(format!(
                "shares the 12 Mbit/s (USB 1.1) hub \"{}\" with {}. Everything behind that hub shares one 12 Mbit/s link; if the stutter test shows problems, plug the mouse straight into the computer and compare.",
                hub.name,
                busy.join(", ")
            ));
        }
    }

    let neighbours: Vec<String> = devices
        .iter()
        .filter(|d| {
            d.bus_id == mouse.bus_id
                && d.sysfs_name != mouse.sysfs_name
                && !matches!(d.device_class, UsbClass::Hub)
        })
        .map(|d| format!("[{}] {}", d.kind_label(), d.name))
        .collect();
    if !neighbours.is_empty() {
        notes.push(format!(
            "Shares bus {} with: {} (normal)",
            mouse.bus_id,
            neighbours.join(", ")
        ));
    }

    MouseReport { notes, issues }
}

/// Pad or cut a name to `max_len` characters (not bytes, so product names
/// with non-ASCII characters cannot split a character).
fn truncate_name(name: &str, max_len: usize) -> String {
    if name.chars().count() <= max_len {
        format!("{:width$}", name, width = max_len)
    } else {
        let cut: String = name.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", cut)
    }
}

/// Read every USB device under `root` (normally `/sys/bus/usb/devices`),
/// with the interfaces listed beside it (`1-2:1.0` belongs to `1-2`,
/// `1-0:1.0` to root hub `usb1`).
fn enumerate_usb_devices(root: &Path) -> Vec<UsbDevice> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();

    let mut interfaces: HashMap<String, Vec<UsbInterface>> = HashMap::new();
    for name in names.iter().filter(|n| n.contains(':')) {
        if let (Some(owner), Some(iface)) =
            (interface_owner(name), parse_interface(&root.join(name)))
        {
            interfaces.entry(owner).or_default().push(iface);
        }
    }

    names
        .iter()
        .filter(|n| !n.contains(':'))
        .filter_map(|n| {
            let ifaces = interfaces.remove(n).unwrap_or_default();
            parse_usb_device(&root.join(n), n, ifaces)
        })
        .collect()
}

/// The device an interface directory belongs to: `1-2.3:1.0` -> `1-2.3`,
/// and `1-0:1.0` (a root hub's own interface) -> `usb1`.
fn interface_owner(name: &str) -> Option<String> {
    let device = name.split(':').next()?;
    match device.split_once('-') {
        Some((bus, "0")) => Some(format!("usb{}", bus)),
        Some(_) => Some(device.to_string()),
        None => None,
    }
}

fn parse_interface(path: &Path) -> Option<UsbInterface> {
    Some(UsbInterface {
        class: read_hex_file(&path.join("bInterfaceClass"))? as u8,
        subclass: read_hex_file(&path.join("bInterfaceSubClass")).unwrap_or(0) as u8,
        protocol: read_hex_file(&path.join("bInterfaceProtocol")).unwrap_or(0) as u8,
    })
}

/// Bus number from a sysfs device name: `usb3` -> 3, `3-1.2` -> 3.
fn bus_from_name(name: &str) -> Option<u8> {
    match name.strip_prefix("usb") {
        Some(n) => n.parse().ok(),
        None => name.split('-').next()?.parse().ok(),
    }
}

fn parse_usb_device(
    path: &Path,
    sysfs_name: &str,
    interfaces: Vec<UsbInterface>,
) -> Option<UsbDevice> {
    if !sysfs_name.starts_with("usb") && !sysfs_name.contains('-') {
        return None;
    }
    let vendor_id = read_hex_file(&path.join("idVendor"))?;
    let product_id = read_hex_file(&path.join("idProduct"))?;

    let name = read_string_file(&path.join("product"))
        .or_else(|| read_string_file(&path.join("manufacturer")))
        .unwrap_or_else(|| "Unknown Device".to_string());

    let device_class = read_hex_file(&path.join("bDeviceClass")).unwrap_or(0) as u8;

    let bus_id = read_string_file(&path.join("busnum"))
        .and_then(|s| s.parse().ok())
        .or_else(|| bus_from_name(sysfs_name))
        .unwrap_or(0);

    let speed_mbps = read_string_file(&path.join("speed")).and_then(|s| s.parse().ok());

    Some(UsbDevice {
        name,
        vendor_id,
        product_id,
        bus_id,
        device_class: classify(device_class, &interfaces),
        sysfs_name: sysfs_name.to_string(),
        speed_mbps,
        interfaces,
    })
}

/// Main class of a device. When several apply, the most bandwidth-hungry
/// wins: a webcam with a microphone is Video, a headset with volume buttons
/// is Audio, a mouse is HID.
fn classify(device_class: u8, interfaces: &[UsbInterface]) -> UsbClass {
    let has = |c: u8| device_class == c || interfaces.iter().any(|i| i.class == c);
    if has(CLASS_HUB) {
        UsbClass::Hub
    } else if has(CLASS_VIDEO) {
        UsbClass::Video
    } else if has(CLASS_AUDIO) {
        UsbClass::Audio
    } else if has(CLASS_MASS_STORAGE) {
        UsbClass::MassStorage
    } else if has(CLASS_HID) {
        UsbClass::Hid
    } else {
        UsbClass::Other
    }
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

/// One interface of a USB device (from a `1-2:1.0` directory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbInterface {
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
}

pub struct UsbDevice {
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus_id: u8,
    pub device_class: UsbClass,
    /// sysfs name: `usb1` for a root hub, `1-2.3` for port 3 of the hub on
    /// port 2 of bus 1.
    pub sysfs_name: String,
    /// Link speed in Mbit/s (1.5, 12, 480, 5000, ...).
    pub speed_mbps: Option<f64>,
    pub interfaces: Vec<UsbInterface>,
}

impl UsbDevice {
    pub fn is_root_hub(&self) -> bool {
        self.sysfs_name.starts_with("usb")
    }

    fn has_hid_protocol(&self, protocol: u8) -> bool {
        self.interfaces
            .iter()
            .any(|i| i.class == CLASS_HID && i.protocol == protocol)
    }

    pub fn is_mouse(&self) -> bool {
        self.has_hid_protocol(HID_PROTOCOL_MOUSE)
    }

    pub fn is_keyboard(&self) -> bool {
        self.has_hid_protocol(HID_PROTOCOL_KEYBOARD)
    }

    /// Streams audio or video, or moves bulk storage data.
    pub fn is_busy(&self) -> bool {
        matches!(
            self.device_class,
            UsbClass::Audio | UsbClass::Video | UsbClass::MassStorage
        )
    }

    /// sysfs name of the hub this device is plugged into.
    pub fn parent_name(&self) -> Option<String> {
        if self.is_root_hub() {
            return None;
        }
        let (bus, ports) = self.sysfs_name.split_once('-')?;
        Some(match ports.rsplit_once('.') {
            Some((upstream, _)) => format!("{}-{}", bus, upstream),
            None => format!("usb{}", bus),
        })
    }

    /// Whether this device is somewhere downstream of `hub`.
    pub fn is_behind(&self, hub: &UsbDevice) -> bool {
        if hub.is_root_hub() {
            !self.is_root_hub() && self.bus_id == hub.bus_id
        } else {
            self.sysfs_name.starts_with(&format!("{}.", hub.sysfs_name))
        }
    }

    fn kind_label(&self) -> &'static str {
        match (self.is_mouse(), self.is_keyboard()) {
            (true, true) => "Mouse+Kbd",
            (true, false) => "Mouse",
            (false, true) => "Keyboard",
            (false, false) => match self.device_class {
                UsbClass::Hid => "HID",
                UsbClass::MassStorage => "Storage",
                UsbClass::Audio => "Audio",
                UsbClass::Video => "Video",
                UsbClass::Hub => "Hub",
                UsbClass::Other => "Other",
            },
        }
    }

    fn speed_label(&self) -> String {
        match self.speed_mbps {
            Some(s) => format!("{} Mbit/s", s),
            None => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbClass {
    Hid, // Human Interface Device (mice, keyboards)
    MassStorage,
    Audio,
    Video,
    Hub,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_from_name() {
        assert_eq!(bus_from_name("usb1"), Some(1));
        assert_eq!(bus_from_name("usb12"), Some(12));
        assert_eq!(bus_from_name("3-1.2"), Some(3));
        assert_eq!(bus_from_name("1-0:1.0"), Some(1));
        assert_eq!(bus_from_name("power"), None);
    }

    #[test]
    fn test_interface_owner() {
        assert_eq!(interface_owner("1-2:1.0").as_deref(), Some("1-2"));
        assert_eq!(interface_owner("1-2.3:1.1").as_deref(), Some("1-2.3"));
        assert_eq!(interface_owner("2-0:1.0").as_deref(), Some("usb2"));
    }

    #[test]
    fn test_truncate_name_handles_non_ascii() {
        let name = "Souris sans fil – Édition très spéciale avec molette";
        let cut = truncate_name(name, 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.ends_with("..."));
        assert_eq!(truncate_name("Mouse", 8), "Mouse   ");
    }

    /// Fake `/sys/bus/usb/devices` tree in a temp dir. Interface names
    /// contain ':', which Windows does not allow in file names.
    #[cfg(unix)]
    mod sysfs {
        use super::super::*;
        use std::path::PathBuf;

        struct FakeSysfs {
            root: PathBuf,
        }

        impl FakeSysfs {
            fn new(tag: &str) -> Self {
                let root = std::env::temp_dir().join(format!(
                    "mouse-trap-usb-{}-{}",
                    tag,
                    std::process::id()
                ));
                let _ = fs::remove_dir_all(&root);
                fs::create_dir_all(&root).unwrap();
                Self { root }
            }

            fn write(&self, dir: &str, attrs: &[(&str, &str)]) {
                let path = self.root.join(dir);
                fs::create_dir_all(&path).unwrap();
                for (file, value) in attrs {
                    fs::write(path.join(file), format!("{}\n", value)).unwrap();
                }
            }

            /// A device directory. `busnum` is left out for root hubs so
            /// the name fallback (`usbN`) is exercised too.
            fn device(
                &self,
                name: &str,
                ids: (&str, &str),
                product: &str,
                class: &str,
                speed: &str,
            ) {
                let mut attrs = vec![
                    ("idVendor", ids.0),
                    ("idProduct", ids.1),
                    ("product", product),
                    ("bDeviceClass", class),
                    ("speed", speed),
                ];
                let bus = bus_from_name(name).unwrap().to_string();
                if !name.starts_with("usb") {
                    attrs.push(("busnum", &bus));
                }
                self.write(name, &attrs);
            }

            fn interface(&self, name: &str, class: &str, subclass: &str, protocol: &str) {
                self.write(
                    name,
                    &[
                        ("bInterfaceClass", class),
                        ("bInterfaceSubClass", subclass),
                        ("bInterfaceProtocol", protocol),
                    ],
                );
            }

            fn root_hub(&self, bus: u8, speed: &str) {
                let name = format!("usb{}", bus);
                self.device(&name, ("1d6b", "0002"), "xHCI Host Controller", "09", speed);
                self.interface(&format!("{}-0:1.0", bus), "09", "00", "00");
            }

            fn devices(&self) -> Vec<UsbDevice> {
                enumerate_usb_devices(&self.root)
            }
        }

        impl Drop for FakeSysfs {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        fn find<'a>(devices: &'a [UsbDevice], name: &str) -> &'a UsbDevice {
            devices.iter().find(|d| d.sysfs_name == name).unwrap()
        }

        /// A typical desktop: mouse and flash drive on the USB 2 root hub,
        /// a wireless receiver, and a webcam with microphone on bus 2.
        fn desktop(tag: &str) -> FakeSysfs {
            let fs = FakeSysfs::new(tag);
            fs.root_hub(1, "480");
            fs.root_hub(2, "480");
            // Wired mouse: device class 00, boot mouse interface
            fs.device(
                "1-2",
                ("046d", "c08b"),
                "G502 HERO Gaming Mouse",
                "00",
                "12",
            );
            fs.interface("1-2:1.0", "03", "01", "02");
            fs.interface("1-2:1.1", "03", "00", "00");
            // Wireless receiver: boot keyboard + boot mouse + vendor HID
            fs.device("1-3", ("046d", "c52b"), "USB Receiver", "00", "12");
            fs.interface("1-3:1.0", "03", "01", "01");
            fs.interface("1-3:1.1", "03", "01", "02");
            fs.interface("1-3:1.2", "03", "00", "00");
            // Flash drive
            fs.device("1-4", ("0781", "5567"), "Cruzer Blade", "00", "480");
            fs.interface("1-4:1.0", "08", "06", "50");
            // Webcam with microphone (miscellaneous device class)
            fs.device("2-1", ("046d", "0825"), "Webcam C270", "ef", "480");
            fs.interface("2-1:1.0", "0e", "01", "00");
            fs.interface("2-1:1.1", "0e", "02", "00");
            fs.interface("2-1:1.2", "01", "01", "00");
            fs.interface("2-1:1.3", "01", "02", "00");
            fs
        }

        #[test]
        fn classifies_devices_by_interface_class() {
            let fs = desktop("classify");
            let devices = fs.devices();
            assert_eq!(devices.len(), 6);

            let mouse = find(&devices, "1-2");
            assert!(mouse.is_mouse());
            assert!(!mouse.is_keyboard());
            assert_eq!(mouse.device_class, UsbClass::Hid);
            assert_eq!(mouse.bus_id, 1);
            assert_eq!(mouse.speed_mbps, Some(12.0));
            assert_eq!(mouse.interfaces.len(), 2);

            let receiver = find(&devices, "1-3");
            assert!(receiver.is_mouse() && receiver.is_keyboard());

            assert_eq!(find(&devices, "1-4").device_class, UsbClass::MassStorage);
            assert_eq!(find(&devices, "2-1").device_class, UsbClass::Video);
        }

        #[test]
        fn root_hubs_get_their_own_bus_number() {
            let fs = desktop("roothub");
            let devices = fs.devices();
            let usb1 = find(&devices, "usb1");
            let usb2 = find(&devices, "usb2");
            assert_eq!(usb1.bus_id, 1);
            assert_eq!(usb2.bus_id, 2);
            assert!(usb1.is_root_hub());
            assert_eq!(usb1.device_class, UsbClass::Hub);
            assert_eq!(find(&devices, "1-2").parent_name().as_deref(), Some("usb1"));
        }

        #[test]
        fn sharing_a_bus_is_information_not_a_problem() {
            let fs = desktop("shared");
            let devices = fs.devices();
            let report = assess_mouse(&devices, find(&devices, "1-2"));
            assert!(report.issues.is_empty(), "{:?}", report.issues);
            assert!(report
                .notes
                .iter()
                .any(|n| n.starts_with("Shares bus 1") && n.contains("[Storage] Cruzer Blade")));

            let text = render(&devices);
            assert!(text.contains("Mouse: G502 HERO Gaming Mouse"));
            assert!(text.contains("Mouse: USB Receiver"));
            assert!(text.contains("No USB connection problems found"));
            assert!(!text.contains("Possible issue"));
        }

        #[test]
        fn full_speed_hub_shared_with_streaming_device_is_flagged() {
            let fs = FakeSysfs::new("fshub");
            fs.root_hub(1, "480");
            // Old USB 1.1 hub on port 1 with a mouse and a USB headset behind it
            fs.device("1-1", ("05e3", "0605"), "USB 1.1 Hub", "09", "12");
            fs.interface("1-1:1.0", "09", "00", "00");
            fs.device("1-1.1", ("413c", "301a"), "Dell MS116 Mouse", "00", "1.5");
            fs.interface("1-1.1:1.0", "03", "01", "02");
            fs.device("1-1.2", ("1395", "0025"), "Headset", "00", "12");
            fs.interface("1-1.2:1.0", "01", "01", "00");
            fs.interface("1-1.2:1.3", "03", "00", "00");
            let devices = fs.devices();
            let report = assess_mouse(&devices, find(&devices, "1-1.1"));
            assert_eq!(report.issues.len(), 1, "{:?}", report.issues);
            assert!(report.issues[0].contains("[Audio] Headset"));
            assert!(report
                .notes
                .iter()
                .any(|n| n.contains("through hub: USB 1.1 Hub")));
            // Low speed is a limit worth knowing, not a fault
            assert!(report.notes.iter().any(|n| n.contains("about 125 Hz")));
        }

        #[test]
        fn high_speed_hub_shared_with_storage_is_fine() {
            let fs = FakeSysfs::new("hshub");
            fs.root_hub(1, "480");
            fs.device("1-1", ("05e3", "0610"), "USB2.0 Hub", "09", "480");
            fs.interface("1-1:1.0", "09", "00", "02");
            fs.device("1-1.1", ("1532", "0084"), "Razer DeathAdder", "00", "12");
            fs.interface("1-1.1:1.0", "03", "01", "02");
            fs.device("1-1.4", ("0bc2", "2322"), "Portable Disk", "00", "480");
            fs.interface("1-1.4:1.0", "08", "06", "50");
            let devices = fs.devices();
            let mouse = find(&devices, "1-1.1");
            assert!(mouse.is_behind(find(&devices, "1-1")));
            assert!(!find(&devices, "1-1").is_behind(mouse));
            let report = assess_mouse(&devices, mouse);
            assert!(report.issues.is_empty(), "{:?}", report.issues);
            assert!(report.notes.iter().any(|n| n.contains("up to 1000 Hz")));
        }

        #[test]
        fn no_mouse_and_no_sysfs_are_reported_plainly() {
            let fs = FakeSysfs::new("nomouse");
            fs.root_hub(1, "480");
            fs.device("1-4", ("0781", "5567"), "Cruzer Blade", "00", "480");
            fs.interface("1-4:1.0", "08", "06", "50");
            let text = render(&fs.devices());
            assert!(text.contains("No device declares a USB boot-mouse interface"));
            assert!(text.contains("[Storage  ]"));

            let missing = fs.root.join("does-not-exist");
            assert!(enumerate_usb_devices(&missing).is_empty());
        }
    }
}
