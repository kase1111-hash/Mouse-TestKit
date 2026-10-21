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
//! The thread ends when the mouse is removed (`WM_INPUT_DEVICE_CHANGE`) or
//! reading fails; the closed channel then tells the test the mouse is gone.

use std::collections::VecDeque;
use std::mem;
use std::ptr;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use winapi::shared::hidusage::{HID_USAGE_GENERIC_MOUSE, HID_USAGE_PAGE_GENERIC};
use winapi::shared::minwindef::UINT;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetRawInputData,
    RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GIDC_REMOVAL, HRAWINPUT, MSG,
    RAWINPUT, RAWINPUTHEADER, RID_INPUT, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};
use winapi::um::winuser::{
    GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterRawInputDevices, RAWINPUTDEVICE,
    RAWINPUTDEVICELIST, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDI_DEVICENAME, RIM_TYPEMOUSE,
};

/// `RAWMOUSE::usFlags` bit for absolute coordinates.
const MOUSE_MOVE_ABSOLUTE: u16 = 0x0001;

// Re-export canonical types from the shared library crate.
pub use mouse_testkit::types::{DeviceLost, InputBatch, MouseButton, MouseEvent, TimedEvent};

/// A selected mouse on Windows: its reports arrive from the raw-input thread
/// started by [`select_mouse`]. Read it with [`read_events`].
pub struct MouseDevice {
    receiver: Receiver<RawMouseData>,
    /// Reports received while waiting and not yet returned.
    pending: VecDeque<RawMouseData>,
    /// The raw-input thread has ended: the mouse was removed or reading
    /// failed.
    gone: bool,
    /// Wheel movement not yet adding up to a whole notch (high-resolution
    /// wheels report fractions of `WHEEL_DELTA`).
    wheel_remainder: i32,
    _thread_handle: Option<thread::JoinHandle<()>>,
}

/// Raw mouse data from Windows
#[derive(Clone, Debug)]
pub struct RawMouseData {
    pub dx: i32,
    pub dy: i32,
    pub button_flags: u16,
    pub button_data: u16,
    /// When the raw-input thread received the report.
    pub time: Instant,
}

/// Button transition flags in `RAWMOUSE::usButtonFlags`, in event order.
const BUTTON_FLAGS: [(u16, MouseButton, bool); 10] = [
    (0x0001, MouseButton::Left, true),
    (0x0002, MouseButton::Left, false),
    (0x0004, MouseButton::Right, true),
    (0x0008, MouseButton::Right, false),
    (0x0010, MouseButton::Middle, true),
    (0x0020, MouseButton::Middle, false),
    (0x0040, MouseButton::Side, true),
    (0x0080, MouseButton::Side, false),
    (0x0100, MouseButton::Extra, true),
    (0x0200, MouseButton::Extra, false),
];
/// RI_MOUSE_WHEEL: vertical wheel movement in `usButtonData`.
const RI_MOUSE_WHEEL: u16 = 0x0400;
/// One wheel notch.
const WHEEL_DELTA: i32 = 120;

impl MouseDevice {
    /// Move everything the thread has sent into `pending`, noting when the
    /// thread has ended.
    fn take_sent(&mut self) {
        loop {
            match self.receiver.try_recv() {
                Ok(data) => self.pending.push_back(data),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.gone = true;
                    break;
                }
            }
        }
    }

    /// Turn the pending reports into events.
    fn take_events(&mut self) -> Vec<TimedEvent> {
        let mut events = Vec::new();

        while let Some(data) = self.pending.pop_front() {
            let mut push = |event| {
                events.push(TimedEvent {
                    time: data.time,
                    event,
                })
            };
            if data.dx != 0 || data.dy != 0 {
                push(MouseEvent::Move {
                    dx: data.dx,
                    dy: data.dy,
                });
            }
            for (flag, button, pressed) in BUTTON_FLAGS {
                if data.button_flags & flag != 0 {
                    push(if pressed {
                        MouseEvent::ButtonPress(button)
                    } else {
                        MouseEvent::ButtonRelease(button)
                    });
                }
            }
            if data.button_flags & RI_MOUSE_WHEEL != 0 {
                let delta = data.button_data as i16 as i32;
                if delta.signum() != self.wheel_remainder.signum() {
                    self.wheel_remainder = 0;
                }
                self.wheel_remainder += delta;
                let notches = self.wheel_remainder / WHEEL_DELTA;
                if notches != 0 {
                    self.wheel_remainder -= notches * WHEEL_DELTA;
                    push(MouseEvent::Scroll { delta: notches });
                }
            }
        }

        events
    }

    /// Fetch pending events (non-blocking), each stamped with the time the
    /// raw-input thread received it. Prefer [`read_events`]: here a removed
    /// mouse only shows as missing events.
    #[allow(dead_code)]
    pub fn fetch_timed_events(&mut self) -> Vec<TimedEvent> {
        self.take_sent();
        self.take_events()
    }
}

/// Read the mouse: wait up to `wait` for input (not at all when some is
/// already pending), then return everything pending, each event stamped
/// with the time the raw-input thread received it (same signature as the
/// Linux `input::read_events`, so CLI tests need no `cfg`).
///
/// The raw-input thread drains Windows' queue as reports arrive, so no
/// batch has `lost_data`. Returns [`DeviceLost`] once the mouse was removed
/// (or the thread failed) and everything received before was returned.
pub fn read_events(device: &mut MouseDevice, wait: Duration) -> Result<InputBatch, DeviceLost> {
    device.take_sent();
    if device.pending.is_empty() && !device.gone && !wait.is_zero() {
        match device.receiver.recv_timeout(wait) {
            Ok(data) => device.pending.push_back(data),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => device.gone = true,
        }
        device.take_sent();
    }
    if device.pending.is_empty() && device.gone {
        return Err(DeviceLost);
    }
    Ok(InputBatch {
        events: device.take_events(),
        lost_data: false,
    })
}

/// Fetch pending events with the time each was received (same signature as
/// the Linux `input::fetch_timed_events`). Prefer [`read_events`].
#[allow(dead_code)]
pub fn fetch_timed_events(device: &mut MouseDevice) -> Vec<TimedEvent> {
    device.fetch_timed_events()
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
    // Windows device paths look like:
    // \\?\HID#VID_046D&PID_C52B&MI_00#7&2a1b3c&0&0000#{...}
    // VID/PID identify the model; the instance ID after it tells two units
    // of the same model apart (it depends on the port they are plugged in).
    if let Some(hid_start) = path.find("HID#") {
        let remainder = &path[hid_start + 4..];
        if let Some((vid_pid, rest)) = remainder.split_once('#') {
            let vid_pid = vid_pid.replace('&', " ");
            return match rest.split('#').next().filter(|i| !i.is_empty()) {
                Some(instance) => format!("Mouse ({}, instance {})", vid_pid, instance),
                None => format!("Mouse ({})", vid_pid),
            };
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
    let (ready_sender, ready) = mpsc::channel();
    let device_handle = selected.handle;

    let thread_handle = thread::spawn(move || {
        run_input_loop(sender, device_handle, ready_sender);
    });
    if ready.recv() != Ok(true) {
        eprintln!("Error: could not start reading the mouse (Raw Input registration failed).");
        return None;
    }

    Some(MouseDevice {
        receiver,
        pending: VecDeque::new(),
        gone: false,
        wheel_remainder: 0,
        _thread_handle: Some(thread_handle),
    })
}

/// Run the Raw Input message loop in a background thread, forwarding reports
/// from the device with the given raw-input handle only (Raw Input delivers
/// every mouse and touchpad to the same window). Reports through `ready`
/// whether registration succeeded. Returns, closing the channel, when the
/// device is removed, the test has dropped the receiver, or the message
/// loop fails.
fn run_input_loop(sender: Sender<RawMouseData>, device_handle: usize, ready: Sender<bool>) {
    unsafe {
        // Register window class
        let class_name: Vec<u16> = "MouseTrapInput\0".encode_utf16().collect();
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
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            hinstance,
            ptr::null_mut(),
        );

        if hwnd.is_null() {
            eprintln!("Failed to create input window");
            let _ = ready.send(false);
            return;
        }

        // Register for raw mouse input. DEVNOTIFY: WM_INPUT_DEVICE_CHANGE
        // when a mouse is plugged in or removed.
        let rid = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_MOUSE,
            dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
            hwndTarget: hwnd,
        };

        if RegisterRawInputDevices(&rid, 1, mem::size_of::<RAWINPUTDEVICE>() as UINT) == 0 {
            eprintln!("Failed to register for raw input");
            let _ = ready.send(false);
            return;
        }
        let _ = ready.send(true);

        // Blocking message loop — no heartbeat or busy-wait needed.
        // GetMessageW blocks until a message arrives, yielding the CPU.
        let mut msg: MSG = mem::zeroed();
        loop {
            let ret = GetMessageW(&mut msg, hwnd, 0, 0);
            if ret <= 0 {
                // 0 = WM_QUIT, -1 = error — exit either way
                break;
            }

            let keep_going = match msg.message {
                WM_INPUT => process_raw_input(msg.lParam as HRAWINPUT, &sender, device_handle),
                // The tested mouse was unplugged: nothing more will come
                WM_INPUT_DEVICE_CHANGE => {
                    !(msg.wParam as u32 == GIDC_REMOVAL && msg.lParam as usize == device_handle)
                }
                _ => true,
            };
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !keep_going {
                break;
            }
        }
    }
}

/// Process a WM_INPUT message. Returns `false` when the test has dropped
/// its receiver and the thread should end.
unsafe fn process_raw_input(
    handle: HRAWINPUT,
    sender: &Sender<RawMouseData>,
    device_handle: usize,
) -> bool {
    let time = Instant::now();
    let mut size: UINT = 0;

    GetRawInputData(
        handle,
        RID_INPUT,
        ptr::null_mut(),
        &mut size,
        mem::size_of::<RAWINPUTHEADER>() as UINT,
    );

    if size == 0 {
        return true;
    }

    // u64 storage keeps the RAWINPUT cast aligned.
    let mut buffer: Vec<u64> = vec![0; (size as usize).div_ceil(8)];

    if GetRawInputData(
        handle,
        RID_INPUT,
        buffer.as_mut_ptr() as *mut _,
        &mut size,
        mem::size_of::<RAWINPUTHEADER>() as UINT,
    ) != size
    {
        return true;
    }

    let raw = &*(buffer.as_ptr() as *const RAWINPUT);

    if raw.header.dwType == RIM_TYPEMOUSE && raw.header.hDevice as usize == device_handle {
        let mouse = raw.data.mouse();
        // Absolute reports (tablets, remote desktop) carry positions, not counts.
        let relative = mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0;

        let data = RawMouseData {
            dx: if relative { mouse.lLastX } else { 0 },
            dy: if relative { mouse.lLastY } else { 0 },
            button_flags: mouse.usButtonFlags,
            button_data: mouse.usButtonData,
            time,
        };

        // Only send if there's actual data
        if data.dx != 0 || data.dy != 0 || data.button_flags != 0 {
            return sender.send(data).is_ok();
        }
    }
    true
}
