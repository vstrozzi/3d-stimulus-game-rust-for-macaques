# Timing and Precision: Implementation Notes

This document describes the timing architecture of the monkey\_3d\_game system,
the precision guarantees of each measurement, known latency sources, and
the assumptions required to achieve sub-1 ms accuracy in a psychophysical
experiment.

---

## 1. System Architecture Overview

```
┌─────────────────────────┐       ┌───────────────────────────┐
│       Controller        │       │        Game (Bevy)        │
│  (Python native / JS)   │       │                           │
│                         │       │  FixedUpdate (60 Hz)      │
│  Writes commands ──────────────▶│    game logic, physics    │
│  Reads game state  ◀────────────│    elapsed_secs           │
│  Logs trial data        │       │    frame_number           │
│                         │  SHM  │                           │
│                         │       │  Update (vsync-locked)    │
│                         │       │    rendering              │
│                         │       │    render_elapsed_secs    │
│                         │       │    render_frame_number    │
│                         │       │    photodiode_white       │
└─────────────────────────┘       └───────────────────────────┘
                                           │
                                    GPU pipeline
                                           │
                                    Compositor
                                           │
                                    Display scanout
                                           │
                                    ▼ Photons hit retina
```

Communication between controller and game happens through a shared memory
region (`SharedMemory` struct, `repr(C)`, all fields atomic).

---

## 2. Two Clock Domains

The game has two distinct update loops, each with its own timestamp and
frame counter:

| Field                  | Update schedule      | What it measures                         |
|------------------------|----------------------|------------------------------------------|
| `frame_number`         | **FixedUpdate** (60 Hz) | Game-logic tick counter                  |
| `elapsed_secs`         | **FixedUpdate** (60 Hz) | Cumulative game-logic time since round start |
| `render_frame_number`  | **Update** (vsync)   | Render frame counter                     |
| `render_elapsed_secs`  | **Update** (vsync)   | Time at which the frame was submitted for rendering |
| `photodiode_white`     | **Update** (vsync)   | State of the photodiode calibration square |

### FixedUpdate (game logic)

- Configured at exactly 60 Hz via `Time::<Fixed>::from_hz(60.0)`.
- Bevy guarantees this runs at a fixed timestep: each tick advances
  `elapsed_secs` by exactly 1/60 s = 16.667 ms.
- If the render loop is slower than 60 fps, Bevy runs multiple FixedUpdate
  ticks per render frame to catch up. If faster, some render frames have
  zero FixedUpdate ticks.
- `frame_number` increments by exactly 1 per FixedUpdate tick.
- **Precision**: deterministic, no jitter. The 16.667 ms step is exact
  in floating-point arithmetic.

### Update (render loop)

- Runs once per vertical sync (vsync) when `PresentMode::Fifo` is active.
- On native: driven by the OS compositor's vsync signal.
- On WASM: driven by `requestAnimationFrame`, which the browser fires at
  the display's refresh rate (typically 60 Hz).
- `render_frame_number` increments by 1 per Update tick.
- `render_elapsed_secs` samples the same round-start clock as
  `elapsed_secs`, but at Update time (not FixedUpdate time).
- **Precision**: subject to OS/browser scheduling jitter, typically ±1 ms
  on a well-configured system, occasionally worse under load.

---

## 3. Latency Pipeline: From Game State to Photon

When the game decides to display a stimulus, the following latency stages
occur before the participant sees it:

```
 Game logic          Render submit       GPU work          Compositor         Scanout
 (FixedUpdate)       (Update)            (GPU pipeline)    (OS/browser)       (LCD/OLED)
     │                   │                    │                 │                  │
     ├── 0–16.7 ms ──▶  ├── <1 ms ────────▶  ├── 0.5–8 ms ──▶ ├── 0–16.7 ms ──▶ │
     │                   │                    │                 │                  │
     t_logic         t_render_submit      t_gpu_done       t_compositor      t_photon
```

### Stage 1: FixedUpdate → Update (0–16.7 ms)

A game state change in FixedUpdate is not rendered until the next Update
tick. In the best case they coincide (0 ms); in the worst case the state
change happened just after an Update tick and must wait a full vsync
period.

- **Measured by**: comparing `elapsed_secs` (when the state changed) with
  `render_elapsed_secs` (when it was rendered).
- **Logged**: yes, both are in the trial log per frame.

### Stage 2: CPU render submission (<1 ms)

Bevy's render stage runs within the Update tick. For this game (simple
scene, no heavy shaders), CPU-side render submission takes well under 1 ms.

- **Measured by**: not directly logged. Could be measured via Bevy
  diagnostics but typically negligible for this workload.

### Stage 3: GPU pipeline (0.5–8 ms)

The GPU processes the command buffer. With `PresentMode::Fifo` and
double-buffering, the GPU must finish before the next vsync.

- **Measured by**: not directly observable from application code.
  Vulkan's `VK_GOOGLE_display_timing` extension can report this, but
  Bevy does not expose it. In practice, if no frames are dropped
  (render_frame_number has no gaps), the GPU finished in time.
- **Logged**: indirectly — a dropped frame appears as a
  `render_frame_number` gap.

### Stage 4: Compositor (0–16.7 ms)

The compositor decides which buffer to display at the next vsync. With
`PresentMode::Fifo`, the frame enters a FIFO queue. Typical Fifo
implementations use double-buffering (2 buffers), meaning:

- If the game submits before the vsync deadline, the frame displays at the
  **next** vsync: ~0 ms compositor delay.
- The FIFO queue can introduce 1 additional vsync of latency if the queue
  is full (triple-buffering behavior).

On Linux/Wayland with a simple compositor, expect 1 vsync of latency.
On Windows DWM, expect 1–2 vsyncs.

- **Measured by**: not observable from application code. The photodiode
  provides ground truth.

### Stage 5: Display scanout (0–16.7 ms for LCD, <1 ms for OLED)

The display scans out the frame from top to bottom. An element at the top
of the screen appears ~0 ms after vsync; at the bottom, up to 1 full
frame period later.

- The photodiode square is positioned at the **top-right corner**
  (0 px from top), so scanout delay is minimal (~0.1 ms for top rows).
- **Measured by**: the physical photodiode sensor placed on this corner.

### Total end-to-end latency

| Component              | Typical (ms) | Worst case (ms) |
|------------------------|-------------|-----------------|
| FixedUpdate → Update   | 0–16.7      | 16.7            |
| CPU render submission  | <1          | <1              |
| GPU pipeline           | 0.5–3       | 8               |
| Compositor (Fifo)      | 0–16.7      | 33.4            |
| Display scanout (top)  | ~0.1        | ~0.5            |
| **Total**              | **~1–37**   | **~60**         |

The critical insight: this latency is **nearly constant** for a given
hardware setup and vsync phase. It varies by at most ±1 vsync period
(±16.7 ms) due to phase alignment, but does not drift over time.

---

## 4. What We Log and What We Can Derive

### Per-frame trial log fields

| Field                 | Source     | Precision            | Notes                              |
|-----------------------|-----------|----------------------|------------------------------------|
| `frame_number`        | FixedUpdate | Exact (integer)     | Game logic tick, resets per trial   |
| `render_frame_number` | Update     | Exact (integer)      | Render frame, resets per trial      |
| `elapsed_secs`        | FixedUpdate | Exact (deterministic) | 16.667 ms steps, no jitter        |
| `render_elapsed_secs` | Update     | ~0.1 ms (native), ~5–100 μs (WASM†) | Real wall-clock at render submit |
| `photodiode_white`    | Update     | Exact (boolean)      | Game's intended photodiode state   |
| `is_animating`        | FixedUpdate | Exact (boolean)     | Whether door animation is running  |

† WASM timing precision depends on browser security headers (see Section 6).

### Derivable timing information

1. **Game-logic frame delta**: `elapsed_secs[n] - elapsed_secs[n-1]` =
   exactly 16.667 ms (by construction).

2. **Render-frame delta**: `render_elapsed_secs[n] - render_elapsed_secs[n-1]` =
   actual time between consecutive render submissions. Should be ~16.667 ms
   at 60 Hz vsync; deviations indicate jitter or dropped frames.

3. **Logic-to-render delay**: `render_elapsed_secs - elapsed_secs` for the
   same frame tells you how long the game state waited before being
   rendered. This is Stage 1 above.

4. **Photodiode transition log**: the sequence of `photodiode_white` values
   produces a predictable alternating pattern (W, B, W, B, ...). Each
   transition is a calibration event that can be matched to the physical
   photodiode signal.

5. **Dropped render frames**: if `render_frame_number` jumps by >1 between
   consecutive log entries, a render frame was missed. The trial log does
   not contain data for the missed frame (it was never rendered).

---

## 5. Photodiode Calibration Procedure

### Purpose

Determine the constant offset `T_offset` between the game's
`render_elapsed_secs` and actual photon emission, for the current
hardware setup.

### Setup

1. Place a photodiode sensor on the top-right corner of the display,
   covering the 50×50 px calibration square.
2. Connect the photodiode to a DAQ, oscilloscope, or timing board
   (e.g., Black Box Toolkit, Arduino with μs-resolution timer).
3. Press **B** in the game to enable the photodiode square.

### Calibration protocol

1. Let the game run for several seconds with the photodiode enabled.
2. The square alternates white/black every render frame (every ~16.7 ms
   at 60 Hz).
3. Record both:
   - The photodiode sensor's voltage transitions (rising = black→white,
     falling = white→black) with hardware timestamps `T_sensor[i]`.
   - The game's trial log with `render_elapsed_secs[i]` and
     `photodiode_white[i]` for each render frame.
4. Align the two sequences by matching transitions:
   - Find the first W→B transition in the trial log at
     `render_elapsed_secs = T_game`.
   - Find the corresponding falling edge in the sensor signal at
     `T_sensor`.
   - `T_offset = T_sensor - T_game`.
5. Verify across multiple transitions — `T_offset` should be constant
   (within ±0.5 ms on good hardware).

### Applying the calibration

For any stimulus event at `render_elapsed_secs = T`, the actual display
time is:

```
T_display = T + T_offset
```

This offset includes GPU pipeline + compositor + scanout latency and is
stable for the duration of a session (unless vsync mode changes or the
system enters thermal throttling).

### Expected `T_offset` values

| Setup                         | Typical T_offset |
|-------------------------------|------------------|
| Native Linux, simple compositor, LCD | 16–25 ms  |
| Native Linux, OLED                   | 8–18 ms   |
| Native Windows (DWM)                 | 25–40 ms  |
| WASM in Chromium                     | 20–35 ms  |

---

## 6. Native vs WASM Differences

| Aspect                    | Native (Linux)                    | WASM (browser)                          |
|---------------------------|-----------------------------------|-----------------------------------------|
| Update loop driver        | OS compositor vsync               | `requestAnimationFrame` (browser)       |
| Clock precision           | `Instant::now()` ≈ ns resolution  | `performance.now()` ≈ 5 μs–100 μs†     |
| Vsync reliability         | High (Fifo + compositor)          | Browser may throttle (background tabs, power saving) |
| Dropped frames observable | Yes (render_frame_number gaps)    | Yes, but browser may skip rAF silently  |
| GC / JIT pauses           | None                              | Possible; introduces rare >1 ms spikes  |
| Thread model               | Bevy on main thread, controller separate process | Same thread via rAF interleaving |
| Timing of controller poll | Independent process, true 60 Hz   | Interleaved with render; timing coupled |

† **WASM clock precision**: by default, browsers degrade `performance.now()`
to ~100 μs resolution as a Spectre mitigation. To restore ~5 μs precision,
serve the page with these HTTP headers:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

Without these headers, `render_elapsed_secs` in WASM has ~100 μs
granularity, which is sufficient for 1 ms accuracy but provides no margin.

### Recommendation

**Use native builds for timing-critical experimental sessions.** WASM is
suitable for demos, training, and piloting, but native provides tighter
vsync locking, higher clock precision, and no GC/JIT interference.

---

## 7. Ring Buffer and Polling

### Ring buffer (FrameRingBuffer)

- 8 slots of full `SharedGameState` structs.
- The game writes one entry per FixedUpdate tick (60 Hz), advancing
  `write_head` monotonically.
- The controller/monitor drains unseen entries via `read_game_state_since()`.
- If the consumer falls behind by >8 ticks (~133 ms), oldest entries are
  overwritten. This is detectable: `current_head - last_head > 8`.

### Monitor polling

- The monitor (`monitor.py`) polls SHM at a configurable rate (default
  60 Hz, `--hz` flag).
- **Ring buffer drain** (`read_game_state_since`): gives exact per-tick
  `elapsed_secs` and `frame_number`. No frames are missed as long as the
  monitor doesn't fall behind by >8 ticks.
- **Snapshot read** (`read_game_state`): gives the latest
  `render_frame_number`, `render_elapsed_secs`, `photodiode_white`. These
  are **sampled**, not drained — if two render frames occur between two
  monitor polls, the intermediate state is lost in the monitor display
  (but still logged in the trial log by the controller).

### What the monitor cannot capture

1. **Actual display onset** — only the physical photodiode can measure this.
2. **Compositor frame drops** — if the OS shows the same buffer twice, the
   game doesn't know.
3. **Inter-poll render frames** — `r_dt` in the monitor is computed from
   snapshots; it may alias if the poll and vsync are not phase-locked.
4. **Sub-frame timing** — the monitor shows one row per poll tick. Events
   that occur between polls are only visible in the trial log.

---

## 8. Assumptions for Sub-1 ms Accuracy

To claim sub-1 ms accuracy on stimulus onset timing, **all** of the
following must hold:

### Hardware assumptions

- [ ] Display runs at a stable, known refresh rate (e.g., 60.000 Hz ± 0.01 Hz).
- [ ] `PresentMode::Fifo` is active (not `AutoNoVsync`). Verify with `V` key toggle.
- [ ] No thermal throttling causing GPU frequency scaling during the session.
- [ ] Physical photodiode sensor has ≤0.1 ms response time.
- [ ] DAQ/timing hardware has ≤0.1 ms timestamp resolution.
- [ ] Photodiode is placed on the **top** edge of the display to minimize
      scanout delay (~0.1 ms for top rows vs ~16 ms for bottom rows on LCD).

### Software assumptions

- [ ] Native build (not WASM) for timing-critical sessions.
- [ ] No other GPU-intensive applications running (avoids compositor contention).
- [ ] `render_frame_number` shows no gaps during the experimental block
      (no dropped frames). Any trial with a gap should be flagged.
- [ ] Photodiode calibration performed at session start and verified at
      session end (drift check).
- [ ] `T_offset` standard deviation across calibration transitions is <0.5 ms.
      If not, the setup is too jittery for sub-1 ms claims.
- [ ] Controller does not write to SHM fields marked `READ_ONLY_FIELDS`
      (`render_frame_number`, `render_elapsed_secs`, `photodiode_white`).

### Analysis assumptions

- [ ] Stimulus onset time is computed as `render_elapsed_secs + T_offset`,
      not `elapsed_secs` (which is the game-logic timestamp, potentially
      up to 16.7 ms earlier).
- [ ] Trials with `render_frame_number` gaps, anomalous `r_dt` (>2× or
      <0.5× expected), or `T_offset` drift >1 ms are excluded from analysis.
- [ ] The alternating photodiode pattern (W/B/W/B) is used for alignment,
      not absolute color — this makes the calibration robust to one-off
      misreads.

---

## 9. Checklist: Before Running an Experiment

1. Build game native: `cargo build --release -p game_node`
2. Build shared lib: `cargo build --release -p shared --features python`
3. Copy `.so`: `cp target/release/libshared.so controller_python/monkey_shared.so`
4. Start game, attach controller, start monitor:
   `python controller_python/monitor.py --hz 60`
5. Verify monitor shows:
   - `g_dt` = 16.67 consistently (no jitter in game logic)
   - `r_dt` ≈ 16.67 (render frames arriving on schedule)
   - `gaps = 0`, `dups = 0`, `frz = 0`
   - `out% = 0.0%`
6. Press **B** to enable photodiode. Verify `pdi` alternates W/B.
7. Run photodiode calibration (see Section 5). Record `T_offset`.
8. Verify `T_offset` std dev < 0.5 ms across ≥100 transitions.
9. Run experimental block.
10. At session end, re-run calibration. Verify `T_offset` has not drifted
    by more than 0.5 ms from session start.

---

## 10. Summary of Timing Fields

```
Timeline:     ──── FixedUpdate ticks ────────────────────────────────
                   │    │    │    │    │    │    │
                   ▼    ▼    ▼    ▼    ▼    ▼    ▼
  frame_number:    1    2    3    4    5    6    7    (game logic)
  elapsed_secs:  0.000 .017 .033 .050 .067 .083 .100 (deterministic)

Timeline:     ──── Update ticks (vsync) ─────────────────────────────
                  │         │         │         │
                  ▼         ▼         ▼         ▼
  render_frame:   1         2         3         4    (render)
  render_elapsed: 0.001     .018      .034      .051 (real clock, ±jitter)
  photodiode:     W         B         W         B    (alternating)

  T_offset:    ◄──────────────────────────────────▶  (constant, measured
               render_elapsed_secs → actual photon    via photodiode)
```

The combination of `render_elapsed_secs` (game-side render timestamp) +
`T_offset` (photodiode-calibrated constant) gives you the actual display
onset time with sub-1 ms accuracy, provided the assumptions in Section 8
are met.
