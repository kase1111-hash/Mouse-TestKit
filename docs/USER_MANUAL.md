# Mouse TRAP User Manual

Precision diagnostics for your mouse.

---

## Getting Started

Launch the application and you'll see the Dashboard. Use the sidebar to navigate between tests.

The Dashboard shows the **input source**. "RAW INPUT" means the app reads the mouse hardware directly and timing tests are precise. "FRAMEWORK" means it only sees what the window system delivers (frame-rate limited); on Linux, add your user to the `input` group (`sudo usermod -aG input $USER`) and log in again to get raw input.

---

## Auto Diagnostics (recommended first step)

A one-minute guided check-up that looks for faults by itself. Available from the Dashboard ("Run Check-up"), the sidebar, or CLI menu option 13.

1. Click **Start Diagnostics**, then take your hand off the mouse
2. Follow the instruction shown for each step. Every step ends on its own once enough data is collected or its time runs out:
   - **Sensor at rest** (5 s) - do not touch the mouse. Any movement, click or scroll here is phantom input
   - **Continuous movement** (up to 12 s) - move in smooth circles. Measures polling rate, stutters, unstable rate, dropouts
   - **Button clicks** (up to 20 s) - about 20 normal left clicks, then a few right clicks. Looks for switch bounce (accidental double clicks), presses too short for a finger, sticky releases, buttons that never release
   - **Scroll wheel** (up to 12 s) - about 10 notches down, then 10 up. Looks for notches reported in the wrong direction (encoder skipping) and a dead direction
   - **Lift-off** (10 s) - move, lift the mouse about 1 cm, set it down, move again, three times. Looks for cursor jumps when the sensor re-acquires the surface
3. Read the verdict: **Healthy**, **Needs attention** (warnings), **Fault detected** (a critical finding), or **Inconclusive** (not enough data - re-run and follow the instructions for the whole step)
4. Each finding explains what was measured and what it usually means. Tick **Show raw measurements** for the numbers behind it
5. Press **Abort** to stop early (a partial report is still produced) or **Run Again** to repeat

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
