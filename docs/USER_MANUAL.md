# Mouse-TestKit User Manual

Precision diagnostics for your mouse.

---

## Getting Started

Launch the application and you'll see the Dashboard. Use the sidebar to navigate between tests.

---

## Core Tests

### Polling Rate Monitor
Measures how often your mouse reports its position to the computer.

1. Click **Start**
2. Move your mouse around continuously
3. View real-time Hz readings and graph

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
Detects if mouse firmware forces diagonal movements to straight lines.

1. Click **Start Test**
2. Draw diagonal lines at various angles (15°, 30°, 45°, etc.)
3. Click **Stop Test**
4. If lines appear unnaturally straight, snapping is detected

---

### Acceleration Detection
Detects if cursor speed varies with movement speed.

1. Click **Start Test**
2. Alternate between SLOW and FAST movements
3. Move the same physical distance at different speeds
4. Click **Stop Test**

**1.0x factor** = No acceleration (ideal)

---

### Double-Click Test
Detects switch issues that cause accidental double-clicks.

1. Click the large button repeatedly
2. Clicks faster than threshold are flagged as accidental
3. Multiple accidental double-clicks may indicate failing switches

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
