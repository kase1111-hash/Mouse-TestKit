# Mouse TRAP User Manual

Precision diagnostics for your mouse.

---

## Getting Started

Launch the application and you'll see the Dashboard. Use the sidebar to navigate between tests.

The Dashboard shows the **input source** and which device the tests measure. Mouse TRAP reads every mouse-like device; in **Auto** it measures the one you click, scroll or move a few millimetres, and keeps that device while a test runs (a bumped second mouse or a drifting TrackPoint cannot take over). Use the **Measure** list to pin a specific device, or choose "Pointer (framework input)". "RAW INPUT" plus the device name means the app reads that mouse's hardware reports directly and timing tests are precise. "FRAMEWORK" means the tests only see what the window system delivers (frame-rate limited, after OS acceleration): this is the case until you use a readable mouse, when the pointer is driven by something that cannot be read directly (touchpad, tablet, remote session), and on Linux when your user is not in the `input` group (`sudo usermod -aG input $USER`, then log in again). If the pointer keeps moving while the measured device reports nothing, a warning appears: select the mouse you are actually testing. A mouse that is unplugged and plugged back in is picked up again within a few seconds.

---

## Auto Diagnostics (recommended first step)

A one-minute guided check-up that looks for faults by itself. Available from the Dashboard ("Run Check-up"), the sidebar, or CLI menu option 13.

1. Click **Start Diagnostics**, then take your hand off the mouse
2. Follow the instruction shown for each step. Every step ends on its own once enough data is collected or its time runs out:
   - **Sensor at rest** (about 7 s: 2 s to let go, then 5 s) - do not touch the mouse or the desk. Clicks or scrolls here are phantom input; movement counts as drift or jitter only when it keeps showing up (in 3 or more separate seconds). A single bump is noted but is not a fault
   - **Continuous movement** (up to 12 s) - move in brisk, wide circles. A mouse only sends a report when it has moved, so only reports sent while it moves fast enough to report on every poll are timed. If your movement is too slow to time (common at 2-8 kHz or low DPI) the step shows "move faster" and the polling checks are inconclusive rather than flagged. Measures polling rate, stutters (judged by the share of movement time lost to late reports), unstable rate, dropouts
   - **Button clicks** (up to 20 s) - point at the box shown and click: about 20 normal left clicks, then a few right clicks. Looks for switch bounce (accidental double clicks: one is a warning, a repeat is a fault), presses too short for a finger, sticky releases, buttons that never release (reported however few clicks were made)
   - **Scroll wheel** (up to 12 s) - about 10 notches down, then 10 up. Looks for notches reported in the wrong direction (encoder skipping)
   - **Lift-off** (10 s) - move, lift the mouse about 1 cm, set it down, move again, three times. Looks for cursor jumps when the sensor re-acquires the surface
3. Read the verdict: **Healthy**, **Needs attention** (warnings), **Fault detected** (a critical finding), or **Inconclusive** (a step did not collect enough data, or a button or wheel direction registered nothing - if you did use it, it may be dead; if you skipped it, re-run and follow the instructions for the whole step). A run is only Healthy when every step completed
4. Each finding explains what was measured and what it usually means. Tick **Show raw measurements** for the numbers behind it
5. Press **Abort** (top right) or **Esc** to stop early (a partial report is still produced) or **Run Again** to repeat. During the click step only Esc stops the run, so stray clicks cannot end it, and the sidebar export buttons stay disabled until the run ends

Without raw input (FRAMEWORK input source) the movement and lift-off steps are left out, and switch bounce, very short presses and scroll skips are not judged, because window-system events are frame-rate limited and pointer movement is accelerated. The report is marked "Limited check". Keep the pointer inside the window: clicks and scrolls outside it are not seen.

The report is included in the JSON/CSV export. If a warning appears once, re-run: a fault that repeats is real, a one-off is often the hand.

---

## Core Tests

### Polling Rate Monitor
Measures how often your mouse reports its position to the computer.

1. Click **Start**
2. Move your mouse around continuously
3. View real-time Hz readings and graph. "Nominal" is the nearest standard rate the measurement corresponds to

**Common polling rates:** 125Hz, 500Hz, 1000Hz, 4000Hz, 8000Hz

---

### Stutter Detection
Detects irregular timing between mouse events that can cause choppy cursor movement.

1. Click **Start**
2. Move mouse in circles or back-and-forth continuously
3. Spikes above the red threshold line indicate stutters

**Tip:** Adjust sensitivity slider - lower = more sensitive detection.

---

### Click Response
Tests button registration and measures hold duration.

1. Select **Left Click** or **Right Click**
2. Click **Start**
3. Click inside the test area
4. View clicks count, CPS (clicks per second), and hold times

---

### Click Stickiness
Detects stuck or delayed button releases (common switch failure symptom).

1. Select **Left Click** or **Right Click**
2. Click **Start**
3. Click rapidly in the test area
4. Holds >100ms are flagged as potentially sticky

---

### Lift-Off Jump
Detects cursor jumps when lifting the mouse off the surface.

1. Click **Start**
2. Move your mouse normally
3. Slowly lift the mouse off the pad
4. Large jumps during lift indicate high lift-off distance (LOD)

---

### USB Conflict Detection (CLI, Linux only)
Shows other devices connected to the same USB controller/hub that may cause bandwidth conflicts.

1. Select **USB Conflict Detection** from the CLI menu
2. The scanner reads `/sys/bus/usb/devices` to enumerate connected devices
3. Review results for potential bandwidth contention

---

### Scroll Wheel (GUI only)
Tests scroll wheel functionality and consistency. This test is only available in the GUI application.

1. Click **Start**
2. Hover over the test area
3. Scroll up and down
4. View step count, speed, and direction changes

---

## Advanced Tests

### DPI Accuracy
Verifies your mouse's actual DPI matches the configured setting.

1. Set your mouse DPI in its software
2. Enter the target DPI value
3. Enter the distance you'll move (use a ruler)
4. Press **SPACE** to start
5. Move the mouse exactly that distance
6. Press **SPACE** to finish

**95%+ accuracy** = Good DPI calibration

---

### Angle Snapping
Detects firmware that straightens near-horizontal or near-vertical hand movements into perfectly axis-aligned lines.

1. Click **Start Test**
2. Draw freehand lines that are *almost* horizontal or vertical (tilted 5-10°), one at a time, pausing briefly between them. Do not use a ruler
3. Draw at least 5 such lines, then click **Stop Test**
4. A slightly tilted freehand line that comes out exactly on-axis with zero wobble is the signature of snapping. If too few near-axis lines were drawn the result is **Inconclusive**

---

### Acceleration Detection
Detects if the number of counts for a fixed distance changes with movement speed.

1. Pick a fixed physical distance (for example the full width of your mousepad, or between two marks)
2. Click **Start Test**
3. Move across that exact distance, pause, move back, pause - about 5 passes slowly
4. Do about 5 more passes quickly, then click **Stop Test**

Each pass must be at least 50 counts and the fast passes must be clearly faster than the slow ones, otherwise the result is **Inconclusive**.

**1.0x factor** = No acceleration (ideal). Above 1.15x or below 0.85x is reported as acceleration.

---

### Double-Click Test
Detects switch issues that cause accidental double-clicks.

1. Click **Start**, then click the large target repeatedly at a normal pace
2. A press closer than the threshold to the previous press, or within 25 ms of the previous release, is flagged as accidental (a finger cannot do either)
3. Multiple accidental double-clicks indicate a failing switch

---

### Jitter Test
Measures sensor noise when the mouse is stationary.

1. Place mouse on pad and **don't touch it**
2. Click **Take Sample (5s)**
3. Wait 5 seconds without moving the mouse
4. Lower distance = less jitter = better sensor

---

## Tips

- Close other applications for most accurate timing tests
- Use a consistent mousepad surface
- Let mouse warm up for a few minutes for best results

---

## About

Click the **About** button in the sidebar to view version info.

Licensed under MIT License.
