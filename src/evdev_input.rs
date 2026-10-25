//! Reading a mouse's evdev node (Linux)
//!
//! Shared by the CLI tests and the GUI's input bridge:
//!
//! - The REL_X and REL_Y events of one hardware report become one `Move`,
//!   and every event is stamped with the kernel's time of the report, not
//!   the time it was read.
//! - The kernel queues a reader's events in a small buffer: about 128
//!   events for a mouse, 5 ms of an 8000 Hz mouse's reports or 40 ms at
//!   1000 Hz. When the reader falls behind, the kernel drops what it had
//!   queued and inserts `SYN_DROPPED`. [`ReportReader`] starts a new
//!   [`InputBatch`] with `lost_data` set there, so timing analyses do not
//!   take the hole for a gap between reports, skips the incomplete report
//!   that follows the marker, and releases a button whose release was
//!   dropped (checked against the kernel's own key state, `EVIOCGKEY`), so
//!   it does not look stuck. The `evdev` crate's own resynchronisation is
//!   not used: it reports no gap, and it loses a release that arrives in
//!   the incomplete report after the marker.
//! - [`EvdevMouse::read`] drains everything queued (the `evdev` crate reads
//!   at most 32 events per call, about 10 reports, which an 8000 Hz mouse
//!   outpaces) and [`EvdevMouse::wait`] sleeps until the device has data,
//!   so a reader wakes when reports arrive instead of on a fixed timer.

use crate::types::{DeviceLost, InputBatch, MouseButton, MouseEvent, TimedEvent};
use evdev::raw_stream::RawDevice;
use evdev::{EventSummary, InputEvent, RelativeAxisCode, SynchronizationCode};
use std::io::{self, ErrorKind};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant, SystemTime};

/// Upper bound on reads per [`EvdevMouse::read`] call. Each read returns at
/// most 32 events and the kernel queues at most a few hundred, so this only
/// stops a device that keeps the queue full from holding the caller forever.
const MAX_READS_PER_CALL: usize = 64;

/// Map an evdev timestamp (CLOCK_REALTIME) onto the monotonic clock using a
/// `(SystemTime, Instant)` pair read at the same moment. Events mapped with
/// the same pair keep their exact spacing. A timestamp after the reference
/// (the wall clock was stepped back) maps to the reference; so does one
/// older than `max_age`, when given (the clock was stepped forward or the
/// system slept, and the read time is the better estimate).
pub fn kernel_time_to_instant(
    timestamp: SystemTime,
    ref_system: SystemTime,
    ref_instant: Instant,
    max_age: Option<Duration>,
) -> Instant {
    match ref_system.duration_since(timestamp) {
        Ok(age) if max_age.is_none_or(|max| age <= max) => {
            ref_instant.checked_sub(age).unwrap_or(ref_instant)
        }
        _ => ref_instant,
    }
}

/// Turns the events read from a mouse's evdev node into one event per
/// hardware report, split at every place the kernel dropped reports.
#[derive(Debug, Default)]
pub struct ReportReader {
    /// Buttons whose press was passed on and whose release was not yet.
    down: Vec<MouseButton>,
    /// Between a `SYN_DROPPED` and the `SYN_REPORT` that ends the incomplete
    /// report after it (this can span reads).
    skipping: bool,
    /// When the last drop not yet resynchronised happened.
    dropped_at: Option<Instant>,
    /// Buttons in `down` whose press came after that drop: their release
    /// cannot have been dropped in it.
    pressed_since_drop: Vec<MouseButton>,
}

impl ReportReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Translate events read from the device, in order. Returns one batch,
    /// plus one more (with `lost_data`) after each `SYN_DROPPED`. A batch
    /// can be empty; one after a drop is returned even when empty.
    pub fn translate(
        &mut self,
        events: &[InputEvent],
        to_instant: impl Fn(SystemTime) -> Instant,
    ) -> Vec<InputBatch> {
        let mut batches = vec![InputBatch::default()];
        // Motion of the current report: (time of its first axis event, dx, dy)
        let mut motion: Option<(Instant, i32, i32)> = None;
        let flush = |batches: &mut Vec<InputBatch>, motion: Option<(Instant, i32, i32)>| {
            if let Some((time, dx, dy)) = motion {
                if dx != 0 || dy != 0 {
                    push(batches, time, MouseEvent::Move { dx, dy });
                }
            }
        };

        for ev in events {
            match ev.destructure() {
                EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
                    // The unfinished report before the marker lost its end.
                    motion = None;
                    self.skipping = true;
                    self.dropped_at = Some(to_instant(ev.timestamp()));
                    self.pressed_since_drop.clear();
                    let last = batches.last_mut().expect("never empty");
                    if last.events.is_empty() {
                        last.lost_data = true;
                    } else {
                        batches.push(InputBatch {
                            events: Vec::new(),
                            lost_data: true,
                        });
                    }
                }
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    if self.skipping {
                        // End of the incomplete report after the marker
                        self.skipping = false;
                        motion = None;
                    } else {
                        flush(&mut batches, motion.take());
                    }
                }
                _ if self.skipping => {}
                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_X, value) => {
                    motion
                        .get_or_insert_with(|| (to_instant(ev.timestamp()), 0, 0))
                        .1 += value;
                }
                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_Y, value) => {
                    motion
                        .get_or_insert_with(|| (to_instant(ev.timestamp()), 0, 0))
                        .2 += value;
                }
                // One notch is reported both as REL_WHEEL (+-1) and, on newer
                // kernels, as REL_WHEEL_HI_RES (+-120). Only the notch-granular
                // event is used so a notch counts once.
                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_WHEEL, delta) => {
                    push(
                        &mut batches,
                        to_instant(ev.timestamp()),
                        MouseEvent::Scroll { delta },
                    );
                }
                EventSummary::Key(_, key, value) => {
                    let button = MouseButton::from(key);
                    if button == MouseButton::Unknown {
                        continue;
                    }
                    let event = match value {
                        1 => {
                            if !self.down.contains(&button) {
                                self.down.push(button);
                            }
                            if !self.pressed_since_drop.contains(&button) {
                                self.pressed_since_drop.push(button);
                            }
                            MouseEvent::ButtonPress(button)
                        }
                        0 => {
                            self.down.retain(|&b| b != button);
                            self.pressed_since_drop.retain(|&b| b != button);
                            MouseEvent::ButtonRelease(button)
                        }
                        _ => continue, // autorepeat (2)
                    };
                    push(&mut batches, to_instant(ev.timestamp()), event);
                }
                _ => {}
            }
        }
        // A report cut off at the end of the read still moved the mouse
        flush(&mut batches, motion);
        batches
    }

    /// Whether a drop happened that [`Self::resync`] still has to check
    /// buttons for: only buttons passed on as pressed can need a release.
    pub fn needs_resync(&self) -> bool {
        self.dropped_at.is_some() && !self.down.is_empty()
    }

    /// After a drop: release every button passed on as pressed that
    /// `is_down` (the device's own key state) says is up. One held at the
    /// drop lost its release among the dropped events: that release goes at
    /// the start of the last batch with `lost_data` in `batches` (added if
    /// there is none), stamped with the time of the drop, which is no later
    /// than the events after it. One first pressed after the drop was
    /// released after the queue was read (reading the key state discards
    /// that release from the queue): its release goes at the end, stamped
    /// `read_at`. A button pressed during the gap is not invented: its press
    /// time is unknown.
    pub fn resync(
        &mut self,
        batches: &mut Vec<InputBatch>,
        read_at: Instant,
        is_down: impl Fn(MouseButton) -> bool,
    ) {
        let Some(at) = self.dropped_at.take() else {
            return;
        };
        let pressed_since = std::mem::take(&mut self.pressed_since_drop);
        let released: Vec<MouseButton> =
            self.down.iter().copied().filter(|&b| !is_down(b)).collect();
        if released.is_empty() {
            return;
        }
        self.down.retain(|b| !released.contains(b));
        let (late, dropped): (Vec<MouseButton>, Vec<MouseButton>) = released
            .into_iter()
            .partition(|b| pressed_since.contains(b));
        if let Some(last) = batches.last_mut() {
            last.events
                .extend(late.into_iter().map(|button| TimedEvent {
                    time: read_at,
                    event: MouseEvent::ButtonRelease(button),
                }));
        }
        if dropped.is_empty() {
            return;
        }
        let index = match batches.iter().rposition(|b| b.lost_data) {
            Some(index) => index,
            None => {
                batches.push(InputBatch {
                    events: Vec::new(),
                    lost_data: true,
                });
                batches.len() - 1
            }
        };
        let releases = dropped.into_iter().map(|button| TimedEvent {
            time: at,
            event: MouseEvent::ButtonRelease(button),
        });
        batches[index].events.splice(0..0, releases);
    }

    /// Forget a drop that needs no resynchronisation.
    fn drop_settled(&mut self) {
        self.dropped_at = None;
        self.pressed_since_drop.clear();
    }
}

fn push(batches: &mut [InputBatch], time: Instant, event: MouseEvent) {
    if let Some(batch) = batches.last_mut() {
        batch.events.push(TimedEvent { time, event });
    }
}

/// A mouse's evdev node opened for reading, with drop handling.
#[derive(Debug)]
pub struct EvdevMouse {
    device: RawDevice,
    reader: ReportReader,
    /// Reading failed for good (ENODEV after an unplug, or another error).
    gone: bool,
}

impl EvdevMouse {
    /// Take over an opened device and make its reads non-blocking.
    pub fn new(device: RawDevice) -> io::Result<Self> {
        let fd = device.as_raw_fd();
        // SAFETY: fcntl on a file descriptor this device owns.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above; only O_NONBLOCK is added.
        if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            device,
            reader: ReportReader::new(),
            gone: false,
        })
    }

    pub fn device(&self) -> &RawDevice {
        &self.device
    }

    pub fn device_mut(&mut self) -> &mut RawDevice {
        &mut self.device
    }

    /// Sleep until the device has input or has failed, for at most
    /// `timeout` (`None`: no limit). Returns whether it is worth reading
    /// (`false` on a timeout or a signal).
    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        if self.gone {
            return true;
        }
        let ms = match timeout {
            None => -1,
            // Rounded up: a sub-millisecond wait must not become a spin
            Some(t) => t.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32,
        };
        let mut fds = libc::pollfd {
            fd: self.device.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&mut fds, 1, ms) };
        match ready {
            0 => false,
            // Interrupted by a signal: nothing to read yet. Any other error
            // is left for the read to report, so a caller never spins.
            _ if ready < 0 => io::Error::last_os_error().kind() != ErrorKind::Interrupted,
            _ => true,
        }
    }

    /// Read everything the device has queued, without waiting. Returns the
    /// batches it forms (see [`ReportReader::translate`]; empty batches
    /// without a gap are left out), or [`DeviceLost`] once the device has
    /// failed and everything read before that was returned. `max_age` is
    /// passed to [`kernel_time_to_instant`].
    pub fn read(&mut self, max_age: Option<Duration>) -> Result<Vec<InputBatch>, DeviceLost> {
        if self.gone {
            return Err(DeviceLost);
        }
        let mut raw: Vec<InputEvent> = Vec::new();
        for _ in 0..MAX_READS_PER_CALL {
            match self.device.fetch_events() {
                Ok(events) => {
                    let before = raw.len();
                    raw.extend(events);
                    if raw.len() == before {
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                // ENODEV after an unplug, or another fatal error
                Err(_) => {
                    self.gone = true;
                    break;
                }
            }
        }
        if raw.is_empty() && self.gone {
            return Err(DeviceLost);
        }

        // One reference pair per read, taken after it, so the reports read
        // together keep their kernel spacing.
        let ref_system = SystemTime::now();
        let ref_instant = Instant::now();
        let mut batches = self.reader.translate(&raw, |time| {
            kernel_time_to_instant(time, ref_system, ref_instant, max_age)
        });
        if self.reader.needs_resync() {
            // Everything queued was just read, so the key events the ioctl
            // discards from the queue are at most a few microseconds old.
            match self.device.get_key_state() {
                Ok(keys) => self.reader.resync(&mut batches, ref_instant, |button| {
                    button.key_code().is_some_and(|code| keys.contains(code))
                }),
                Err(_) => self.gone = true,
            }
        } else {
            self.reader.drop_settled();
        }
        batches.retain(|b| b.lost_data || !b.events.is_empty());
        Ok(batches)
    }
}

/// Whether the evdev node `node` (e.g. `event5`) reports relative X motion:
/// a mouse, trackball or pointing stick. Read from sysfs, which anyone may
/// read, so it also works for a node the user is not allowed to open.
pub fn node_reports_rel_x(node: &str) -> bool {
    std::fs::read_to_string(format!("/sys/class/input/{node}/device/capabilities/rel"))
        .is_ok_and(|bits| rel_bits_include_x(&bits))
}

/// Whether a sysfs `capabilities/rel` bitmap includes REL_X (bit 0). The
/// file lists hexadecimal words, most significant first; the relative axes
/// all fit in the last one.
fn rel_bits_include_x(bits: &str) -> bool {
    bits.split_whitespace()
        .last()
        .and_then(|word| u64::from_str_radix(word, 16).ok())
        .is_some_and(|word| word & 1 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::{EventType, KeyCode};

    fn rel(axis: RelativeAxisCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::RELATIVE.0, axis.0, value)
    }

    fn x(value: i32) -> InputEvent {
        rel(RelativeAxisCode::REL_X, value)
    }

    fn key(code: KeyCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::KEY.0, code.0, value)
    }

    fn syn() -> InputEvent {
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        )
    }

    fn dropped() -> InputEvent {
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_DROPPED.0,
            0,
        )
    }

    fn kinds(batch: &InputBatch) -> Vec<String> {
        batch
            .events
            .iter()
            .map(|e| format!("{:?}", e.event))
            .collect()
    }

    fn translate(reader: &mut ReportReader, events: &[InputEvent]) -> Vec<InputBatch> {
        let now = Instant::now();
        reader.translate(events, |_| now)
    }

    #[test]
    fn x_and_y_of_one_report_become_one_move() {
        let mut reader = ReportReader::new();
        let events = [
            x(3),
            rel(RelativeAxisCode::REL_Y, -2),
            syn(),
            rel(RelativeAxisCode::REL_Y, 5),
            syn(),
        ];
        let batches = translate(&mut reader, &events);
        assert_eq!(batches.len(), 1);
        assert!(!batches[0].lost_data);
        assert_eq!(
            kinds(&batches[0]),
            ["Move { dx: 3, dy: -2 }", "Move { dx: 0, dy: 5 }"]
        );
    }

    #[test]
    fn buttons_wheel_and_trailing_motion_are_translated() {
        let mut reader = ReportReader::new();
        let events = [
            key(KeyCode::BTN_LEFT, 1),
            syn(),
            key(KeyCode::BTN_LEFT, 2), // autorepeat: ignored
            key(KeyCode::BTN_EXTRA, 0),
            rel(RelativeAxisCode::REL_WHEEL, -1),
            rel(RelativeAxisCode::REL_WHEEL_HI_RES, -120),
            syn(),
            key(KeyCode::KEY_A, 1),
            x(7),
        ];
        let batches = translate(&mut reader, &events);
        assert_eq!(
            kinds(&batches[0]),
            [
                "ButtonPress(Left)",
                "ButtonRelease(Extra)",
                "Scroll { delta: -1 }",
                "Move { dx: 7, dy: 0 }",
            ]
        );
    }

    #[test]
    fn batched_reports_keep_their_kernel_spacing() {
        // Eight 1000 Hz reports that sat in the kernel buffer and were read
        // in one batch 0.5 ms after the last one
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut events = Vec::new();
        for i in 0..8u64 {
            let at = base + Duration::from_millis(i);
            for ev in [x(3), rel(RelativeAxisCode::REL_Y, -1), syn()] {
                events.push(with_time(ev, at));
            }
        }
        let ref_system = base + Duration::from_micros(7_500);
        let ref_instant = Instant::now();
        let mut reader = ReportReader::new();
        let batches = reader.translate(&events, |ts| {
            kernel_time_to_instant(ts, ref_system, ref_instant, None)
        });
        let out = &batches[0].events;
        assert_eq!(out.len(), 8, "one Move per report");
        for pair in out.windows(2) {
            assert_eq!(pair[1].time - pair[0].time, Duration::from_millis(1));
        }
        assert_eq!(
            ref_instant - out[7].time,
            Duration::from_micros(500),
            "stamped when the device reported, not when read"
        );
    }

    /// `event` with its kernel timestamp set to `at`.
    fn with_time(event: InputEvent, at: SystemTime) -> InputEvent {
        let since = at.duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let raw = libc::input_event {
            time: libc::timeval {
                tv_sec: since.as_secs() as libc::time_t,
                tv_usec: since.subsec_micros() as libc::suseconds_t,
            },
            type_: event.event_type().0,
            code: event.code(),
            value: event.value(),
        };
        InputEvent::from(raw)
    }

    #[test]
    fn untrustworthy_kernel_times_fall_back_to_the_read_time() {
        let now_system = SystemTime::now();
        let now = Instant::now();
        let max = Some(Duration::from_secs(1));
        // Stamped after the reference: the wall clock was stepped back
        let future = now_system + Duration::from_millis(5);
        assert_eq!(kernel_time_to_instant(future, now_system, now, max), now);
        assert_eq!(kernel_time_to_instant(future, now_system, now, None), now);
        // Hours old: the clock was stepped forward or the system slept
        let stale = now_system - Duration::from_secs(3600);
        assert_eq!(kernel_time_to_instant(stale, now_system, now, max), now);
        // Without a limit, an old report keeps its age (input queued while
        // a prompt waited is then recognised as older than the test)
        let queued = now_system - Duration::from_secs(5);
        if let Some(expected) = now.checked_sub(Duration::from_secs(5)) {
            assert_eq!(
                kernel_time_to_instant(queued, now_system, now, None),
                expected
            );
        }
    }

    #[test]
    fn a_steady_stream_never_reports_lost_data() {
        // A healthy 8000 Hz mouse moving fast for a second, read in chunks
        let mut reader = ReportReader::new();
        let mut moves = 0;
        for _ in 0..250 {
            let mut chunk = Vec::new();
            for _ in 0..32 {
                chunk.extend([x(2), rel(RelativeAxisCode::REL_Y, 1), syn()]);
            }
            for batch in translate(&mut reader, &chunk) {
                assert!(!batch.lost_data);
                moves += batch.events.len();
            }
        }
        assert_eq!(moves, 250 * 32);
        assert!(!reader.needs_resync());
    }

    #[test]
    fn dropped_reports_start_a_new_batch_and_skip_the_incomplete_report() {
        // Before this was handled, the 9 counts of the incomplete report were
        // passed on and the hole was invisible: the next interval read as a
        // stutter of the mouse.
        let mut reader = ReportReader::new();
        let events = [x(3), syn(), dropped(), x(9), syn(), x(2), syn()];
        let batches = translate(&mut reader, &events);
        assert_eq!(batches.len(), 2);
        assert!(!batches[0].lost_data);
        assert_eq!(kinds(&batches[0]), ["Move { dx: 3, dy: 0 }"]);
        assert!(batches[1].lost_data);
        assert_eq!(kinds(&batches[1]), ["Move { dx: 2, dy: 0 }"]);
    }

    #[test]
    fn a_drop_at_the_start_marks_the_first_batch() {
        let mut reader = ReportReader::new();
        let batches = translate(
            &mut reader,
            &[
                dropped(),
                rel(RelativeAxisCode::REL_Y, 1),
                syn(),
                x(4),
                syn(),
            ],
        );
        assert_eq!(batches.len(), 1);
        assert!(batches[0].lost_data);
        assert_eq!(kinds(&batches[0]), ["Move { dx: 4, dy: 0 }"]);
    }

    #[test]
    fn the_incomplete_report_is_skipped_across_reads() {
        let mut reader = ReportReader::new();
        let first = translate(&mut reader, &[x(1), syn(), dropped(), x(9)]);
        assert_eq!(first.len(), 2);
        assert!(first[1].lost_data && first[1].events.is_empty());
        // The rest of the incomplete report arrives in the next read; the
        // gap was already reported
        let second = translate(
            &mut reader,
            &[rel(RelativeAxisCode::REL_Y, 9), syn(), x(2), syn()],
        );
        assert_eq!(second.len(), 1);
        assert!(!second[0].lost_data);
        assert_eq!(kinds(&second[0]), ["Move { dx: 2, dy: 0 }"]);
    }

    #[test]
    fn a_release_lost_in_a_drop_is_restored_and_no_press_is_invented() {
        let mut reader = ReportReader::new();
        // Left pressed and seen; then the queue overflows. Its release was
        // in the dropped part; the right button went down during the gap.
        let mut batches = translate(
            &mut reader,
            &[
                key(KeyCode::BTN_LEFT, 1),
                syn(),
                dropped(),
                x(5),
                syn(),
                x(1),
                syn(),
            ],
        );
        assert!(reader.needs_resync());
        reader.resync(&mut batches, Instant::now(), |b| b == MouseButton::Right);
        assert_eq!(batches.len(), 2);
        assert_eq!(kinds(&batches[0]), ["ButtonPress(Left)"]);
        assert!(batches[1].lost_data);
        assert_eq!(
            kinds(&batches[1]),
            ["ButtonRelease(Left)", "Move { dx: 1, dy: 0 }"]
        );
        assert!(!reader.needs_resync());

        // A button still held through the gap is left alone
        let mut reader = ReportReader::new();
        let mut batches = translate(
            &mut reader,
            &[key(KeyCode::BTN_LEFT, 1), syn(), dropped(), x(5), syn()],
        );
        reader.resync(&mut batches, Instant::now(), |b| b == MouseButton::Left);
        assert!(batches.iter().all(|b| b
            .events
            .iter()
            .all(|e| !matches!(e.event, MouseEvent::ButtonRelease(_)))));
        // ...and its real release later still passes
        let later = translate(&mut reader, &[key(KeyCode::BTN_LEFT, 0), syn()]);
        assert_eq!(kinds(&later[0]), ["ButtonRelease(Left)"]);
    }

    #[test]
    fn a_press_after_the_drop_released_before_the_key_read_stays_in_order() {
        // Left was up at the drop and pressed after it; it was released
        // after the queue was drained, and the key-state read discarded that
        // release from the queue. The release must come after the press.
        let mut reader = ReportReader::new();
        let mut batches = translate(
            &mut reader,
            &[dropped(), x(4), syn(), key(KeyCode::BTN_LEFT, 1), syn()],
        );
        assert!(reader.needs_resync());
        let read_at = Instant::now();
        reader.resync(&mut batches, read_at, |_| false);
        let events: Vec<String> = batches.iter().flat_map(kinds).collect();
        assert_eq!(events, ["ButtonPress(Left)", "ButtonRelease(Left)"]);
        let release = batches.last().unwrap().events.last().unwrap();
        assert_eq!(release.time, read_at, "not stamped at the drop");
        assert!(!reader.needs_resync());
        // Pressed again and released, both seen: nothing left to restore
        let later = translate(&mut reader, &[key(KeyCode::BTN_LEFT, 1), syn()]);
        assert_eq!(kinds(&later[0]), ["ButtonPress(Left)"]);
    }

    #[test]
    fn a_release_in_the_incomplete_report_is_not_lost() {
        // The evdev crate applies the skipped report to its key state
        // without passing it on, so this release went missing and the
        // button looked stuck. Here the kernel's key state restores it.
        let mut reader = ReportReader::new();
        let mut batches = translate(
            &mut reader,
            &[
                key(KeyCode::BTN_LEFT, 1),
                syn(),
                dropped(),
                key(KeyCode::BTN_LEFT, 0),
                syn(),
            ],
        );
        reader.resync(&mut batches, Instant::now(), |_| false);
        let released = batches
            .iter()
            .flat_map(|b| &b.events)
            .filter(|e| matches!(e.event, MouseEvent::ButtonRelease(MouseButton::Left)))
            .count();
        assert_eq!(released, 1);
    }

    #[test]
    fn sysfs_rel_bits() {
        // A typical mouse: REL_X, REL_Y, REL_HWHEEL, REL_WHEEL and their
        // high-resolution versions
        assert!(rel_bits_include_x("1943\n"));
        assert!(rel_bits_include_x("3"));
        // A wheel-only consumer-control node, a keyboard, garbage
        assert!(!rel_bits_include_x("140\n"));
        assert!(!rel_bits_include_x("0\n"));
        assert!(!rel_bits_include_x(""));
        assert!(!rel_bits_include_x("zz"));
    }
}
