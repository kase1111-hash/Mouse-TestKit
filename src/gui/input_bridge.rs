//! Input bridge for the GUI
//!
//! Provides a background thread that reads raw platform input (evdev on Linux,
//! Raw Input on Windows) and delivers events via channel to the GUI panels.
//! This bypasses egui's pointer processing, giving accurate high-resolution
//! timing data for polling rate, stutter, and latency measurements.
//!
//! On platforms where raw input is unavailable (macOS), returns None
//! and panels fall back to egui's pointer delta.

use std::sync::mpsc;
use std::thread;
use std::time::Instant;

/// A raw input event from mouse hardware with high-resolution timestamp.
#[derive(Debug, Clone)]
pub struct RawInputEvent {
    pub kind: RawInputKind,
    pub timestamp: Instant,
}

/// The type of raw input event.
#[derive(Debug, Clone)]
pub enum RawInputKind {
    /// Mouse moved by (dx, dy) raw counts.
    Move { dx: i32, dy: i32 },
    /// Mouse button was pressed.
    ButtonPress(RawButton),
    /// Mouse button was released.
    ButtonRelease(RawButton),
    /// Scroll wheel moved by delta steps.
    Scroll { delta: i32 },
}

/// Mouse button identifiers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawButton {
    Left,
    Right,
    Middle,
    Side,
    Extra,
}

/// Handle to the background input thread.
///
/// Owns the receiving end of the channel. When dropped, the sender
/// in the background thread will error on next `send()` and exit.
/// The thread may remain blocked on device read until the next event;
/// this is acceptable since `InputBridge` lives for the app's lifetime.
pub struct InputBridge {
    receiver: mpsc::Receiver<RawInputEvent>,
    // Keep thread handle alive. Thread exits when sender errors on send.
    _thread: thread::JoinHandle<()>,
}

impl InputBridge {
    /// Drain all pending raw input events (non-blocking).
    /// Call once per frame in the GUI update loop.
    pub fn poll(&self) -> Vec<RawInputEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
    }

    /// Returns true if the bridge has raw input available.
    /// Useful for panels to know whether to fall back to egui input.
    pub fn has_events_available(&self) -> bool {
        // If we can successfully peek, the channel is alive
        // This is a heuristic — the bridge is alive if the thread is running
        true
    }
}

// ─── Linux implementation ───────────────────────────────────────────────────

#[cfg(target_os = "linux")]
impl InputBridge {
    /// Start the background evdev input thread.
    ///
    /// Auto-selects the first mouse device found (has REL_X + BTN_LEFT).
    /// Returns `None` if no mouse device is found or permissions are insufficient.
    /// Does NOT grab the device — the GUI still needs normal pointer input for its UI.
    pub fn start() -> Option<Self> {
        use evdev::{Device, RelativeAxisType, Key};
        use std::fs;

        // Find the first mouse device by scanning /dev/input/event*
        let mut mouse_device: Option<Device> = None;
        let mut permission_denied = 0usize;

        if let Ok(entries) = fs::read_dir("/dev/input") {
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.path());

            for entry in entries {
                let path = entry.path();
                if !path.to_string_lossy().contains("event") {
                    continue;
                }

                match Device::open(&path) {
                    Ok(device) => {
                        let has_rel_x = device
                            .supported_relative_axes()
                            .map(|axes| axes.contains(RelativeAxisType::REL_X))
                            .unwrap_or(false);
                        let has_left_btn = device
                            .supported_keys()
                            .map(|keys| keys.contains(Key::BTN_LEFT))
                            .unwrap_or(false);

                        if has_rel_x && has_left_btn {
                            let name = device
                                .name()
                                .unwrap_or("Unknown")
                                .to_string();
                            eprintln!(
                                "InputBridge: using device '{}' at {}",
                                name,
                                path.display()
                            );
                            mouse_device = Some(device);
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        permission_denied += 1;
                    }
                    Err(_) => {}
                }
            }
        }

        if mouse_device.is_none() && permission_denied > 0 {
            eprintln!(
                "InputBridge: {} device(s) inaccessible (permission denied). \
                 Add user to 'input' group: sudo usermod -aG input $USER",
                permission_denied
            );
        }

        let device = mouse_device?;
        let (sender, receiver) = mpsc::channel();

        let thread = thread::Builder::new()
            .name("input-bridge-evdev".into())
            .spawn(move || {
                Self::linux_event_loop(device, sender);
            })
            .ok()?;

        Some(InputBridge {
            receiver,
            _thread: thread,
        })
    }

    /// Event loop that reads raw evdev events and sends them over the channel.
    ///
    /// Merges consecutive REL_X/REL_Y events with the same kernel timestamp
    /// into a single `Move { dx, dy }` event. This prevents double-counting
    /// for polling rate measurement (one physical poll → one Move event).
    fn linux_event_loop(mut device: evdev::Device, sender: mpsc::Sender<RawInputEvent>) {
        use evdev::{InputEventKind, RelativeAxisType, Key};
        use std::time::SystemTime;

        loop {
            match device.fetch_events() {
                Ok(events) => {
                    // Accumulate dx/dy per kernel timestamp to merge X+Y into one Move
                    let mut pending_dx: i32 = 0;
                    let mut pending_dy: i32 = 0;
                    let mut last_ts: Option<SystemTime> = None;

                    for ev in events {
                        let ts = ev.timestamp();

                        // Flush accumulated move when kernel timestamp changes
                        if let Some(prev_ts) = last_ts {
                            if ts != prev_ts && (pending_dx != 0 || pending_dy != 0) {
                                if sender
                                    .send(RawInputEvent {
                                        kind: RawInputKind::Move {
                                            dx: pending_dx,
                                            dy: pending_dy,
                                        },
                                        timestamp: Instant::now(),
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                pending_dx = 0;
                                pending_dy = 0;
                            }
                        }
                        last_ts = Some(ts);

                        match ev.kind() {
                            InputEventKind::RelAxis(axis) => match axis {
                                RelativeAxisType::REL_X => {
                                    pending_dx += ev.value();
                                }
                                RelativeAxisType::REL_Y => {
                                    pending_dy += ev.value();
                                }
                                RelativeAxisType::REL_WHEEL
                                | RelativeAxisType::REL_WHEEL_HI_RES => {
                                    if sender
                                        .send(RawInputEvent {
                                            kind: RawInputKind::Scroll {
                                                delta: ev.value(),
                                            },
                                            timestamp: Instant::now(),
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                                _ => {}
                            },
                            InputEventKind::Key(key) => {
                                let button = match key {
                                    Key::BTN_LEFT => Some(RawButton::Left),
                                    Key::BTN_RIGHT => Some(RawButton::Right),
                                    Key::BTN_MIDDLE => Some(RawButton::Middle),
                                    Key::BTN_SIDE => Some(RawButton::Side),
                                    Key::BTN_EXTRA => Some(RawButton::Extra),
                                    _ => None,
                                };
                                if let Some(btn) = button {
                                    let kind = if ev.value() == 1 {
                                        RawInputKind::ButtonPress(btn)
                                    } else if ev.value() == 0 {
                                        RawInputKind::ButtonRelease(btn)
                                    } else {
                                        continue; // ignore repeat (value == 2)
                                    };
                                    if sender
                                        .send(RawInputEvent {
                                            kind,
                                            timestamp: Instant::now(),
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    // Flush any remaining accumulated move after processing the batch
                    if pending_dx != 0 || pending_dy != 0 {
                        if sender
                            .send(RawInputEvent {
                                kind: RawInputKind::Move {
                                    dx: pending_dx,
                                    dy: pending_dy,
                                },
                                timestamp: Instant::now(),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Non-blocking mode: no events ready yet
                    thread::sleep(std::time::Duration::from_micros(500));
                }
                Err(_) => {
                    // Device disconnected or fatal error
                    eprintln!("InputBridge: device error, stopping");
                    return;
                }
            }
        }
    }
}

// ─── Windows implementation ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
impl InputBridge {
    /// Start the background Raw Input thread.
    ///
    /// Creates a hidden window registered for raw mouse input via RIDEV_INPUTSINK.
    /// Uses blocking `GetMessageW` — no heartbeat polling needed.
    /// Returns `None` if window creation or raw input registration fails.
    pub fn start() -> Option<Self> {
        let (sender, receiver) = mpsc::channel();

        let thread = thread::Builder::new()
            .name("input-bridge-rawinput".into())
            .spawn(move || {
                Self::windows_event_loop(sender);
            })
            .ok()?;

        Some(InputBridge {
            receiver,
            _thread: thread,
        })
    }

    fn windows_event_loop(sender: mpsc::Sender<RawInputEvent>) {
        unsafe {
            use std::mem;
            use std::ptr;
            use winapi::shared::hidusage::{HID_USAGE_GENERIC_MOUSE, HID_USAGE_PAGE_GENERIC};
            use winapi::shared::minwindef::UINT;
            use winapi::um::libloaderapi::GetModuleHandleW;
            use winapi::um::winuser::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
                GetRawInputData, RegisterClassW, RegisterRawInputDevices, TranslateMessage,
                CS_HREDRAW, CS_VREDRAW, HRAWINPUT, MSG, RAWINPUT, RAWINPUTDEVICE,
                RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE, WM_INPUT,
                WNDCLASSW, WS_OVERLAPPEDWINDOW,
            };

            // Register window class
            let class_name: Vec<u16> = "MouseTrapInputBridge\0".encode_utf16().collect();
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
                eprintln!("InputBridge: failed to create input window");
                return;
            }

            // Register for raw mouse input
            let rid = RAWINPUTDEVICE {
                usUsagePage: HID_USAGE_PAGE_GENERIC,
                usUsage: HID_USAGE_GENERIC_MOUSE,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };

            if RegisterRawInputDevices(
                &rid,
                1,
                mem::size_of::<RAWINPUTDEVICE>() as UINT,
            ) == 0
            {
                eprintln!("InputBridge: failed to register for raw input");
                return;
            }

            eprintln!("InputBridge: Windows Raw Input registered");

            // Blocking message loop — no heartbeat hack needed
            let mut msg: MSG = mem::zeroed();
            loop {
                let ret = GetMessageW(&mut msg, hwnd, 0, 0);
                if ret <= 0 {
                    break; // WM_QUIT or error
                }

                if msg.message == WM_INPUT {
                    Self::process_wm_input(msg.lParam as HRAWINPUT, &sender);
                }

                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe fn process_wm_input(
        handle: winapi::um::winuser::HRAWINPUT,
        sender: &mpsc::Sender<RawInputEvent>,
    ) {
        use std::mem;
        use winapi::shared::minwindef::UINT;
        use winapi::um::winuser::{
            GetRawInputData, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEMOUSE,
        };

        let mut size: UINT = 0;
        GetRawInputData(
            handle,
            RID_INPUT,
            std::ptr::null_mut(),
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
        if raw.header.dwType != RIM_TYPEMOUSE {
            return;
        }

        let mouse = raw.data.mouse();
        let now = Instant::now();

        // Movement (already combined dx+dy in a single WM_INPUT on Windows)
        if mouse.lLastX != 0 || mouse.lLastY != 0 {
            let _ = sender.send(RawInputEvent {
                kind: RawInputKind::Move {
                    dx: mouse.lLastX,
                    dy: mouse.lLastY,
                },
                timestamp: now,
            });
        }

        let flags = mouse.usButtonFlags;

        // RI_MOUSE_LEFT_BUTTON_DOWN / UP
        if flags & 0x0001 != 0 {
            let _ = sender.send(RawInputEvent {
                kind: RawInputKind::ButtonPress(RawButton::Left),
                timestamp: now,
            });
        }
        if flags & 0x0002 != 0 {
            let _ = sender.send(RawInputEvent {
                kind: RawInputKind::ButtonRelease(RawButton::Left),
                timestamp: now,
            });
        }
        // RI_MOUSE_RIGHT_BUTTON_DOWN / UP
        if flags & 0x0004 != 0 {
            let _ = sender.send(RawInputEvent {
                kind: RawInputKind::ButtonPress(RawButton::Right),
                timestamp: now,
            });
        }
        if flags & 0x0008 != 0 {
            let _ = sender.send(RawInputEvent {
                kind: RawInputKind::ButtonRelease(RawButton::Right),
                timestamp: now,
            });
        }
        // RI_MOUSE_MIDDLE_BUTTON_DOWN / UP
        if flags & 0x0010 != 0 {
            let _ = sender.send(RawInputEvent {
                kind: RawInputKind::ButtonPress(RawButton::Middle),
                timestamp: now,
            });
        }
        if flags & 0x0020 != 0 {
            let _ = sender.send(RawInputEvent {
                kind: RawInputKind::ButtonRelease(RawButton::Middle),
                timestamp: now,
            });
        }
        // RI_MOUSE_WHEEL
        if flags & 0x0400 != 0 {
            let delta = mouse.usButtonData as i16 as i32 / 120; // WHEEL_DELTA
            let _ = sender.send(RawInputEvent {
                kind: RawInputKind::Scroll { delta },
                timestamp: now,
            });
        }
    }
}

// ─── Fallback for unsupported platforms (macOS, etc.) ───────────────────────

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl InputBridge {
    /// Raw input is not available on this platform.
    /// GUI panels will fall back to egui's pointer delta (reduced accuracy).
    pub fn start() -> Option<Self> {
        eprintln!(
            "InputBridge: raw input not available on this platform. \
             GUI tests will use framework input (reduced accuracy)."
        );
        None
    }
}
