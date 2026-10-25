# Mouse TRAP User Manual

Precision diagnostics for your mouse.

---

## Getting Started

Launch the application and you'll see the Dashboard. Use the sidebar to navigate between tests.

The Dashboard shows the **input source** and which device the tests measure. Mouse TRAP reads every mouse-like device; in **Auto** it measures the one you click, scroll or move a few millimetres, and keeps that device while a test runs (a bumped second mouse or a drifting TrackPoint cannot take over). A run measures one device: choosing another in the **Measure** list during a run, or another device taking over after the measured one was unplugged, ends every test running (on whichever page), so two mice are never mixed in one result: Polling Rate, Stutter, Lift-Off, Scroll Wheel, DPI and Jitter say so on their page, the click, double-click, angle-snapping and acceleration tests stop and keep what was measured, and Auto Diagnostics does not judge the step in progress. Use the **Measure** list to pin a specific device, or choose "Pointer (framework input)". "RAW INPUT" plus the device name means the app reads that mouse's hardware reports directly and timing tests are precise. "FRAMEWORK" means the tests only see what the window system delivers (frame-rate limited, after OS acceleration): this is the case until you use a readable mouse, when the pointer is driven by something that cannot be read directly (touchpad, tablet, remote session), and on Linux when your user is not in the `input` group (`sudo usermod -aG input $USER`, then log in again). Whenever a mouse-like device cannot be opened for lack of permission, the Dashboard says so with that fix, even if another device (for example a virtual one the desktop lets every user open) can be read. If the pointer keeps moving while the measured device reports nothing, a warning appears: select the mouse you are actually testing. A mouse that is unplugged and plugged back in is picked up again within a few seconds. On Windows each device in the list shows its model (VID/PID), interface and instance ID, so two units of the same model have different entries. If the computer falls behind reading the mouse while a test runs (the system then drops some of its reports), the test's page says so, and so does the JSON/CSV export (`reports_lost`, or a "Reports Lost" row for that test). As in the CLI, nothing is timed across the lost reports (they are not counted as a stutter, bounce, slow release or lift-off jump), and a DPI measurement, acceleration pass or jitter sample cut by them is set aside to be taken again (an angle-snapping line is dropped); clicks or movement lost with them are missing from the results, so run the test again to confirm.

---

## Auto Diagnostics (recommended first step)

A one-minute guided check-up that looks for faults by itself. Available from the Dashboard ("Run Check-up"), the sidebar, or CLI menu option 13.

1. Click **Start Diagnostics**, then take your hand off the mouse
2. Follow the instruction shown for each step. Every step ends on its own once enough data is collected or its time runs out:
   - **Sensor at rest** (about 7 s: 2 s to let go, then 5 s) - do not touch the mouse or the desk. Clicks or scrolls here are phantom input; movement counts as drift or jitter only when it keeps showing up (in 3 or more separate seconds). A single bump is noted but is not a fault, and neither is a hand that leaves late: a burst at least as large as all the movement before it, followed by at least a second of stillness, is taken (with anything before it) as the hand letting go. A sensor that keeps reporting small amounts all through the step is still judged as jitter
   - **Continuous movement** (up to 12 s) - move in brisk, wide circles without lifting the mouse. A mouse only sends a report when it has moved, so only reports sent while it moves fast enough to report on every poll are timed. If your movement is too slow to time (common at 2-8 kHz or low DPI) the step shows "move faster" and the polling checks are inconclusive rather than flagged. Measures polling rate, stutters (judged by the share of movement time lost to late reports), unstable rate, and freezes: the reports stopping for 0.1-1 s while the mouse was moving, and then either the held-back movement arriving all at once (the cursor freezes and jumps) or, at 1 kHz and up, the reports going on at full speed with the movement in between lost (the cursor stops and carries on). No hand stops and sets off again without slowing down and speeding up, which shows in the reports at 1 kHz and up; at 125-500 Hz a hand can stop within a poll or two, so only freezes that hold the movement back are judged there. A mouse lifted while moving fast and set down still moving looks like a lost-movement freeze, so do not lift it. One freeze is only noted, as a moment of interference can cause it; two or more are a warning. Fast reports pinned at the largest value the mouse's report format holds (127 counts for 8-bit reports, 2047 for 12-bit, 32767 for 16-bit), with none larger, are a warning of their own: the mouse cannot report faster movement and loses or delays it (lower the DPI or raise the polling rate); this is not counted as stutter. Direction reversals and brief stops are not timed, as in the Stutter test. A Bluetooth mouse that delivers two or more reports at once, one connection event (7.5-15 ms) apart, is noted, and its rate, stutters and steadiness are judged per delivery; a USB mouse is never judged that way. A rate of 60-110 Hz is only noted: it is normal for a Bluetooth mouse (67-133 Hz), while a USB mouse should report 125 Hz or more. Under 60 Hz the pointer moves in visible steps on any connection and is a warning
   - **Button clicks** (up to 20 s) - point at the box shown and click: about 20 normal left clicks, then a few right clicks. Looks for switch bounce (accidental double clicks: one or two are a warning to re-run, three or more a fault, as in the Double-Click test), presses too short for a finger, sticky releases, buttons that never release (reported however few clicks were made)
   - **Scroll wheel** (up to 12 s) - about 10 notches down, then 10 up. Looks for notches reported in the wrong direction (encoder skipping)
   - **Lift-off** (10 s) - lift the mouse straight up about 1 cm and set it straight back down without sliding it, keep it still for a second, then move it in a small circle and stop; three times. Uses the rules of the Lift-Off Jump test: motion after a pause that carries the cursor more than 2 mm within 100 ms and then stops is a jump, judged by its shape as there (one is noted; two or more that no hand could make are a warning; two or more that could also be the hand make the step inconclusive); longer movements, and smooth movements of the hand, are not judged. Distances use the DPI set on the DPI Accuracy page; otherwise (and in the CLI) 800 DPI is assumed, the finding says so, and two or more jumps make the step inconclusive rather than a warning, because on a higher-DPI mouse they may be ordinary set-down shifts. If the mouse never paused and then moved, lift-off is reported as not tested
3. Read the verdict: **Healthy**, **Needs attention** (warnings), **Fault detected** (a critical finding), or **Inconclusive** (a step did not collect enough data, or a button or wheel direction registered nothing - if you did use it, it may be dead; if you skipped it, re-run and follow the instructions for the whole step, or the mouse's raw input stopped during a step: it was unplugged, its link dropped out, or another input was chosen). A run is only Healthy when every step completed. A run keeps the input it started with: if raw input stops or another device is chosen (or takes over after the mouse was unplugged), the step in progress is not judged and nothing else is measured in its place until the mouse is back (plugged into the same port again, or its wireless link reconnected); another unit of the same model is not taken for it
4. Each finding explains what was measured and what it usually means. Tick **Show raw measurements** for the numbers behind it
5. Press **Abort** (top right) or **Esc** to stop early (a partial report is still produced) or **Run Again** to repeat. During the click step only Esc stops the run, so stray clicks cannot end it, and the sidebar export buttons stay disabled until the run ends

Without raw input (FRAMEWORK input source) the movement and lift-off steps are left out, and switch bounce, very short presses and scroll skips are not judged, because window-system events are frame-rate limited and pointer movement is accelerated. A scroll direction that registered nothing is not named either: natural scrolling swaps the system's up and down. The report is marked "Limited check". Keep the pointer inside the window: clicks and scrolls outside it are not seen.

The report is included in the JSON/CSV export. If a warning appears once, re-run: a fault that repeats is real, a one-off is often the hand.

---

## Core Tests

### Polling Rate Monitor
Measures how often your mouse reports its position to the computer. Requires raw input ("RAW INPUT" on the Dashboard); with framework input the panel explains why it cannot measure instead of showing the display refresh rate as Hz.

1. Click **Start**
2. Move the mouse quickly and continuously, e.g. in circles
3. View real-time Hz readings and graph. "Nominal" is the nearest standard USB rate the measurement corresponds to. Under 110 Hz none is named: that is normal for a Bluetooth mouse (67-133 Hz), and the panel says so. Nor is one named for a reading more than 4% above a standard rate, which no USB mouse gives (the computer polls it on a fixed clock): about 133 Hz is a Bluetooth mouse on a 7.5 ms connection interval
4. Each **Start** begins a new measurement, so switching the polling-rate setting or the mouse between runs does not blend the readings. If the mouse's raw input stops (unplugged, link dropped, or another input chosen) the run ends and says so; it does not carry on with whatever input comes next

A Bluetooth mouse can deliver two or more reports at once, one connection event apart: the panel (and the CLI monitor) then says so and shows deliveries per second, which is how often the cursor moves, and names no USB rate.

A mouse only sends a report when it has moved at least one count, so slow movement and direction changes legitimately produce fewer reports. Readings are only taken while the mouse moves fast enough to report at every poll; otherwise "Current" shows "-" and asks you to move faster, and Min/Avg/Nominal are not affected. The CLI monitor (1) uses the same rule. At 4000-8000 Hz each report holds very few counts: move briskly or use 1600+ DPI.

**Common polling rates:** 125Hz, 500Hz, 1000Hz, 4000Hz, 8000Hz

---

### Stutter Detection
Detects reports that arrive late while the mouse is moving fast (choppy cursor movement). Requires raw input.

1. Click **Start**
2. Move the mouse quickly and continuously in circles until at least 3 s of fast movement has been judged
3. Spikes above the red threshold line indicate stutters

Only intervals during fast, steady movement are judged against the median interval; slow passes, direction reversals and pauses are ignored because a healthy mouse legitimately sends fewer reports then. Each interval is counted once. A stutter must also be at least 2.5 ms long, so normal sub-millisecond timing noise at 2-8 kHz is not flagged. A gap of 0.1-1 s between full-size reports, after which the held-back movement arrives in one report (the cursor freezes, then jumps), is a **freeze**: the connection dropped out while the hand kept moving. While circling, that report can point well away from the one before the gap (it carries the chord of the arc the hand moved through); a hand that stops and sets off again, whichever way, sends an ordinary report first. At 1 kHz and up, a gap of 0.1-1 s with full-speed reports on both sides is a freeze too, even though the movement in between was lost: a hand always slows down before it stops and speeds up after, which shows in the reports at those rates (at 125-500 Hz it can happen within a poll or two, so such gaps are not judged there). A mouse lifted while moving fast and set down still moving looks the same, so keep it on the pad. Two or more freezes make the rating at least Fair. The rating is **Inconclusive** until enough fast movement was recorded. Reports that arrive in batches (a Bluetooth mouse) are judged per delivery, and reports pinned at the largest value the mouse's report format holds are shown as clipping (movement lost at high speed), not as stutters. If the mouse's raw input stops, the run ends and says so.

At 125-250 Hz, a hand that stops for a moment (under 0.1 s) and sets off again the same way can still count as a stutter: keep moving. Requiring the held-back movement to arrive after every late report would stop that, but would also hide links that drop reports and their movement.

**Tip:** Adjust sensitivity slider (multiple of the median interval) - lower = more sensitive detection. The default is 2.5x, as in the CLI and Auto Diagnostics; a setting saved at the old default of 2.0x is moved to 2.5x.

---

### Click Response
Counts clicks and measures hold duration. Click latency (the delay inside the mouse) is not measured; that needs external hardware.

1. Select **Left Click** or **Right Click**
2. Click **Start**
3. Click inside the test area (only presses there count)
4. View clicks count, CPS (clicks per second), and hold times

Without raw input every click is still counted, even a press and release within one display frame, but press and release times are only known to the frame (about 16 ms): hold times are rounded to frames, the shortest hold is not shown, and the export says so. A run ends if raw input is lost or gained part way, so one run never mixes the two timings.

---

### Click Stickiness
Detects stuck or delayed button releases (common switch failure symptom).

1. Select **Left Click** or **Right Click**
2. Click **Start**
3. Click rapidly in the test area
4. Clicks held longer than 250 ms (the Auto Diagnostics threshold) are counted as long holds. After at least 5 clicks, two or more long holds are reported as slow or sticky releases; a single one is treated as the hand, not the switch. A button held down for over a second is reported as possibly stuck

---

### Lift-Off Jump
Detects cursor jumps when the mouse is lifted or set down. Needs raw input: with the FRAMEWORK input source the test does not run, because accelerated per-frame pointer motion cannot be measured in mm. If the mouse's raw input stops or another input is chosen during a run, the run ends and says so; a motion still under way at that moment is not judged (as in the CLI).

1. Set your mouse's DPI on the **DPI Accuracy** page, so jumps can be measured in mm (otherwise 800 DPI is assumed and the page says so)
2. Rest the mouse on the pad, click **Start** and keep the mouse still
3. Lift it straight up about 1 cm, hold it a moment, then set it straight back down without sliding it, and press **SPACE** with your other hand
4. Wait about a second and repeat at least 5 times, then press **Enter** or **Esc** (or click **Stop**: that moves the mouse, so motion in the last 0.6 s before the click is taken as the trip to the button and not judged)

Until the mouse has been still for 0.1 s after **Start**, the status reads "Waiting for the mouse to be still" and nothing is judged.

Motion in the first 100 ms after the mouse was still is summed; if it carries the cursor more than 2 mm, it is jump-sized. A movement that keeps going for more than another 2 mm is treated as sliding and not judged. A jump-sized movement is then judged by its shape, from its report times: one that started and stopped (or turned back) harder than about 3 g, which no hand does, is a **jump**; one that sped up and slowed down smoothly in one direction is the hand nudging the mouse and is not judged; anything in between is a **possible jump** - typically the mouse landing while it still slid sideways, a 125 Hz mouse whose reports are too far apart to show the shape, or a movement still under way when the test was stopped. One jump may be a hand slip; two or more mean a high lift-off distance (LOD), unless the DPI was assumed: then they may be ordinary set-down shifts of a higher-DPI mouse, and the result is **Possible jumps - DPI not set** (as in the CLI test and Auto Diagnostics). Possible jumps (with fewer than two jumps) ask you to set the mouse straight down and repeat; if they come back, the lift-off distance is likely too high. A clean lift moves nothing, so with no jumps and fewer than 5 lifts recorded the result is inconclusive. The CLI test and the Auto Diagnostics lift step use the same rules.

---

### USB Conflict Detection (CLI, Linux only)
Shows how each mouse is connected: its USB bus, the hubs in between, its link speed, and what else shares the bus.

1. Select **USB Conflict Scanner** from the CLI menu
2. The scanner reads `/sys/bus/usb/devices`; a mouse is recognised by its HID boot-mouse interface (Bluetooth, PS/2 and the few USB mice without the boot protocol are not listed)
3. Sharing a bus or controller with other devices is normal and is shown as information. Low speed (1.5 Mbit/s) is noted because it limits polling to about 125 Hz. A mouse behind a 12 Mbit/s (USB 1.1) hub together with an audio, video or storage device is shown as a possible issue: if the stutter test shows problems, plug the mouse straight into the computer and compare

---

### Scroll Wheel (GUI only)
Counts wheel notches and checks for encoder faults. This test is only available in the GUI application.

1. Click **Start**
2. Hover over the test area
3. Scroll steadily down at least 10 notches, then up at least 10
4. A notch reported in the opposite direction within 60 ms of the previous one is a skip (a finger cannot turn the wheel back that fast). One or two are reported as a possible skip, three or more as a skipping wheel. Many notches one way and none the other means that direction may be dead. Without raw input (always on macOS) up and down are as the system reports them, and natural scrolling swaps them, so the panel then does not say which direction is missing: turn the wheel both ways and see which one adds nothing. Skips are then not judged either, as in Auto Diagnostics: the system's wheel events only carry the display frame's time, too coarse to time a reversal. If raw input starts or stops during a run that has counted notches, the run ends and says so
5. Lost notches are not detected automatically: roll the wheel slowly by 10 clicks you can feel and check the counter rose by exactly 10

Scrolling speed is shown but not rated: it describes your hand, not the wheel.

---

## Advanced Tests

### DPI Accuracy
Verifies your mouse's actual DPI matches the configured setting. Requires raw input (the window-system pointer is scaled by OS acceleration).

1. Set your mouse DPI in its software
2. Enter the target DPI value (up to 64000)
3. Enter the distance you'll move, in inches or cm (use a ruler; longer is more accurate)
4. Put the mouse on the start mark and press **SPACE**
5. Move the mouse to the end mark
6. Press **SPACE** to finish

The result uses the straight-line distance between where the movement started and ended, so wobble or overshooting and coming back to the mark does not inflate it. If the mouse's raw input stops during a measurement (unplugged, link dropped, or another input chosen), the measurement is cancelled and the panel says so: counts from whatever input comes next are never added to it.

**95%+ accuracy** = Good DPI calibration. More than 25% off is shown as **Check setup**: almost always a different DPI stage, a wrong distance, or a movement that missed the marks.

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

With raw input this covers the mouse's **sensor/firmware** only: OS pointer acceleration (Windows "Enhance pointer precision", macOS or libinput acceleration) is applied after the raw counts, so a "no acceleration" result says nothing about that setting. Without raw input the test measures the on-screen pointer, which does include OS acceleration.

1. Pick a fixed physical distance (for example the full width of your mousepad, or between two marks)
2. Click **Start Test**
3. Move across that exact distance, pause, move back, pause - about 5 passes slowly
4. Do about 5 more passes quickly, then press **Enter** (or **Esc**) to stop without moving the mouse. **Stop Test** works too: the move to the button is not counted as a pass

Each pass must be at least 50 counts and the fast passes must be clearly faster than the slow ones, otherwise the result is **Inconclusive**. A movement that goes there and back without the pause is left out (the panel says how many). The slow and fast passes are compared by their medians, so one stray movement cannot decide the result.

**1.0x factor** = No acceleration (ideal). Above 1.15x is reported as acceleration; below 0.85x as fast passes losing counts, which is almost always the sensor losing tracking at speed (or the pointer hitting the screen edge), not an acceleration setting. The export records the verdict and whether raw counts or the on-screen pointer were measured. The result keeps describing the input the run used, and a run is stopped if raw input is lost or gained part way.

---

### Double-Click Test
Detects switch issues that cause accidental double-clicks.

1. Click **Start**, then click the large target repeatedly at a normal pace
2. A press closer than the threshold to the previous press, or within 25 ms of the previous release, is flagged as accidental (a finger cannot do either)
3. One or two accidental double-clicks are a possible fault (run the test again); three or more indicate a failing switch

Only presses on the target count. Switch bounce needs raw input: without it presses are only known to the display frame (about 16 ms), too coarse for these checks, so clicks and intervals are shown but the switch is not judged (as in Auto Diagnostics).

---

### Jitter Test
Measures sensor noise when the mouse is stationary.

1. Place mouse on pad
2. Click **Take Sample (5s)**, then let go of the mouse: recording starts 2 seconds later so the click and your hand leaving are not recorded
3. Wait 5 seconds without touching the mouse (a button press while recording discards the sample, as does raw input stopping or starting, for example when the mouse is unplugged). The sample keeps recording if you open another page meanwhile
4. Lower distance = less jitter = better sensor. Amounts are raw counts at the mouse's current DPI; a healthy sensor that nobody touches normally reports nothing

Without raw input no rating is given: only pointer movement in screen points is visible.

---

## Tips

- Close other applications for most accurate timing tests
- Use a consistent mousepad surface
- Let mouse warm up for a few minutes for best results

---

## Command-Line (CLI) Tests

The `mouse-testkit` binary (Linux and Windows) runs the same tests from a text menu. Timing comes from the device's own report timestamps, and click thresholds are the Auto Diagnostics ones, so the CLI, the GUI and the check-up agree. On Windows the tested mouse's clicks still reach the window under the pointer, so while a test runs the console's own mouse handling (QuickEdit selection, right-click paste) is switched off: clicking on the console window cannot paste text into the test or pause it. If the mouse disconnects during a CLI test (it is unplugged, its cable or receiver drops out, or a Bluetooth link is lost), the test stops at once and says so: the results cover only the input before that, a press or lift still open at that moment is not judged, and a clean result is reported as **Inconclusive** (a problem already found still stands); Auto Diagnostics is never Healthy then. Stutter, Jitter and Acceleration, which show measurements rather than a verdict, show what was measured before that, followed by the same note. If the computer falls behind reading the mouse and the system drops some of its reports, nothing is timed across the gap (it is not counted as a stutter, bounce or lift-off jump), a DPI or acceleration pass or a jitter sample in progress is set aside to be taken again (an angle-snapping line is dropped, a reaction-time trial repeated), and a note at the end of every test says how often it happened. Where a CLI test works differently from the GUI panel described above:

- **Click Reaction Time (4)** - after a random 1.5-4 s wait, `CLICK NOW!` appears; click as fast as you can. Clicking before the cue (or under 100 ms after it, which is a guess rather than a reaction) is a false start and the trial is repeated. The result is your reaction time plus display and input delay (typically 200-300 ms). It is not the mouse's own click latency, which is a few ms and cannot be separated from human reaction this way. Pressing `q` at the "press any key to start" prompt counts as not run
- **Click Stickiness (5)** - click normally about 20 times. A hold over 250 ms is a slow release; one long hold is treated as the hand, two or more as a late-releasing switch. Presses under 3 ms (contact bounce) and a button still held for over a second when you press `q` (stuck switch) are also reported. Fewer than 5 clicks is **Inconclusive**. The result lists the clicks each button registered and only covers those buttons: if only one registered, it says so (a button you clicked that is missing may be dead). Clicks made before the test started (for example while choosing the device) are not counted
- **Lift-Off Jump (6)** - enter the mouse's DPI (or press Enter to assume 800) and nudge the mouse when asked, which confirms the right device is being read (a clean lift sends nothing, so does a silent device). Then lift the mouse straight up about 1 cm and set it straight back down, 5-10 times with a pause in between, pressing **SPACE** after each lift. Motion in the first 100 ms of each lift or set-down is summed; if it carries the cursor more than 2 mm, it is a jump. A movement that keeps going for more than another 2 mm after that is treated as sliding and not judged. Jumps are judged by their shape as in the GUI panel: a smooth hand movement is not judged, and one that could also be the hand is only a possible jump (**Possible issue** - set the mouse straight down and repeat). One jump may be a hand slip; two or more that no hand could make mean a high lift-off distance, unless the DPI was assumed: then they may be ordinary set-down shifts of a higher-DPI mouse and the result is **Inconclusive** (as in Auto Diagnostics and the GUI panel). A clean lift produces no motion at all, so with no jumps and fewer than 5 lifts recorded the result is **Inconclusive**; with no movement at all the test asks you to nudge the mouse again to confirm it is still connected
- **DPI Accuracy (7)** - enter the configured DPI (`1600`, `1,600`, `1.6k` and `1,6k` all work; press Enter to just measure the DPI without judging it) and the distance you will move (for example `10cm` or `4in`; default 10 cm). Line up one edge of the mouse with the start mark of a ruler, press **SPACE**, slide it until the same edge is on the end mark in one movement (any direction), press **SPACE** again. The straight-line distance in counts gives the DPI. Take 3 or more samples: the verdict uses their median, so one sample spoiled by lifting the mouse does not decide it. A longer distance reduces the effect of hand placement; if the deviation could be explained by placing the mouse 1 mm off at each end, the result is **Inconclusive**. As in the GUI, a result more than 25% off is **Inconclusive** too (check the setup: almost always another DPI stage than entered, a wrong distance or a missed mark), not a sensor fault
- **Angle Snapping (8)** - draw freehand lines that are *almost* horizontal or vertical (tilted about 5°), a few centimetres long, pausing between them; press `q` when done. Uses the same line analysis as the GUI. Diagonal lines cannot show snapping, so fewer than 3 near-axis lines is **Inconclusive**
- **Double-Click (10)** - click normally about 20 times per button. A press within 50 ms of the previous press, or within 25 ms of the previous release, of the same button is an accidental double-click. 1-2 are a possible fault (re-run), 3 or more a failing switch. As in test 5, the result lists the clicks per button and notes when only one button registered
- **Jitter (11)** - nudge the mouse when asked (this confirms the device is live), let go, then press SPACE. Recording starts 2 s later, as in the GUI. A sample whose movement is confined to one or two seconds (a bump of the desk, the hand leaving late) is set aside and taken again rather than rated as sensor noise, as in Auto Diagnostics
- **Run All Standard Tests (12)** - before each test you can press Enter to run it, `s` to skip it or `q` to stop the suite. The summary shows what each test found (problem, possible problem, no problem, inconclusive, not run); tests that were skipped or had no mouse are never reported as completed

---

## About

Click the **About** button in the sidebar to view version info.

Licensed under MIT License.
