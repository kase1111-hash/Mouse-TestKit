//! Input bridge for the GUI
//!
//! Reads raw platform input (evdev on Linux, Raw Input on Windows) on
//! background threads and delivers it to the GUI panels, bypassing egui's
//! frame-rate-limited, OS-accelerated pointer.
//!
//! - **Every** mouse-capable device is read and each event is tagged with the
//!   device it came from. [`DeviceSelector`] decides which one the tests
//!   measure: in Auto mode the device the user moves, clicks or scrolls, kept
//!   for as long as a test runs. The user can also pin a device or choose
//!   pointer (framework) input.
//! - Linux: events carry the kernel's timestamp, so reports read in one batch
//!   keep their real spacing. A device that errors (unplugged) is dropped and
//!   `/dev/input` is rescanned every [`RESCAN_INTERVAL`], so a replugged mouse
//!   comes back.
//! - Windows: reports are told apart by `RAWINPUTHEADER::hDevice` and stamped
//!   when `WM_INPUT` is received (Raw Input carries no hardware timestamp).
//! - [`PointerCrossCheck`] notices when the window-system pointer keeps moving
//!   while the measured device reports nothing (a touchpad, tablet or remote
//!   session is driving the pointer), so the GUI does not present a silent
//!   device as the mouse under test.
//!
//! On platforms without a raw reader (macOS) [`InputBridge::start`] returns
//! `None` and panels fall back to egui's pointer delta.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", target_os = "windows", test))]
use std::sync::mpsc::SyncSender;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::sync::mpsc::TrySendError;

pub use mouse_testkit::analysis::coalesce::EventCoalescer;

/// Maximum number of raw messages buffered between GUI frames.
///
/// When no test is running the GUI only repaints a few times a second; a
/// bounded channel caps memory use instead of growing without limit. At
/// 8000 Hz this still buffers several seconds.
#[cfg(any(target_os = "linux", target_os = "windows", test))]
const CHANNEL_CAPACITY: usize = 65_536;

/// How often `/dev/input` is rescanned for mice plugged in after start-up.
/// A replugged mouse is usable again within this time; nodes already
/// examined are only `stat`ed, not reopened.
pub const RESCAN_INTERVAL: Duration = Duration::from_secs(2);

/// Motion (|dx| + |dy| counts) a device must report within [`CLAIM_WINDOW`]
/// to count as deliberately moved by the user. Even 2 mm of hand movement is
/// over 30 counts at 400 DPI, the lowest common setting, while a mouse at
/// rest or a TrackPoint's drift produces a few counts per second at most.
pub const CLAIM_MOTION_COUNTS: u32 = 30;
/// Window over which [`CLAIM_MOTION_COUNTS`] must add up.
pub const CLAIM_WINDOW: Duration = Duration::from_millis(300);

/// How long the window-system pointer must have been moving, without a single
/// motion report from the measured device, before that device is considered
/// not to be the one the user is moving. A moving mouse reports at least 125
/// times a second, so a whole second without a report is not timing jitter.
pub const POINTER_MISMATCH_AFTER: Duration = Duration::from_secs(1);
/// Pointer-motion frames closer together than this count as one continuous
/// movement (frames are 4-33 ms apart while the pointer moves).
const POINTER_FRAME_GAP: Duration = Duration::from_millis(100);

/// A raw input event from mouse hardware with high-resolution timestamp.
#[derive(Debug, Clone)]
pub struct RawInputEvent {
    pub kind: RawInputKind,
    /// When the device reported the event: the kernel timestamp on Linux,
    /// the moment `WM_INPUT` arrived on Windows. Never the time the GUI read
    /// it.
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

/// Identifies one input device for as long as it stays connected.
pub type DeviceId = u64;

/// A mouse-capable device the bridge is reading.
#[derive(Debug, Clone, PartialEq)]
pub struct InputDevice {
    pub id: DeviceId,
    /// Name reported by the device (Linux) or derived from its hardware ID
    /// (Windows).
    pub name: String,
    /// Where it is attached, e.g. `event5` on Linux or the HID interface on
    /// Windows. Tells apart the several nodes one receiver often exposes.
    pub location: String,
}

impl InputDevice {
    /// Name and location, for device pickers.
    pub fn label(&self) -> String {
        if self.location.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.location)
        }
    }
}

/// Which input the tests measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceChoice {
    /// The device the user deliberately moves, clicks or scrolls.
    #[default]
    Auto,
    /// Only this device.
    Device(DeviceId),
    /// No raw input: panels use the window system's pointer.
    Pointer,
}

/// Message from the reader thread(s) to the GUI side.
#[cfg_attr(
    not(any(target_os = "linux", target_os = "windows", test)),
    allow(dead_code)
)]
#[derive(Debug)]
pub enum BridgeMessage {
    /// A mouse-capable device was found (also sent for devices present at
    /// start-up).
    Added(InputDevice),
    /// The device disconnected or failed; no more events will come from it.
    Removed(DeviceId),
    /// An event from a device.
    Event(DeviceId, RawInputEvent),
}

/// Chooses which device's events reach the tests.
///
/// In [`DeviceChoice::Auto`] a device becomes the measured one when the user
/// deliberately uses it: a button press, a wheel notch, or
/// [`CLAIM_MOTION_COUNTS`] of motion within [`CLAIM_WINDOW`]. A node that
/// never moves (a TrackPoint, a keyboard's mouse interface, a receiver's
/// spare interface) is never chosen. While frozen (a test is running) the
/// measured device is kept even if another device is bumped.
#[derive(Debug, Default)]
pub struct DeviceSelector {
    choice: DeviceChoice,
    active: Option<DeviceId>,
    frozen: bool,
    /// Per candidate device: start of its current motion burst and the counts
    /// moved since.
    bursts: HashMap<DeviceId, (Instant, u32)>,
}

impl DeviceSelector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn choice(&self) -> DeviceChoice {
        self.choice
    }

    /// Change the choice. Pinning a device also makes it the one Auto keeps
    /// measuring if the user switches back to Auto.
    pub fn set_choice(&mut self, choice: DeviceChoice) {
        if let DeviceChoice::Device(id) = choice {
            self.active = Some(id);
        }
        self.choice = choice;
        self.bursts.clear();
    }

    /// Freeze Auto selection while a test is running, so a bumped second
    /// mouse or a drifting TrackPoint cannot take over mid-measurement.
    pub fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
    }

    /// The device whose events are delivered, if any.
    pub fn active(&self) -> Option<DeviceId> {
        match self.choice {
            DeviceChoice::Auto => self.active,
            DeviceChoice::Device(id) => Some(id),
            DeviceChoice::Pointer => None,
        }
    }

    /// Whether `event` from `device` should reach the tests (updating the
    /// Auto selection on deliberate use).
    pub fn accept(&mut self, device: DeviceId, event: &RawInputEvent) -> bool {
        match self.choice {
            DeviceChoice::Pointer => false,
            DeviceChoice::Device(id) => id == device,
            DeviceChoice::Auto => {
                if self.active == Some(device) {
                    return true;
                }
                if self.active.is_some() && self.frozen {
                    return false;
                }
                if self.deliberate_use(device, event) {
                    self.active = Some(device);
                    self.bursts.clear();
                    true
                } else {
                    false
                }
            }
        }
    }

    fn deliberate_use(&mut self, device: DeviceId, event: &RawInputEvent) -> bool {
        match event.kind {
            RawInputKind::ButtonPress(_) | RawInputKind::Scroll { .. } => true,
            RawInputKind::ButtonRelease(_) => false,
            RawInputKind::Move { dx, dy } => {
                let counts = dx.unsigned_abs().saturating_add(dy.unsigned_abs());
                let burst = self.bursts.entry(device).or_insert((event.timestamp, 0));
                if event.timestamp.saturating_duration_since(burst.0) > CLAIM_WINDOW {
                    *burst = (event.timestamp, 0);
                }
                burst.1 = burst.1.saturating_add(counts);
                burst.1 >= CLAIM_MOTION_COUNTS
            }
        }
    }

    /// The device is gone: stop measuring it (a pinned choice reverts to
    /// Auto, which picks up the replugged mouse on its next use).
    pub fn device_removed(&mut self, device: DeviceId) {
        if self.active == Some(device) {
            self.active = None;
        }
        if self.choice == DeviceChoice::Device(device) {
            self.choice = DeviceChoice::Auto;
        }
        self.bursts.remove(&device);
    }

    /// In Auto mode with no test running, stop measuring the current device
    /// (the user is evidently using another one).
    pub fn release_unless_frozen(&mut self) {
        if self.choice == DeviceChoice::Auto && !self.frozen {
            self.active = None;
            self.bursts.clear();
        }
    }
}

/// Detects that the window-system pointer is being moved by something other
/// than the measured device.
///
/// Fed once per GUI frame with whether egui's pointer moved and whether the
/// measured device delivered motion. Reports a mismatch after
/// [`POINTER_MISMATCH_AFTER`] of pointer movement without any raw motion,
/// and keeps reporting it until the measured device moves again.
#[derive(Debug, Default)]
pub struct PointerCrossCheck {
    last_pointer: Option<Instant>,
    unmatched: Duration,
    mismatch: bool,
}

impl PointerCrossCheck {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update with this frame's observations; returns [`Self::mismatch`].
    pub fn update(&mut self, now: Instant, pointer_moved: bool, raw_moved: bool) -> bool {
        if raw_moved {
            self.unmatched = Duration::ZERO;
            self.mismatch = false;
        } else if pointer_moved {
            if let Some(last) = self.last_pointer {
                let gap = now.saturating_duration_since(last);
                if gap <= POINTER_FRAME_GAP {
                    self.unmatched += gap;
                }
            }
            if self.unmatched >= POINTER_MISMATCH_AFTER {
                self.mismatch = true;
            }
        }
        if pointer_moved {
            self.last_pointer = Some(now);
        }
        self.mismatch
    }

    pub fn mismatch(&self) -> bool {
        self.mismatch
    }
}

/// Handle to the background input thread(s) and the GUI-side device state.
///
/// Owns the receiving end of the channel. When dropped, reader threads exit
/// on their next send and the Linux rescan thread on its next wake-up.
pub struct InputBridge {
    receiver: mpsc::Receiver<BridgeMessage>,
    /// Connected devices, in `/dev/input/eventN` numeric order on Linux.
    devices: Vec<InputDevice>,
    selector: DeviceSelector,
    cross_check: PointerCrossCheck,
    stop: Arc<AtomicBool>,
    _thread: Option<thread::JoinHandle<()>>,
}

impl Drop for InputBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl InputBridge {
    #[cfg(any(target_os = "linux", target_os = "windows", test))]
    fn from_parts(
        receiver: mpsc::Receiver<BridgeMessage>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    ) -> Self {
        Self {
            receiver,
            devices: Vec::new(),
            selector: DeviceSelector::new(),
            cross_check: PointerCrossCheck::new(),
            stop,
            _thread: thread,
        }
    }

    /// A bridge with no reader thread, fed through the returned sender.
    #[cfg(test)]
    pub(crate) fn for_test() -> (Self, SyncSender<BridgeMessage>) {
        let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let bridge = Self::from_parts(receiver, Arc::new(AtomicBool::new(false)), None);
        (bridge, sender)
    }

    /// Drain pending messages (non-blocking) and return the events of the
    /// measured device. Call once per frame in the GUI update loop.
    pub fn poll(&mut self) -> Vec<RawInputEvent> {
        let mut events = Vec::new();
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                BridgeMessage::Added(device) => {
                    self.devices.retain(|d| d.id != device.id);
                    self.devices.push(device);
                    self.devices.sort_by_key(device_sort_key);
                }
                BridgeMessage::Removed(id) => {
                    self.devices.retain(|d| d.id != id);
                    self.selector.device_removed(id);
                }
                BridgeMessage::Event(id, event) => {
                    // Events still queued from a device already removed are
                    // dropped so they cannot re-select it.
                    if self.devices.iter().any(|d| d.id == id) && self.selector.accept(id, &event) {
                        events.push(event);
                    }
                }
            }
        }
        events
    }

    /// Freeze Auto device selection (pass `true` while any test is running).
    pub fn set_selection_frozen(&mut self, frozen: bool) {
        self.selector.set_frozen(frozen);
    }

    /// Compare this frame's window-system pointer motion with the motion the
    /// measured device delivered (`delivered`, the result of [`Self::poll`]).
    /// In Auto mode with no test running, a device that is evidently not the
    /// one moving the pointer stops being measured.
    pub fn cross_check_pointer(
        &mut self,
        now: Instant,
        pointer_moved: bool,
        delivered: &[RawInputEvent],
    ) {
        if self.selector.choice() == DeviceChoice::Pointer {
            self.cross_check = PointerCrossCheck::new();
            return;
        }
        let raw_moved = delivered
            .iter()
            .any(|e| matches!(e.kind, RawInputKind::Move { .. }));
        if self.cross_check.update(now, pointer_moved, raw_moved) && pointer_moved {
            self.selector.release_unless_frozen();
        }
    }

    /// Whether the pointer has been moving while the measured device (or, if
    /// none is measured, every readable device) reported no motion.
    pub fn pointer_mismatch(&self) -> bool {
        self.cross_check.mismatch()
    }

    /// Connected mouse-capable devices.
    pub fn devices(&self) -> &[InputDevice] {
        &self.devices
    }

    pub fn choice(&self) -> DeviceChoice {
        self.selector.choice()
    }

    pub fn set_choice(&mut self, choice: DeviceChoice) {
        self.selector.set_choice(choice);
        self.cross_check = PointerCrossCheck::new();
    }

    /// The device being measured, if raw input is currently in use.
    pub fn active_device(&self) -> Option<&InputDevice> {
        let id = self.selector.active()?;
        self.devices.iter().find(|d| d.id == id)
    }

    /// Whether tests should use raw input: a device is being measured.
    /// `false` until the user moves or clicks a readable mouse, after it is
    /// unplugged, or when pointer input was chosen.
    pub fn is_delivering(&self) -> bool {
        self.active_device().is_some()
    }
}

/// Number of an evdev node (`event12` or `/dev/input/event12` -> 12).
fn event_node_number(location: &str) -> Option<u32> {
    let name = location.rsplit('/').next()?;
    let digits = name.strip_prefix("event")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Devices are listed in numeric node order (`event2` before `event10`),
/// then by name.
fn device_sort_key(device: &InputDevice) -> (u32, String, DeviceId) {
    (
        event_node_number(&device.location).unwrap_or(u32::MAX),
        device.name.clone(),
        device.id,
    )
}

/// Queue an event for the GUI thread.
///
/// Returns `false` when the GUI side has gone away and the input thread
/// should exit. When the channel is full (the GUI has not drained it in a
/// long time, e.g. while minimised) the event is dropped rather than blocking
/// the reader.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn send_event(sender: &SyncSender<BridgeMessage>, device: DeviceId, event: RawInputEvent) -> bool {
    match sender.try_send(BridgeMessage::Event(device, event)) {
        Ok(()) | Err(TrySendError::Full(_)) => true,
        Err(TrySendError::Disconnected(_)) => false,
    }
}

// ─── Linux implementation ───────────────────────────────────────────────────

/// Kernel event timestamps older than this when read are not trusted: the
/// reader drains each device continuously, so such an age means the wall
/// clock was stepped or the system slept, and the read time is the better
/// estimate.
#[cfg(target_os = "linux")]
const MAX_REPORT_AGE: Duration = Duration::from_secs(1);

/// Map an evdev timestamp (CLOCK_REALTIME) onto the monotonic clock using a
/// `(SystemTime, Instant)` pair read at the same moment. Events mapped with
/// the same pair keep their exact spacing.
#[cfg(target_os = "linux")]
fn kernel_time_to_instant(
    timestamp: std::time::SystemTime,
    ref_system: std::time::SystemTime,
    ref_instant: Instant,
) -> Instant {
    match ref_system.duration_since(timestamp) {
        Ok(age) if age <= MAX_REPORT_AGE => ref_instant.checked_sub(age).unwrap_or(ref_instant),
        // Stamped after the reference (clock stepped back) or implausibly old
        _ => ref_instant,
    }
}

#[cfg(target_os = "linux")]
fn raw_button(key: evdev::KeyCode) -> Option<RawButton> {
    use evdev::KeyCode;
    match key {
        KeyCode::BTN_LEFT => Some(RawButton::Left),
        KeyCode::BTN_RIGHT => Some(RawButton::Right),
        KeyCode::BTN_MIDDLE => Some(RawButton::Middle),
        KeyCode::BTN_SIDE => Some(RawButton::Side),
        KeyCode::BTN_EXTRA => Some(RawButton::Extra),
        _ => None,
    }
}

/// Turn one batch of evdev events into bridge events.
///
/// The REL_X/REL_Y events of a SYN_REPORT-terminated report become one
/// `Move` (one hardware report, one event), and every event is stamped via
/// `to_instant(kernel timestamp)` rather than the time it was read.
#[cfg(target_os = "linux")]
fn translate_reports(
    events: impl IntoIterator<Item = (std::time::SystemTime, evdev::EventSummary)>,
    to_instant: impl Fn(std::time::SystemTime) -> Instant,
) -> Vec<RawInputEvent> {
    use evdev::{EventSummary, RelativeAxisCode, SynchronizationCode};

    let mut out = Vec::new();
    // Motion of the current report: (time of its first axis event, dx, dy)
    let mut motion: Option<(Instant, i32, i32)> = None;
    let flush = |out: &mut Vec<RawInputEvent>, motion: Option<(Instant, i32, i32)>| {
        if let Some((timestamp, dx, dy)) = motion {
            if dx != 0 || dy != 0 {
                out.push(RawInputEvent {
                    kind: RawInputKind::Move { dx, dy },
                    timestamp,
                });
            }
        }
    };

    for (time, summary) in events {
        match summary {
            EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                flush(&mut out, motion.take());
            }
            EventSummary::RelativeAxis(_, RelativeAxisCode::REL_X, value) => {
                motion.get_or_insert_with(|| (to_instant(time), 0, 0)).1 += value;
            }
            EventSummary::RelativeAxis(_, RelativeAxisCode::REL_Y, value) => {
                motion.get_or_insert_with(|| (to_instant(time), 0, 0)).2 += value;
            }
            // One notch is reported both as REL_WHEEL (+-1) and, on newer
            // kernels, as REL_WHEEL_HI_RES (+-120). Only the notch-granular
            // event is used so a notch counts once.
            EventSummary::RelativeAxis(_, RelativeAxisCode::REL_WHEEL, delta) => {
                out.push(RawInputEvent {
                    kind: RawInputKind::Scroll { delta },
                    timestamp: to_instant(time),
                });
            }
            EventSummary::Key(_, key, value) => {
                let Some(button) = raw_button(key) else {
                    continue;
                };
                let kind = match value {
                    1 => RawInputKind::ButtonPress(button),
                    0 => RawInputKind::ButtonRelease(button),
                    _ => continue, // autorepeat (2)
                };
                out.push(RawInputEvent {
                    kind,
                    timestamp: to_instant(time),
                });
            }
            _ => {}
        }
    }
    flush(&mut out, motion);
    out
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use evdev::{Device, KeyCode, RelativeAxisCode};
    use std::collections::HashSet;
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::SystemTime;

    /// A device node as it exists now. A node recreated after a replug has a
    /// new inode, so it is examined again even if it reuses the same name.
    type NodeKey = (PathBuf, u64);

    /// Outcome of one scan of `/dev/input`.
    pub(super) struct ScanResult {
        /// `/dev/input` could be listed.
        pub readable: bool,
        /// Mice with a running reader after this scan.
        pub mice: usize,
        /// Nodes that could not be opened for lack of permission.
        pub permission_denied: usize,
    }

    /// Finds mice under `/dev/input` and starts one reader thread per mouse.
    pub(super) struct Scanner {
        sender: SyncSender<BridgeMessage>,
        next_id: DeviceId,
        /// Nodes with a live reader (the reader removes its entry on exit).
        open: Arc<Mutex<HashSet<NodeKey>>>,
        /// Nodes that were opened and are not mice.
        not_mice: HashSet<NodeKey>,
    }

    fn is_mouse(device: &Device) -> bool {
        let has_rel_x = device
            .supported_relative_axes()
            .is_some_and(|axes| axes.contains(RelativeAxisCode::REL_X));
        let has_left_btn = device
            .supported_keys()
            .is_some_and(|keys| keys.contains(KeyCode::BTN_LEFT));
        has_rel_x && has_left_btn
    }

    fn lock(open: &Mutex<HashSet<NodeKey>>) -> std::sync::MutexGuard<'_, HashSet<NodeKey>> {
        open.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    impl Scanner {
        pub fn new(sender: SyncSender<BridgeMessage>) -> Self {
            Self {
                sender,
                next_id: 1,
                open: Arc::new(Mutex::new(HashSet::new())),
                not_mice: HashSet::new(),
            }
        }

        /// Examine every event node not seen before, in numeric order, and
        /// start a reader for each new mouse. Does NOT grab devices: the GUI
        /// still needs normal pointer input.
        pub fn scan(&mut self) -> ScanResult {
            let mut result = ScanResult {
                readable: false,
                mice: 0,
                permission_denied: 0,
            };
            let entries = match fs::read_dir("/dev/input") {
                Ok(entries) => entries,
                Err(_) => return result,
            };
            result.readable = true;

            let mut nodes: Vec<(u32, PathBuf)> = entries
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    let number = event_node_number(&path.to_string_lossy())?;
                    Some((number, path))
                })
                .collect();
            nodes.sort();

            let mut present = HashSet::new();
            for (_, path) in nodes {
                let Ok(meta) = fs::metadata(&path) else {
                    continue;
                };
                let key: NodeKey = (path.clone(), meta.ino());
                present.insert(key.clone());
                if self.not_mice.contains(&key) {
                    continue;
                }
                if lock(&self.open).contains(&key) {
                    result.mice += 1;
                    continue;
                }
                match Device::open(&path) {
                    Ok(device) if is_mouse(&device) => {
                        if self.spawn_reader(device, key) {
                            result.mice += 1;
                        }
                    }
                    Ok(_) => {
                        self.not_mice.insert(key);
                    }
                    // A just-plugged node may not have its group permissions
                    // yet; it is retried on the next scan.
                    Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                        result.permission_denied += 1;
                    }
                    Err(_) => {}
                }
            }
            self.not_mice.retain(|key| present.contains(key));
            result
        }

        fn spawn_reader(&mut self, device: Device, key: NodeKey) -> bool {
            let id = self.next_id;
            self.next_id += 1;
            let node = key
                .0
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let info = InputDevice {
                id,
                name: device.name().unwrap_or("Unknown device").to_string(),
                location: node.clone(),
            };
            eprintln!(
                "InputBridge: reading '{}' at {}",
                info.name,
                key.0.display()
            );
            let name = info.name.clone();
            if self.sender.send(BridgeMessage::Added(info)).is_err() {
                return false;
            }
            lock(&self.open).insert(key.clone());

            let sender = self.sender.clone();
            let open = Arc::clone(&self.open);
            let reader_key = key.clone();
            let spawned = thread::Builder::new()
                .name(format!("input-bridge-{node}"))
                .spawn(move || {
                    read_device(id, device, &sender);
                    eprintln!("InputBridge: '{name}' stopped reporting (unplugged?)");
                    lock(&open).remove(&reader_key);
                    let _ = sender.send(BridgeMessage::Removed(id));
                });
            if spawned.is_err() {
                lock(&self.open).remove(&key);
                let _ = self.sender.send(BridgeMessage::Removed(id));
                return false;
            }
            true
        }
    }

    /// Read one device until it fails (unplugged) or the GUI goes away.
    fn read_device(id: DeviceId, mut device: Device, sender: &SyncSender<BridgeMessage>) {
        loop {
            let batch: Vec<_> = match device.fetch_events() {
                Ok(events) => events
                    .map(|ev| (ev.timestamp(), ev.destructure()))
                    .collect(),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    // The device is opened in blocking mode, so this is not
                    // expected; handle it anyway rather than spinning.
                    thread::sleep(Duration::from_micros(500));
                    continue;
                }
                // ENODEV after unplug, or another fatal error
                Err(_) => return,
            };
            // One reference pair per batch, taken after the read, so the
            // reports of the batch keep their kernel spacing.
            let ref_system = SystemTime::now();
            let ref_instant = Instant::now();
            let events = translate_reports(batch, |time| {
                kernel_time_to_instant(time, ref_system, ref_instant)
            });
            for event in events {
                if !send_event(sender, id, event) {
                    return;
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl InputBridge {
    /// Start reading every mouse-capable evdev device (REL_X + BTN_LEFT).
    ///
    /// Returns `None` if `/dev/input` cannot be read, or if no mouse could be
    /// opened because of permissions. With no mouse connected yet the bridge
    /// still starts, and picks one up when it is plugged in.
    pub fn start() -> Option<Self> {
        let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let mut scanner = linux::Scanner::new(sender);
        let first = scanner.scan();
        if !first.readable {
            eprintln!("InputBridge: cannot read /dev/input");
            return None;
        }
        if first.mice == 0 && first.permission_denied > 0 {
            eprintln!(
                "InputBridge: {} device(s) inaccessible (permission denied). \
                 Add user to 'input' group: sudo usermod -aG input $USER",
                first.permission_denied
            );
            return None;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("input-bridge-scan".into())
            .spawn(move || loop {
                thread::sleep(RESCAN_INTERVAL);
                if thread_stop.load(Ordering::Relaxed) {
                    return;
                }
                scanner.scan();
            })
            .ok();
        Some(Self::from_parts(receiver, stop, thread))
    }
}

// ─── Windows implementation ─────────────────────────────────────────────────

/// One wheel notch in `RAWMOUSE::usButtonData` (WHEEL_DELTA).
#[cfg(any(target_os = "windows", test))]
const WHEEL_DELTA: i32 = 120;

/// Adds up Windows wheel deltas into whole notches.
///
/// `usButtonData` is a signed multiple of WHEEL_DELTA (120) per notch, but
/// high-resolution wheels report fractions of it (e.g. 30 per event); dividing
/// each event by 120 would drop their movement entirely.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Default)]
struct WheelAccumulator {
    remainder: i32,
}

#[cfg(any(target_os = "windows", test))]
impl WheelAccumulator {
    /// Add one report's delta; returns the whole notches completed (signed).
    fn add(&mut self, delta: i32) -> i32 {
        if delta == 0 {
            return 0;
        }
        // Turning the other way discards the partial notch, as Windows does.
        if delta.signum() != self.remainder.signum() {
            self.remainder = 0;
        }
        self.remainder += delta;
        let notches = self.remainder / WHEEL_DELTA;
        self.remainder -= notches * WHEEL_DELTA;
        notches
    }
}

/// Display name and location from a Raw Input device path such as
/// `\\?\HID#VID_046D&PID_C08B&MI_00#7&2a1b&0&0000#{...}`.
#[cfg(any(target_os = "windows", test))]
fn windows_device_label(path: &str, id: DeviceId) -> (String, String) {
    let trimmed = path.trim_start_matches(r"\\?\");
    let mut segments = trimmed.split('#');
    let bus = segments.next().unwrap_or("");
    let hardware = segments.next().unwrap_or("");
    let fallback_location = format!("handle {id:#x}");
    if hardware.is_empty() {
        return ("Mouse".to_string(), fallback_location);
    }
    if bus.eq_ignore_ascii_case("HID") {
        let (ids, rest): (Vec<&str>, Vec<&str>) = hardware
            .split('&')
            .partition(|part| part.starts_with("VID_") || part.starts_with("PID_"));
        let name = if ids.is_empty() {
            "HID mouse".to_string()
        } else {
            format!("HID mouse {}", ids.join(" "))
        };
        let location = if rest.is_empty() {
            fallback_location
        } else {
            rest.join(" ")
        };
        (name, location)
    } else {
        (format!("Mouse ({bus} {hardware})"), fallback_location)
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::collections::HashSet;
    use std::mem;
    use std::ptr;
    use winapi::shared::hidusage::{HID_USAGE_GENERIC_MOUSE, HID_USAGE_PAGE_GENERIC};
    use winapi::shared::minwindef::UINT;
    use winapi::um::libloaderapi::GetModuleHandleW;
    use winapi::um::winuser::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetRawInputData,
        GetRawInputDeviceInfoW, GetRawInputDeviceList, RegisterClassW, RegisterRawInputDevices,
        TranslateMessage, CS_HREDRAW, CS_VREDRAW, GIDC_ARRIVAL, GIDC_REMOVAL, HRAWINPUT,
        MOUSE_MOVE_ABSOLUTE, MSG, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER,
        RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDI_DEVICEINFO, RIDI_DEVICENAME, RID_DEVICE_INFO,
        RID_INPUT, RIM_TYPEMOUSE, RI_MOUSE_WHEEL, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };

    /// Button transition flags in `RAWMOUSE::usButtonFlags`, in event order.
    const BUTTON_FLAGS: [(u16, RawButton, bool); 10] = [
        (0x0001, RawButton::Left, true),
        (0x0002, RawButton::Left, false),
        (0x0004, RawButton::Right, true),
        (0x0008, RawButton::Right, false),
        (0x0010, RawButton::Middle, true),
        (0x0020, RawButton::Middle, false),
        (0x0040, RawButton::Side, true),
        (0x0080, RawButton::Side, false),
        (0x0100, RawButton::Extra, true),
        (0x0200, RawButton::Extra, false),
    ];

    /// Devices announced to the GUI and their wheel remainders.
    #[derive(Default)]
    struct State {
        known: HashSet<DeviceId>,
        wheels: HashMap<DeviceId, WheelAccumulator>,
    }

    /// The Raw Input device path (`\\?\HID#...`), if available.
    unsafe fn device_path(id: DeviceId) -> Option<String> {
        let handle = id as usize as *mut _;
        let mut size: UINT = 0;
        GetRawInputDeviceInfoW(handle, RIDI_DEVICENAME, ptr::null_mut(), &mut size);
        if size == 0 {
            return None;
        }
        let mut buffer: Vec<u16> = vec![0; size as usize];
        let written = GetRawInputDeviceInfoW(
            handle,
            RIDI_DEVICENAME,
            buffer.as_mut_ptr() as *mut _,
            &mut size,
        );
        if written == 0 || written == UINT::MAX {
            return None;
        }
        buffer.truncate(written as usize);
        Some(
            String::from_utf16_lossy(&buffer)
                .trim_end_matches('\0')
                .to_string(),
        )
    }

    /// Whether the handle is a mouse-type Raw Input device.
    unsafe fn is_mouse(id: DeviceId) -> bool {
        let mut info: RID_DEVICE_INFO = mem::zeroed();
        info.cbSize = mem::size_of::<RID_DEVICE_INFO>() as UINT;
        let mut size = info.cbSize;
        let written = GetRawInputDeviceInfoW(
            id as usize as *mut _,
            RIDI_DEVICEINFO,
            &mut info as *mut RID_DEVICE_INFO as *mut _,
            &mut size,
        );
        written != 0 && written != UINT::MAX && info.dwType == RIM_TYPEMOUSE
    }

    /// Mouse devices connected now.
    unsafe fn enumerate_mice() -> Vec<DeviceId> {
        let entry_size = mem::size_of::<RAWINPUTDEVICELIST>() as UINT;
        let mut count: UINT = 0;
        if GetRawInputDeviceList(ptr::null_mut(), &mut count, entry_size) != 0 || count == 0 {
            return Vec::new();
        }
        let mut list: Vec<RAWINPUTDEVICELIST> = vec![mem::zeroed(); count as usize];
        let found = GetRawInputDeviceList(list.as_mut_ptr(), &mut count, entry_size);
        if found == UINT::MAX {
            return Vec::new();
        }
        list.iter()
            .take(found as usize)
            .filter(|d| d.dwType == RIM_TYPEMOUSE)
            .map(|d| d.hDevice as usize as DeviceId)
            .filter(|&id| id != 0)
            .collect()
    }

    /// Announce a device to the GUI. Returns `false` if the GUI is gone.
    unsafe fn add_device(
        id: DeviceId,
        sender: &SyncSender<BridgeMessage>,
        state: &mut State,
    ) -> bool {
        let path = device_path(id).unwrap_or_default();
        let (name, location) = windows_device_label(&path, id);
        state.known.insert(id);
        sender
            .send(BridgeMessage::Added(InputDevice { id, name, location }))
            .is_ok()
    }

    /// Handle `WM_INPUT_DEVICE_CHANGE`. Returns `false` if the GUI is gone.
    unsafe fn device_change(
        change: u32,
        id: DeviceId,
        sender: &SyncSender<BridgeMessage>,
        state: &mut State,
    ) -> bool {
        if change == GIDC_ARRIVAL && id != 0 && !state.known.contains(&id) && is_mouse(id) {
            return add_device(id, sender, state);
        }
        if change == GIDC_REMOVAL && state.known.remove(&id) {
            state.wheels.remove(&id);
            return sender.send(BridgeMessage::Removed(id)).is_ok();
        }
        true
    }

    /// Handle one `WM_INPUT`. Returns `false` if the GUI is gone.
    unsafe fn process_wm_input(
        handle: HRAWINPUT,
        sender: &SyncSender<BridgeMessage>,
        state: &mut State,
    ) -> bool {
        // Raw Input has no hardware timestamp: stamp on receipt, in this
        // thread, not when the GUI drains the channel.
        let timestamp = Instant::now();
        let header_size = mem::size_of::<RAWINPUTHEADER>() as UINT;
        let mut size: UINT = 0;
        GetRawInputData(handle, RID_INPUT, ptr::null_mut(), &mut size, header_size);
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
            header_size,
        ) != size
        {
            return true;
        }

        let raw = &*(buffer.as_ptr() as *const RAWINPUT);
        if raw.header.dwType != RIM_TYPEMOUSE {
            return true;
        }
        let id = raw.header.hDevice as usize as DeviceId;
        // No handle: input synthesised by software (SendInput, remote
        // desktop), not a device that can be tested.
        if id == 0 {
            return true;
        }
        let mouse = raw.data.mouse();
        // Absolute reports (tablets, remote desktop, VM pointer integration)
        // carry screen positions, not sensor counts.
        if mouse.usFlags & MOUSE_MOVE_ABSOLUTE != 0 {
            return true;
        }
        if !state.known.contains(&id) && !add_device(id, sender, state) {
            return false;
        }

        let mut kinds = Vec::new();
        // Movement (dx and dy of one report arrive together on Windows)
        if mouse.lLastX != 0 || mouse.lLastY != 0 {
            kinds.push(RawInputKind::Move {
                dx: mouse.lLastX,
                dy: mouse.lLastY,
            });
        }
        let flags = mouse.usButtonFlags;
        for (flag, button, pressed) in BUTTON_FLAGS {
            if flags & flag != 0 {
                kinds.push(if pressed {
                    RawInputKind::ButtonPress(button)
                } else {
                    RawInputKind::ButtonRelease(button)
                });
            }
        }
        if flags & RI_MOUSE_WHEEL != 0 {
            let delta = mouse.usButtonData as i16 as i32;
            let notches = state.wheels.entry(id).or_default().add(delta);
            if notches != 0 {
                kinds.push(RawInputKind::Scroll { delta: notches });
            }
        }
        kinds
            .into_iter()
            .all(|kind| send_event(sender, id, RawInputEvent { kind, timestamp }))
    }

    /// Create the hidden window, register for raw mouse input and pump
    /// messages. Reports registration success through `ready` first.
    pub(super) fn event_loop(sender: SyncSender<BridgeMessage>, ready: mpsc::Sender<bool>) {
        unsafe {
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
                let _ = ready.send(false);
                return;
            }

            // INPUTSINK: receive input while another window has focus.
            // DEVNOTIFY: WM_INPUT_DEVICE_CHANGE on plug and unplug.
            let rid = RAWINPUTDEVICE {
                usUsagePage: HID_USAGE_PAGE_GENERIC,
                usUsage: HID_USAGE_GENERIC_MOUSE,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: hwnd,
            };
            if RegisterRawInputDevices(&rid, 1, mem::size_of::<RAWINPUTDEVICE>() as UINT) == 0 {
                eprintln!("InputBridge: failed to register for raw input");
                let _ = ready.send(false);
                return;
            }

            let mut state = State::default();
            for id in enumerate_mice() {
                if !add_device(id, &sender, &mut state) {
                    let _ = ready.send(false);
                    return;
                }
            }
            eprintln!("InputBridge: Windows Raw Input registered");
            let _ = ready.send(true);

            // Blocking message loop — no heartbeat polling needed
            let mut msg: MSG = mem::zeroed();
            loop {
                if GetMessageW(&mut msg, hwnd, 0, 0) <= 0 {
                    break; // WM_QUIT or error
                }
                let gui_alive = match msg.message {
                    WM_INPUT => process_wm_input(msg.lParam as HRAWINPUT, &sender, &mut state),
                    WM_INPUT_DEVICE_CHANGE => device_change(
                        msg.wParam as u32,
                        msg.lParam as usize as DeviceId,
                        &sender,
                        &mut state,
                    ),
                    _ => true,
                };
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if !gui_alive {
                    return;
                }
            }
            // Nothing will be read any more: report every device gone so the
            // GUI falls back to framework input instead of a silent device.
            for id in state.known.drain() {
                let _ = sender.send(BridgeMessage::Removed(id));
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl InputBridge {
    /// Start the background Raw Input thread.
    ///
    /// Creates a hidden window registered for raw mouse input from every
    /// mouse. Returns `None` if window creation or raw input registration
    /// fails.
    pub fn start() -> Option<Self> {
        let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let (ready_sender, ready) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("input-bridge-rawinput".into())
            .spawn(move || windows::event_loop(sender, ready_sender))
            .ok()?;
        if ready.recv() != Ok(true) {
            return None;
        }
        Some(Self::from_parts(
            receiver,
            Arc::new(AtomicBool::new(false)),
            Some(thread),
        ))
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

#[cfg(test)]
mod tests {
    use super::*;

    const MOUSE: DeviceId = 1;
    const TRACKPOINT: DeviceId = 2;

    fn ev(kind: RawInputKind, at: Instant) -> RawInputEvent {
        RawInputEvent {
            kind,
            timestamp: at,
        }
    }

    fn mv(dx: i32, dy: i32, at: Instant) -> RawInputEvent {
        ev(RawInputKind::Move { dx, dy }, at)
    }

    fn device(id: DeviceId, name: &str, location: &str) -> InputDevice {
        InputDevice {
            id,
            name: name.to_string(),
            location: location.to_string(),
        }
    }

    #[test]
    fn devices_sort_in_numeric_node_order() {
        assert_eq!(event_node_number("/dev/input/event10"), Some(10));
        assert_eq!(event_node_number("event2"), Some(2));
        assert_eq!(event_node_number("mouse0"), None);
        assert_eq!(event_node_number("event"), None);
        assert_eq!(event_node_number("event1x"), None);

        let mut devices = [
            device(1, "Receiver", "event10"),
            device(2, "TrackPoint", "event2"),
            device(3, "HID mouse", ""),
            device(4, "Mouse", "event9"),
        ];
        devices.sort_by_key(device_sort_key);
        let order: Vec<&str> = devices.iter().map(|d| d.location.as_str()).collect();
        // String order would put event10 first
        assert_eq!(order, ["event2", "event9", "event10", ""]);
    }

    #[test]
    fn auto_never_selects_a_silent_or_drifting_device() {
        let t = Instant::now();
        let mut sel = DeviceSelector::new();
        assert_eq!(sel.active(), None);
        // A TrackPoint drifting a count every 100 ms for 2 s
        for i in 0..20 {
            let at = t + Duration::from_millis(100 * i);
            assert!(!sel.accept(TRACKPOINT, &mv(1, 0, at)));
        }
        assert_eq!(sel.active(), None);
    }

    #[test]
    fn auto_selects_the_mouse_the_user_moves() {
        let t = Instant::now();
        let mut sel = DeviceSelector::new();
        // A slow, deliberate move: 2 counts per 1 ms report at 1000 Hz
        let accepted: Vec<bool> = (0..20)
            .map(|i| sel.accept(MOUSE, &mv(2, -1, t + Duration::from_millis(i))))
            .collect();
        assert_eq!(sel.active(), Some(MOUSE));
        // Claimed within the first 10 reports (30 counts), then everything passes
        assert!(accepted[..10].iter().any(|&a| a));
        assert!(accepted[10..].iter().all(|&a| a));
        // An imperfect hand: tiny wobbles and pauses are all still delivered
        let later = t + Duration::from_secs(3);
        assert!(sel.accept(MOUSE, &mv(0, 1, later)));
        assert!(sel.accept(
            MOUSE,
            &ev(RawInputKind::ButtonRelease(RawButton::Left), later)
        ));
    }

    #[test]
    fn a_click_or_wheel_notch_selects_immediately() {
        let t = Instant::now();
        let mut sel = DeviceSelector::new();
        assert!(sel.accept(MOUSE, &ev(RawInputKind::ButtonPress(RawButton::Left), t)));
        assert_eq!(sel.active(), Some(MOUSE));

        let mut sel = DeviceSelector::new();
        assert!(sel.accept(MOUSE, &ev(RawInputKind::Scroll { delta: -1 }, t)));
        assert_eq!(sel.active(), Some(MOUSE));

        // A stray release (e.g. of a click made before start-up) does not
        let mut sel = DeviceSelector::new();
        assert!(!sel.accept(MOUSE, &ev(RawInputKind::ButtonRelease(RawButton::Left), t)));
        assert_eq!(sel.active(), None);
    }

    #[test]
    fn auto_keeps_the_device_during_a_test_and_follows_the_user_otherwise() {
        let t = Instant::now();
        let mut sel = DeviceSelector::new();
        sel.accept(MOUSE, &ev(RawInputKind::ButtonPress(RawButton::Left), t));

        // Test running: a second device's clicks and motion are ignored
        sel.set_frozen(true);
        assert!(!sel.accept(
            TRACKPOINT,
            &ev(RawInputKind::ButtonPress(RawButton::Left), t)
        ));
        for i in 0..50 {
            assert!(!sel.accept(TRACKPOINT, &mv(5, 5, t + Duration::from_millis(i))));
        }
        assert_eq!(sel.active(), Some(MOUSE));
        assert!(sel.accept(MOUSE, &mv(1, 0, t)));

        // No test running: deliberate use of the other device switches
        sel.set_frozen(false);
        assert!(sel.accept(
            TRACKPOINT,
            &ev(RawInputKind::ButtonPress(RawButton::Left), t)
        ));
        assert_eq!(sel.active(), Some(TRACKPOINT));
        assert!(!sel.accept(MOUSE, &mv(1, 0, t)));
    }

    #[test]
    fn a_frozen_selector_with_nothing_measured_still_takes_the_used_device() {
        let t = Instant::now();
        let mut sel = DeviceSelector::new();
        sel.set_frozen(true);
        assert!(sel.accept(MOUSE, &ev(RawInputKind::ButtonPress(RawButton::Left), t)));
        assert_eq!(sel.active(), Some(MOUSE));
    }

    #[test]
    fn pinned_and_pointer_choices() {
        let t = Instant::now();
        let mut sel = DeviceSelector::new();
        sel.set_choice(DeviceChoice::Device(TRACKPOINT));
        assert_eq!(sel.active(), Some(TRACKPOINT));
        assert!(sel.accept(TRACKPOINT, &mv(1, 0, t)));
        assert!(!sel.accept(MOUSE, &ev(RawInputKind::ButtonPress(RawButton::Left), t)));

        // Back to Auto keeps measuring the pinned device until another is used
        sel.set_choice(DeviceChoice::Auto);
        assert_eq!(sel.active(), Some(TRACKPOINT));

        sel.set_choice(DeviceChoice::Pointer);
        assert_eq!(sel.active(), None);
        assert!(!sel.accept(TRACKPOINT, &mv(100, 0, t)));
    }

    #[test]
    fn unplugging_the_measured_device_clears_the_selection() {
        let t = Instant::now();
        let mut sel = DeviceSelector::new();
        sel.accept(MOUSE, &ev(RawInputKind::ButtonPress(RawButton::Left), t));
        sel.device_removed(MOUSE);
        assert_eq!(sel.active(), None);

        sel.set_choice(DeviceChoice::Device(TRACKPOINT));
        sel.device_removed(TRACKPOINT);
        assert_eq!(sel.choice(), DeviceChoice::Auto);
        assert_eq!(sel.active(), None);
    }

    /// Simulate `secs` of 60 fps frames.
    fn frames(
        check: &mut PointerCrossCheck,
        start: Instant,
        secs: f64,
        mut observe: impl FnMut(usize) -> (bool, bool),
    ) -> (bool, Instant) {
        let frame = Duration::from_micros(16_667);
        let count = (secs / frame.as_secs_f64()) as usize;
        let mut now = start;
        let mut mismatch = false;
        for i in 0..count {
            now += frame;
            let (pointer, raw) = observe(i);
            mismatch = check.update(now, pointer, raw);
        }
        (mismatch, now)
    }

    #[test]
    fn cross_check_flags_a_pointer_moved_by_another_device() {
        let mut check = PointerCrossCheck::new();
        let (mismatch, now) = frames(&mut check, Instant::now(), 0.9, |_| (true, false));
        assert!(!mismatch, "less than a second of motion is not enough");
        let (mismatch, now) = frames(&mut check, now, 0.2, |_| (true, false));
        assert!(mismatch);
        // Stays flagged while the pointer rests, clears when the device moves
        let (mismatch, now) = frames(&mut check, now, 2.0, |_| (false, false));
        assert!(mismatch);
        assert!(!check.update(now + Duration::from_millis(16), true, true));
    }

    #[test]
    fn cross_check_accepts_the_measured_mouse_with_frame_lag_and_pauses() {
        let mut check = PointerCrossCheck::new();
        // Raw motion arrives every other frame (pointer events lag a frame),
        // with pauses and slow stretches where only some frames see a report.
        let (mismatch, _) = frames(&mut check, Instant::now(), 30.0, |i| {
            let moving = (i / 90) % 3 != 2; // 1.5 s moving, 0.75 s still
            let raw = moving && (i % 2 == 0 || i % 7 == 0);
            (moving, raw)
        });
        assert!(!mismatch);
    }

    #[test]
    fn cross_check_ignores_isolated_pointer_frames() {
        let mut check = PointerCrossCheck::new();
        // A single pointer event every half second (window focus changes,
        // scroll-area relayout) never adds up to a mismatch
        let (mismatch, _) = frames(&mut check, Instant::now(), 60.0, |i| (i % 30 == 0, false));
        assert!(!mismatch);
    }

    #[test]
    fn bridge_follows_plug_events_and_drops_stale_events() {
        let (mut bridge, sender) = InputBridge::for_test();
        let t = Instant::now();
        sender
            .send(BridgeMessage::Added(device(7, "Receiver", "event10")))
            .unwrap();
        sender
            .send(BridgeMessage::Added(device(8, "Gaming Mouse", "event3")))
            .unwrap();
        sender
            .send(BridgeMessage::Event(
                8,
                ev(RawInputKind::ButtonPress(RawButton::Left), t),
            ))
            .unwrap();
        let events = bridge.poll();
        assert_eq!(events.len(), 1);
        let names: Vec<&str> = bridge.devices().iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Gaming Mouse", "Receiver"]);
        assert_eq!(bridge.active_device().map(|d| d.id), Some(8));
        assert!(bridge.is_delivering());

        // Unplugged: its last queued events must not re-select it
        sender.send(BridgeMessage::Removed(8)).unwrap();
        sender
            .send(BridgeMessage::Event(
                8,
                ev(RawInputKind::ButtonPress(RawButton::Left), t),
            ))
            .unwrap();
        assert!(bridge.poll().is_empty());
        assert!(!bridge.is_delivering());
        assert_eq!(bridge.devices().len(), 1);

        // Replugged under a new id: picked up on first use
        sender
            .send(BridgeMessage::Added(device(9, "Gaming Mouse", "event3")))
            .unwrap();
        sender
            .send(BridgeMessage::Event(
                9,
                ev(RawInputKind::ButtonPress(RawButton::Left), t),
            ))
            .unwrap();
        assert_eq!(bridge.poll().len(), 1);
        assert_eq!(bridge.active_device().map(|d| d.id), Some(9));
    }

    #[test]
    fn bridge_releases_a_device_that_is_not_moving_the_pointer() {
        let (mut bridge, sender) = InputBridge::for_test();
        let t = Instant::now();
        sender
            .send(BridgeMessage::Added(device(2, "TrackPoint", "event2")))
            .unwrap();
        sender
            .send(BridgeMessage::Event(
                2,
                ev(RawInputKind::ButtonPress(RawButton::Left), t),
            ))
            .unwrap();
        bridge.poll();
        assert!(bridge.is_delivering());

        // During a test the device is kept; the mismatch is only reported
        bridge.set_selection_frozen(true);
        let mut now = t;
        for _ in 0..90 {
            now += Duration::from_millis(16);
            bridge.cross_check_pointer(now, true, &[]);
        }
        assert!(bridge.pointer_mismatch());
        assert!(bridge.is_delivering());

        // With no test running it is released: framework input is used
        bridge.set_selection_frozen(false);
        now += Duration::from_millis(16);
        bridge.cross_check_pointer(now, true, &[]);
        assert!(!bridge.is_delivering());
    }

    #[test]
    fn wheel_accumulator_adds_up_high_resolution_deltas() {
        let mut wheel = WheelAccumulator::default();
        // A high-resolution wheel: 30 per event, 4 events per notch
        let notches: Vec<i32> = (0..8).map(|_| wheel.add(30)).collect();
        assert_eq!(notches, [0, 0, 0, 1, 0, 0, 0, 1]);
        // Standard wheels: one event per notch, sometimes several at once
        assert_eq!(wheel.add(120), 1);
        assert_eq!(wheel.add(-120), -1);
        assert_eq!(wheel.add(240), 2);
        // A partial notch is dropped when the direction reverses
        assert_eq!(wheel.add(90), 0);
        assert_eq!(wheel.add(-90), 0);
        assert_eq!(wheel.add(-30), -1);
        assert_eq!(wheel.add(0), 0);
    }

    #[test]
    fn windows_device_labels() {
        let (name, location) = windows_device_label(
            r"\\?\HID#VID_046D&PID_C08B&MI_00#7&2a1b3c&0&0000#{378de44c-56ef-11d1-bc8c-00a0c91405dd}",
            1,
        );
        assert_eq!(name, "HID mouse VID_046D PID_C08B");
        assert_eq!(location, "MI_00");
        let (name, location) = windows_device_label(r"\\?\ACPI#PNP0F13#4&1&0#{...}", 0x10);
        assert_eq!(name, "Mouse (ACPI PNP0F13)");
        assert_eq!(location, "handle 0x10");
        let (name, _) = windows_device_label("", 3);
        assert_eq!(name, "Mouse");
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;
    use evdev::{EventType, InputEvent, KeyCode, RelativeAxisCode, SynchronizationCode};
    use std::time::SystemTime;

    fn rel(axis: RelativeAxisCode, value: i32) -> evdev::EventSummary {
        InputEvent::new(EventType::RELATIVE.0, axis.0, value).destructure()
    }

    fn key(code: KeyCode, value: i32) -> evdev::EventSummary {
        InputEvent::new(EventType::KEY.0, code.0, value).destructure()
    }

    fn syn() -> evdev::EventSummary {
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        )
        .destructure()
    }

    #[test]
    fn batched_reports_keep_their_kernel_spacing() {
        // Eight 1000 Hz reports that sat in the kernel buffer and were read
        // in one batch 0.5 ms after the last one
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut batch = Vec::new();
        for i in 0..8u64 {
            let at = base + Duration::from_millis(i);
            batch.push((at, rel(RelativeAxisCode::REL_X, 3)));
            batch.push((at, rel(RelativeAxisCode::REL_Y, -1)));
            batch.push((at, syn()));
        }
        let ref_system = base + Duration::from_micros(7_500);
        let ref_instant = Instant::now();
        let out = translate_reports(batch, |ts| {
            kernel_time_to_instant(ts, ref_system, ref_instant)
        });
        assert_eq!(out.len(), 8, "one Move per report");
        for pair in out.windows(2) {
            let gap = pair[1].timestamp - pair[0].timestamp;
            assert_eq!(gap, Duration::from_millis(1));
        }
        assert_eq!(
            ref_instant - out[7].timestamp,
            Duration::from_micros(500),
            "stamped when the device reported, not when read"
        );
        assert!(out
            .iter()
            .all(|e| matches!(e.kind, RawInputKind::Move { dx: 3, dy: -1 })));
    }

    #[test]
    fn untrustworthy_kernel_times_fall_back_to_the_read_time() {
        let now_system = SystemTime::now();
        let now = Instant::now();
        // Stamped after the reference: the wall clock was stepped back
        let future = now_system + Duration::from_millis(5);
        assert_eq!(kernel_time_to_instant(future, now_system, now), now);
        // Hours old: the clock was stepped forward or the system slept
        let stale = now_system - Duration::from_secs(3600);
        assert_eq!(kernel_time_to_instant(stale, now_system, now), now);
    }

    #[test]
    fn buttons_wheel_and_trailing_motion_are_translated() {
        let t = SystemTime::now();
        let now = Instant::now();
        let batch = vec![
            (t, key(KeyCode::BTN_LEFT, 1)),
            (t, syn()),
            (t, key(KeyCode::BTN_LEFT, 2)), // autorepeat: ignored
            (t, key(KeyCode::BTN_EXTRA, 0)),
            (t, rel(RelativeAxisCode::REL_WHEEL, -1)),
            (t, rel(RelativeAxisCode::REL_WHEEL_HI_RES, -120)),
            (t, syn()),
            (t, key(KeyCode::KEY_A, 1)),
            (t, rel(RelativeAxisCode::REL_X, 7)),
        ];
        let out = translate_reports(batch, |_| now);
        let kinds: Vec<String> = out.iter().map(|e| format!("{:?}", e.kind)).collect();
        assert_eq!(
            kinds,
            [
                "ButtonPress(Left)",
                "ButtonRelease(Extra)",
                "Scroll { delta: -1 }",
                "Move { dx: 7, dy: 0 }",
            ]
        );
    }
}
