//! Input handling module for Windows
//!
//! Provides mouse device detection and raw event reading via the Windows Raw Input API.
//! This module handles:
//! - Enumerating connected mouse devices
//! - Registering for raw input events
//! - Running a background message loop to capture input
//! - Parsing raw mouse data into structured MouseEvent types
//!
//! # Architecture
//!
//! Uses a background thread with a hidden window to receive WM_INPUT messages.
//! Events are sent via an mpsc channel to the main thread for processing.

use std::mem;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use winapi::um::winuser::{
    GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices,
    RAWINPUTDEVICE, RAWINPUTDEVICELIST, RIDEV_INPUTSINK, RIDI_DEVICENAME, RIM_TYPEMOUSE,
};
use winapi::shared::hidusage::{HID_USAGE_GENERIC_MOUSE, HID_USAGE_PAGE_GENERIC};
use winapi::shared::minwindef::UINT;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetRawInputData,
    PeekMessageW, RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    HRAWINPUT, MSG, PM_REMOVE, RAWINPUT, RAWINPUTHEADER, RID_INPUT, WM_INPUT,
    WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

// Re-export canonical types from the shared library crate.
pub use mouse_testkit::types::{MouseEvent, MouseButton};

/// Represents a connected mouse device on Windows
pub struct MouseDevice {
    pub name: String,
    pub handle: usize,
    receiver: Receiver<RawMouseData>,
    _thread_handle: Option<thread::JoinHandle<()>>,
}

/// Raw mouse data from Windows
#[derive(Clone, Debug)]
pub struct RawMouseData {
    pub dx: i32,
    pub dy: i32,
    pub button_flags: u16,
    pub button_data: u16,
}

impl MouseDevice {
    /// Fetch pending events (non-blocking)
    pub fn fetch_events(&mut self) -> Result<Vec<MouseEvent>, std::io::Error> {
        let mut events = Vec::new();

        while let Ok(data) = self.receiver.try_recv() {
            // Convert raw data to MouseEvents
            if data.dx != 0 || data.dy != 0 {
                events.push(MouseEvent::Move { dx: data.dx, dy: data.dy });
            }

            // Check button flags
            // RI_MOUSE_LEFT_BUTTON_DOWN = 0x0001
            if data.button_flags & 0x0001 != 0 {
                events.push(MouseEvent::ButtonPress(MouseButton::Left));
            }
            // RI_MOUSE_LEFT_BUTTON_UP = 0x0002
            if data.button_flags & 0x0002 != 0 {
                events.push(MouseEvent::ButtonRelease(MouseButton::Left));
            }
            // RI_MOUSE_RIGHT_BUTTON_DOWN = 0x0004
            if data.button_flags & 0x0004 != 0 {
                events.push(MouseEvent::ButtonPress(MouseButton::Right));
            }
            // RI_MOUSE_RIGHT_BUTTON_UP = 0x0008
            if data.button_flags & 0x0008 != 0 {
                events.push(MouseEvent::ButtonRelease(MouseButton::Right));
            }
            // RI_MOUSE_MIDDLE_BUTTON_DOWN = 0x0010
            if data.button_flags & 0x0010 != 0 {
                events.push(MouseEvent::ButtonPress(MouseButton::Middle));
            }
            // RI_MOUSE_MIDDLE_BUTTON_UP = 0x0020
            if data.button_flags & 0x0020 != 0 {
                events.push(MouseEvent::ButtonRelease(MouseButton::Middle));
            }
            // RI_MOUSE_WHEEL = 0x0400
            if data.button_flags & 0x0400 != 0 {
                let delta = data.button_data as i16 as i32 / 120; // WHEEL_DELTA
                events.push(MouseEvent::Scroll { delta });
            }
        }

        Ok(events)
    }
}

/// Device info for enumeration
#[derive(Debug, Clone)]
pub struct MouseDeviceInfo {
    pub name: String,
    pub handle: usize,
}

/// Find all mouse devices in the system
pub fn find_mouse_devices() -> Vec<MouseDeviceInfo> {
    let mut mice = Vec::new();

    unsafe {
        // Get device count
        let mut device_count: UINT = 0;
        let size = mem::size_of::<RAWINPUTDEVICELIST>() as UINT;

        if GetRawInputDeviceList(ptr::null_mut(), &mut device_count, size) != 0 {
            eprintln!("Error: Failed to get raw input device count");
            return mice;
        }

        if device_count == 0 {
            return mice;
        }

        // Get device list
        let mut devices: Vec<RAWINPUTDEVICELIST> = vec![mem::zeroed(); device_count as usize];
        let result = GetRawInputDeviceList(devices.as_mut_ptr(), &mut device_count, size);

        if result == u32::MAX {
            eprintln!("Error: Failed to enumerate raw input devices");
            return mice;
        }

        // Filter for mice
        for device in devices.iter().take(result as usize) {
            if device.dwType != RIM_TYPEMOUSE {
                continue;
            }

            // Get device name
            let mut name_size: UINT = 0;
            GetRawInputDeviceInfoW(
                device.hDevice,
                RIDI_DEVICENAME,
                ptr::null_mut(),
                &mut name_size,
            );

            let mut name_buf: Vec<u16> = vec![0; name_size as usize];
            GetRawInputDeviceInfoW(
                device.hDevice,
                RIDI_DEVICENAME,
                name_buf.as_mut_ptr() as *mut _,
                &mut name_size,
            );

            let name = String::from_utf16_lossy(&name_buf)
                .trim_end_matches('\0')
                .to_string();

            // Extract friendly name from path
            let friendly_name = extract_friendly_name(&name);

            mice.push(MouseDeviceInfo {
                name: friendly_name,
                handle: device.hDevice as usize,
            });
        }
    }

    mice
}

/// Extract a user-friendly name from the device path
fn extract_friendly_name(path: &str) -> String {
    // Windows device paths look like: \\?\HID#VID_046D&PID_C52B#...
    // Try to extract VID/PID for identification
    if let Some(hid_start) = path.find("HID#") {
        let remainder = &path[hid_start + 4..];
        if let Some(hash_pos) = remainder.find('#') {
            let vid_pid = &remainder[..hash_pos];
            return format!("Mouse ({})", vid_pid.replace('&', " "));
        }
    }

    // Fallback: use a truncated path
    if path.len() > 50 {
        format!("Mouse ({}...)", &path[..47])
    } else {
        format!("Mouse ({})", path)
    }
}

/// Select a mouse device interactively
pub fn select_mouse() -> Option<MouseDevice> {
    let mice = find_mouse_devices();

    if mice.is_empty() {
        println!();
        println!("╔═══════════════════════════════════════════════════════════════╗");
        println!("║                    No Mouse Devices Found                     ║");
        println!("╠═══════════════════════════════════════════════════════════════╣");
        println!("║ This usually means one of the following:                      ║");
        println!("║                                                               ║");
        println!("║ 1. No mouse connected - Check your USB connection             ║");
        println!("║                                                               ║");
        println!("║ 2. Mouse driver issue - Try reconnecting the device           ║");
        println!("║                                                               ║");
        println!("║ 3. Administrator access may be required for some devices      ║");
        println!("╚═══════════════════════════════════════════════════════════════╝");
        println!();
        return None;
    }

    // For simplicity, select the first mouse (or let user choose if multiple)
    let selected = if mice.len() == 1 {
        println!("Found mouse: {}", mice[0].name);
        &mice[0]
    } else {
        println!("Found {} mouse devices:\n", mice.len());
        for (i, mouse) in mice.iter().enumerate() {
            println!("  {}. {}", i + 1, mouse.name);
        }

        println!("\nSelect device (1-{}), or 0 to cancel: ", mice.len());

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            eprintln!("Error reading input");
            return None;
        }

        let choice: usize = match input.trim().parse() {
            Ok(n) => n,
            Err(_) => {
                println!("Invalid selection.");
                return None;
            }
        };

        if choice == 0 {
            println!("Selection cancelled.");
            return None;
        }

        if choice > mice.len() {
            println!("Invalid selection.");
            return None;
        }

        &mice[choice - 1]
    };

    // Create input receiver thread
    let (sender, receiver) = mpsc::channel();

    let thread_handle = thread::spawn(move || {
        run_input_loop(sender);
    });

    Some(MouseDevice {
        name: selected.name.clone(),
        handle: selected.handle,
        receiver,
        _thread_handle: Some(thread_handle),
    })
}

/// Run the Raw Input message loop in a background thread
fn run_input_loop(sender: Sender<RawMouseData>) {
    unsafe {
        // Register window class
        let class_name: Vec<u16> = "MouseTestKitInput\0".encode_utf16().collect();
        let hinstance = GetModuleHandleW(ptr::null());

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: ptr::null_mut(),
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wc);

        // Create message-only window
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            ptr::null(),
            WS_OVERLAPPEDWINDOW,
            0, 0, 0, 0,
            ptr::null_mut(),
            ptr::null_mut(),
            hinstance,
            ptr::null_mut(),
        );

        if hwnd.is_null() {
            eprintln!("Failed to create input window");
            return;
        }

        // Register for raw mouse input
        let rid = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_MOUSE,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };

        if RegisterRawInputDevices(&rid, 1, mem::size_of::<RAWINPUTDEVICE>() as UINT) == 0 {
            eprintln!("Failed to register for raw input");
            return;
        }

        // Message loop
        let mut msg: MSG = mem::zeroed();
        loop {
            // Non-blocking peek
            if PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_INPUT {
                    process_raw_input(msg.lParam as HRAWINPUT, &sender);
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                // Small sleep to avoid busy loop
                thread::sleep(std::time::Duration::from_micros(100));
            }

            // Check if channel is closed
            if sender.send(RawMouseData { dx: 0, dy: 0, button_flags: 0, button_data: 0 }).is_err() {
                break;
            }
        }
    }
}

/// Process a WM_INPUT message
unsafe fn process_raw_input(handle: HRAWINPUT, sender: &Sender<RawMouseData>) {
    let mut size: UINT = 0;

    GetRawInputData(
        handle,
        RID_INPUT,
        ptr::null_mut(),
        &mut size,
        mem::size_of::<RAWINPUTHEADER>() as UINT,
    );

    if size == 0 {
        return;
    }

    let mut buffer: Vec<u8> = vec![0; size as usize];

    if GetRawInputData(
        handle,
        RID_INPUT,
        buffer.as_mut_ptr() as *mut _,
        &mut size,
        mem::size_of::<RAWINPUTHEADER>() as UINT,
    ) != size
    {
        return;
    }

    let raw = &*(buffer.as_ptr() as *const RAWINPUT);

    if raw.header.dwType == RIM_TYPEMOUSE {
        let mouse = raw.data.mouse();

        let data = RawMouseData {
            dx: mouse.lLastX,
            dy: mouse.lLastY,
            button_flags: mouse.usButtonFlags,
            button_data: mouse.usButtonData,
        };

        // Only send if there's actual data
        if data.dx != 0 || data.dy != 0 || data.button_flags != 0 {
            let _ = sender.send(data);
        }
    }
}

/// Parse a raw event into a MouseEvent (compatibility with evdev API)
pub fn parse_event(data: &RawMouseData) -> Option<MouseEvent> {
    if data.dx != 0 || data.dy != 0 {
        Some(MouseEvent::Move { dx: data.dx, dy: data.dy })
    } else {
        None
    }
}
