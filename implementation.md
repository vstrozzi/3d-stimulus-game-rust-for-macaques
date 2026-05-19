# Monkey 3D Game — Implementation & Resume Guide

This document is the canonical reference for resuming work on this codebase
from a clean session. Sections 1–6 are an architectural map; sections 7–13
are the original timing/precision notes. Sections 14–15 record recent
changes and known open issues so the next session does not re-discover
them.

---

## 1. Repo Layout

```
monkey_3d_game/
├── Cargo.toml                  workspace root (members: shared, game_node)
├── shared/                     no_std-friendly crate: SHM struct, constants,
│   └── src/
│       ├── lib.rs              SharedMemory + SharedGameState (repr(C), atomics)
│       ├── constants.rs        defaults for every SHM field
│       ├── native.rs           mmap-backed native binding
│       ├── python.rs           PyO3 bindings → controller_python/monkey_shared.so
│       └── web.rs              wasm-bindgen WebSharedMemory (offsets, defaults)
├── game_node/                  Bevy game (native + WASM)
│   └── src/
│       ├── lib.rs              plugin wiring, resource registration
│       ├── main.rs             native entry
│       ├── shared_memory/
│       │   ├── shared_memory_reader.rs    SHM init, command reads, ring-buffer
│       │   ├── shared_memory_writer.rs    state writes, frame counters
│       │   └── shared_memory_web_extension.rs   WASM JS-bridge glue
│       └── utils/
│           ├── setup.rs            setup_environment + setup_round
│           ├── pyramid.rs          pyramid geometry + decoration placement
│           ├── decorations.rs      shape meshes + per-decoration rotation
│           ├── load_textures.rs    PBR texture set loading + check_scene_ready
│           ├── warmup.rs           sub-pixel warmup scene (pipeline pre-compile)
│           ├── handle_commands.rs  reset / animation / rotation / blank / stop
│           ├── game_functions.rs   door animation, score bar
│           ├── camera.rs           persistent camera
│           ├── ui.rs               UI scaling
│           ├── debug_functions.rs  photodiode marker, debug overlays
│           ├── objects.rs          shared components & resources
│           └── systems_logic.rs    plugin schedule wiring (Startup/FixedUpdate/Update)
├── controller_python/
│   ├── controller.py           native controller (FSM driving the experiment)
│   ├── monitor.py              live SHM monitor (prints g_dt / r_dt / gaps)
│   └── monkey_shared.so        built from `shared` with --features python
├── controller_main.js          web controller (mirrors controller.py)
├── index.html                  landing page (session-name input)
├── game.html                   game page (loads WASM + vendored JSZip)
├── vendor/jszip.min.js         vendored ZIP lib (CDN with SRI failed)
├── trials_config/
│   └── trial_editor.html       browser-based trial JSON editor
├── tools/
│   └── verify_trial_logs.py    drift / gap / FPS verifier + session overview (native + web ZIPs)
└── assets/                     PBR textures (color, normal, mr, occlusion, depth)
```

---

## 2. Build & Run

### Native game
```
cargo build --release -p game_node
./target/release/game_node
```

### Shared library for Python controller
```
cargo build --release -p shared --features python
cp target/release/libshared.so controller_python/monkey_shared.so
```

### Controller (native)
```
python controller_python/controller.py <trials_config.json>
python controller_python/monitor.py --hz 60      # live SHM dashboard
```

### WASM build
```
wasm-pack build game_node --target web --release --out-dir ../out
```
Serve the repo root over HTTP with COOP/COEP headers (see §6). The page
needs `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` for `SharedArrayBuffer`.

### Verifier
```
python tools/verify_trial_logs.py path/to/session.zip
python tools/verify_trial_logs.py path/to/native_logs/
```
Accepts zips, directories, and individual JSONs; auto-detects web vs.
native. Emits per-trial PNGs, a `session_overview.png` (or one per
platform when comparing native + wasm), and `summary.txt`.

---

## 3. Shared Memory Contract

`shared/src/lib.rs` defines `SharedMemory`, which is `repr(C)` with all
fields as atomics. It has three sub-structs:

- `game_structure_game` (`SharedGameState`) — written by the game,
  read by the controller. Snapshot of the latest state.
- `game_structure_control` (`SharedGameState`) — written by the controller
  to push the next trial config; read by the game on reset.
- `frame_ring_buffer` — 8-slot ring of full `SharedGameState` entries the
  game pushes every `FixedUpdate` so the controller can drain frames it
  missed between polls. Overflow at >8 ticks behind is detectable.

Fields are encoded as `AtomicU32`/`AtomicU64`/`AtomicBool`; floats are
stored as `f32::to_bits()` and re-read with `f32::from_bits()`.

### Notable fields
| Field | Owner | Notes |
|---|---|---|
| `frame_number` | game (FixedUpdate) | Game-logic tick. **Paused when `stop_rendering`**. Resets to 0 on reset. |
| `elapsed_secs` | game (FixedUpdate) | Game-logic time. Paused when `stop_rendering`. Resets to 0 on reset. |
| `render_frame_number` | game (Update→First) | Vsync-aligned frame count. **Always increments** regardless of `stop_rendering`. Committed in next frame's `First`. |
| `render_elapsed_secs` | game (Update→First) | Game-logic clock (same domain as `elapsed_secs`) sampled when the frame was submitted in `Update`. Committed in next frame's `First`. |
| `present_elapsed_secs` | game (`First`) | **Real wall-clock seconds since app start**, sampled at the start of the *next* frame after this one was rendered. Because Bevy's main loop is back-pressured by `present()` under `PresentMode::Fifo`, this stamp ≈ vsync time at which the frame was latched by the compositor. Per-frame deltas reconstruct the actual frame interval. The cross-platform analogue of `Screen('Flip')`'s return value. |
| `photodiode_white` | game (Update→First) | Photodiode square visible+white when the frame was rendered. Committed in next frame's `First`. |
| `is_animating` | game | Set during door animation; verifier uses this to exclude pauses from drift. |
| `is_rendering_stopped` | game | Mirror of `GameConditions.stop_rendering`. |
| `is_scene_ready` | game | Set true once textures + warmup complete. |
| `is_blank` | game | True while `BlankScreen` entity exists. |
| `decorations_rotation` | controller | `[i32; 3]` per face; `-1` = random (seeded), else degrees. |
| `camera_rotation_sense` | controller | `i32 ∈ {-1, +1}`; multiplies rotation speed. |
| `target_door` | controller | Index of the door to align to. |
| Door / pyramid / texture indices | controller | See `shared/src/lib.rs:153+`. |

`READ_ONLY_FIELDS` (`shared/src/lib.rs`) lists fields the controller must
not write — render-side timestamps, photodiode state.

### Decoration shapes
`DecorationShape` enum (`shared/src/lib.rs:85`) is **append-only**:
`Triangle, Square, Circle, Diamond, Star, Cross, Rectangle, Oval,
Pentagon, Kite, Rhombus, Trapezoid, Semicircle`. All meshes are built via
`fan_mesh` in `game_node/src/utils/decorations.rs`. Per-decoration random
rotations are seeded with `GRID_RANDOM_ROTATION_SEED = 0xDEC0_DEAD`
(`ChaCha8Rng`).

---

## 4. Game-Side Architecture (Bevy)

### Plugins
- `SystemsLogicPlugin` (`game_node/src/utils/systems_logic.rs`) wires
  every game system.
- `StateEmitterPlugin` (`game_node/src/shared_memory/shared_memory_writer.rs`)
  registers `FrameCounterResource` and `RenderFrameCounterResource`.

### Schedules
```
Startup:        init_shared_memory_system
                spawn_persistent_camera
                setup_environment
                preload_all_textures
                spawn_warmup_scene           (chain order matters)

FixedPreUpdate: read_shared_memory_commands
                read_shared_memory_game_state_local

FixedUpdate:    handle_reset_command
                handle_check_alignment
                handle_blank_screen
                handle_stop_rendering
                handle_rotation
                handle_zoom
                handle_door_animation
                handle_animation_door_command
                update_score_bar

FixedPostUpdate: clear_pending_commands
                 increment_timing            (paused if stop_rendering)
                 update_shared_memory_local
                 write_shared_memory_game_state

First:          commit_render_sample         (writes prev frame's staged
                                              sample to SHM, stamped with
                                              real-wallclock `now` —
                                              ≈ vsync of the prior flip)

Update:         update_ui_scale
                tick_warmup                  (despawns warmup scene + flips WarmupState.complete)
                check_scene_ready            (gated on WarmupState.complete)
                stage_render_sample          (samples + stages; no SHM write)
```

### Why staging matters (the `Screen('Flip')` analogue)

For psychophysics we want the per-frame timestamp to mean "when did this
frame's photons appear on screen", not "when did the game submit this
frame for rendering". The two differ by the GPU + compositor + scanout
pipeline (~1–37 ms, dominated by one vsync of compositor latency under
Fifo).

Under `PresentMode::Fifo`, `surface.present()` queues the rendered image
for the next vsync. The main thread blocks until a fresh swapchain image
becomes available — which happens *at that vsync*, the moment the
compositor latches the previous frame. So sampling `Instant::now()` at
the very top of the following frame's `First` schedule yields a
timestamp ≈ vsync-of-previous-flip. This is the closest software-only
analogue to Psychtoolbox's `Screen('Flip')` return value.

The implementation:

1. **`stage_render_sample`** runs in `Update` of frame *N*. It increments
   `RenderFrameCounterResource`, samples the photodiode state and
   `round_start`-relative submit time, and stashes everything in the
   `StagedRenderSample` resource. It does **not** write to SHM.
2. The frame is rendered and presented at end of frame *N*.
3. **`commit_render_sample`** runs in `First` of frame *N+1*. It samples
   `Time<Real>::elapsed_secs()` — wall-clock seconds since app start —
   and atomically writes all four fields (`render_frame_number`,
   `render_elapsed_secs`, `present_elapsed_secs`, `photodiode_white`)
   into SHM. The row now consistently describes frame *N* with a
   `present_elapsed_secs` matching frame *N*'s actual flip.

The `FixedPreUpdate` ↔ `FixedPostUpdate` round-trip preserves these
fields through the fixed loop (local reads SHM in PreUpdate, writes back
in PostUpdate).

Residual error: ~0–1 ms on native (Fifo back-pressure jitter), ~0–1 ms
on web (rAF callback scheduling). Both are constant offsets to the true
photon time that the photodiode absorbs during calibration.

> Note: on web the `requestAnimationFrame` argument is microseconds away
> from `Time<Real>::elapsed()` measured at `First`, because winit-wasm
> drives Bevy's main loop *from inside* the rAF callback. A single
> `First`-schedule sample therefore covers both platforms cleanly — no
> separate JS bridge needed.

### Frame counters and round_start lifecycle

| Counter | Where it ticks | Pauses on `stop_rendering`? | Resets on `handle_reset_command`? |
|---|---|---|---|
| `FrameCounterResource` (`frame_number`) | `FixedPostUpdate::increment_timing` | **Yes** (early return) | Yes (= 0) |
| `RenderFrameCounterResource` (`render_frame_number`) | `Update::increment_render_frame_counter` | No | Yes (= 0) |
| `RoundStartTimestamp` (→ `elapsed_secs`) | `FixedPostUpdate::increment_timing` (accumulates `time.delta()`) | Yes | Reset to `Some(Duration::ZERO)` in `setup_round` |

Net result: on the first visible frame of a trial, `frame_number = 0`
and `elapsed_secs ≈ 0`. `render_frame_number` ticks throughout to keep
the warmup loop alive even if the controller toggles `stop_rendering`
early (but see §15 — warmup currently reads the *paused* counter).

### Warmup scene
`game_node/src/utils/warmup.rs` spawns a sub-pixel (scale 0.001) scene
containing one instance of every `StandardMaterial` variant the game can
use, plus a representative mesh, at startup. This forces texture upload
+ pipeline compilation before trial 0 begins, eliminating the
~1.3 s stall that used to appear at the first PLAYING state.
`tick_warmup` despawns these entities once
`WARMUP_FRAMES_AFTER_LOAD = 20` frames have passed after every texture
is fully loaded, then sets `WarmupState.complete = true`.
`check_scene_ready` (`load_textures.rs`) gates `is_scene_ready` on both
`all_loaded` AND `WarmupState.complete`.

---

## 5. Controller FSM

Both `controller_python/controller.py` and `controller_main.js`
implement the same state machine:

```
INIT → WAITING_FOR_START → PLAYING → ANIMATION_DOOR → COOLDOWN → INIT (next trial)
                                  ↘ TIMEOUT_ANIMATION ↗
```

Key invariants and recent fixes:

- **R-press during WAITING_FOR_START does NOT issue `reset`.** It only
  flips `stop_rendering=false`. Reset already happened in `handle_init`.
  (Previously issuing reset again caused a full pyramid despawn/respawn
  burst on every trial start, ~1.3 s GC pause.)
- **Frame-tracking re-sync** at the end of `handle_init`:
  - Python: `self.current_frame = -1; self.last_write_head = self.shm_wrapper.frame_write_head()`
  - JS: `currentFrame = -1; lastWriteHead = readU64(headView, 0)`
- **`logFrame` keys by `state.frame_number`**, not the controller's
  local `currentFrame`. Keying by the latter caused intermediate ticks
  to overwrite each other and produced fake "gaps" in the verifier.
- **Intermediate-frame logging** is gated on the FSM being in
  `PLAYING` / animation states. Logging in `WAITING_FOR_START` produced
  the "two timelines" artefact in plots because frames from a stale
  trial mixed with the new one.

### Logging schema
Per-trial JSON containing one entry per game frame seen by the
controller. Fields include `frame_number`, `render_frame_number`,
`present_elapsed_secs`, `photodiode_white`, `is_animating`, plus
state copies of the control fields. `elapsed_secs` /
`render_elapsed_secs` are SHM-only (used by FSM, not logged). Web logs are bundled into a session ZIP grouped by
level; filename = `{session_name}_{YYYY-MM-DD_HH-MM-SS}.zip`. Session
name is captured on the landing page (`index.html`) and persisted in
`localStorage.session_name`; empty name → `unknown`.

---

## 6. Trial Editor (`trials_config/trial_editor.html`)

Browser editor for trial JSONs. Notes:

- `defaultObject` includes `decorations_rotation: [0, 0, 0]` and the
  `_type` field is **preserved on export** (previously stripped, which
  caused custom objects with door indices 2 or 5 to be reclassified on
  reload).
- Imports backfill `decorations_rotation` and `camera_rotation_sense`
  via an `Array.isArray` check so old trial files still load.
- Per-face rotation widget supports a "Rand" toggle (sets the value to
  `-1`) and a degrees input. `applyRotation` propagates the value to
  any face that shares a constraint group.
- `camera_rotation_sense` is a fixed-group `<select>` with `+1`/`-1`.

---

## 7. System Architecture Overview

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
                                    GPU pipeline → Compositor → Scanout → Photons
```

---

## 8. Two Clock Domains

| Field                  | Update schedule         | What it measures                         |
|------------------------|-------------------------|------------------------------------------|
| `frame_number`         | **FixedUpdate** (60 Hz) | Game-logic tick counter (paused if `stop_rendering`) |
| `elapsed_secs`         | **FixedUpdate** (60 Hz) | Cumulative game-logic time since round start (paused if `stop_rendering`) |
| `render_frame_number`  | **Update** (vsync)      | Render frame counter (never paused)      |
| `render_elapsed_secs`  | **Update** (vsync)      | Real-clock time at frame submission      |
| `photodiode_white`     | **Update** (vsync)      | State of photodiode calibration square   |

### FixedUpdate (game logic)
- Configured at exactly 60 Hz via `Time::<Fixed>::from_hz(60.0)`.
- Each tick advances `elapsed_secs` by exactly 1/60 s = 16.667 ms.
- Bevy runs multiple ticks per render frame to catch up if rendering is
  slow; zero ticks if rendering is faster than 60 Hz.
- **Precision**: deterministic step, no jitter.

### Update (render loop)
- Runs once per vsync under `PresentMode::Fifo`.
- Native: OS compositor vsync. WASM: `requestAnimationFrame`.
- **Precision**: subject to OS/browser scheduling jitter (±1 ms typical).

---

## 9. Latency Pipeline (game state → photon)

```
 Game logic      Render submit    GPU work         Compositor       Scanout
 (FixedUpdate)   (Update)         (GPU pipeline)   (OS/browser)     (LCD/OLED)
     │               │                 │                 │              │
     ├ 0–16.7 ms ─▶ ├ <1 ms ──────▶  ├ 0.5–8 ms ──▶   ├ 0–16.7 ms ─▶ │
```

| Stage | Typical | Worst |
|---|---|---|
| FixedUpdate → Update | 0–16.7 ms | 16.7 ms |
| CPU render submission | <1 ms | <1 ms |
| GPU pipeline | 0.5–3 ms | 8 ms |
| Compositor (Fifo) | 0–16.7 ms | 33.4 ms |
| Display scanout (top) | ~0.1 ms | ~0.5 ms |
| **Total** | **~1–37 ms** | **~60 ms** |

This latency is nearly constant for a given hardware setup; the
photodiode measures it as a fixed `T_offset`.

---

## 10. Photodiode Calibration

### Setup
1. Place photodiode on top-right corner over the 50×50 px square.
2. Connect to DAQ / scope with ≤0.1 ms resolution.
3. Press **B** in the game to enable the calibration square.

### Protocol
1. Let it run several seconds; the square alternates W/B every render frame.
2. Record sensor transitions `T_sensor[i]` and log timestamps
   `render_elapsed_secs[i]` + `photodiode_white[i]`.
3. Align W→B transitions: `T_offset = T_sensor − T_game`.
4. Verify `T_offset` is stable (±0.5 ms across ≥100 transitions).

### Applying
`T_display = render_elapsed_secs + T_offset`.

### Expected values
| Setup | Typical T_offset |
|---|---|
| Native Linux, simple compositor, LCD | 16–25 ms |
| Native Linux, OLED | 8–18 ms |
| Native Windows (DWM) | 25–40 ms |
| WASM in Chromium | 20–35 ms |

---

## 11. Native vs WASM

| Aspect | Native (Linux) | WASM (browser) |
|---|---|---|
| Update loop driver | OS compositor vsync | `requestAnimationFrame` |
| Clock precision | `Instant::now()` ~ns | `performance.now()` ~5–100 μs† |
| Vsync reliability | High | Browser may throttle |
| Dropped frames observable | Yes | Yes (rAF may skip silently) |
| GC / JIT pauses | None | Rare >1 ms spikes |
| Thread model | Game + controller separate processes | Same thread, rAF-interleaved |

† Browsers degrade `performance.now()` to ~100 μs unless served with:
```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

**Recommendation**: native for timing-critical sessions. WASM is fine
for piloting/training.

### Web controller loop
Driven by `requestAnimationFrame` (not `setInterval`). Polling every
1 ms via `setInterval` was preempting the render loop and producing
~30–45 fps in the game. rAF aligns controller polling with vsync and
restores stable 60 Hz.

---

## 12. Ring Buffer & Polling

- 8-slot ring of full `SharedGameState`. Game pushes one entry per
  FixedUpdate. Controller drains via `read_game_state_since()`.
- Overflow detectable: `current_head - last_head > 8`.
- **Snapshot read** (`read_game_state`) gives the latest render-side
  fields; the ring is the only way to recover skipped FixedUpdate ticks.

What the monitor cannot capture: actual display onset (photodiode
only), compositor frame drops, inter-poll render frames (aliasing),
sub-frame timing.

---

## 13. Drift Sources & Definition

In trial logs you will see "drift" — the gap between game time and wall
clock. Causes:

1. **Skipped / catch-up FixedUpdate ticks** when a render frame is slow;
   averages out but produces transient drift.
2. **Intentional pauses** (`stop_rendering`, door animations) where
   `elapsed_secs` is paused by design but the controller wall-clock
   keeps ticking. The verifier excludes these via the `is_animating`
   flag — see `tools/verify_trial_logs.py::_active_drift_series()`.
3. **Web vsync ≠ 60 Hz** on 120 Hz / VRR displays. `FixedUpdate` still
   tries to hit 60 Hz; the resulting uneven tick distribution against
   wall clock manifests as small drift.
4. **Frame-logging key mismatch** (fixed): keying log by local
   `currentFrame` instead of `state.frame_number` caused intermediate
   ticks to overwrite and produced fake gaps. Both controllers now key
   by `state.frame_number`.

"Active drift" in plots = `elapsed_secs − (non_animating_tick_count / 60)`.

---

## 14. Recent Changes (this session)

- **Trial-log schema slimmed**: `elapsed_secs` and `render_elapsed_secs` are
  no longer recorded per frame. Both remain in SHM (the FSM still reads
  `elapsed_secs` for trial-timeout). Analysis is indexed on
  `present_elapsed_secs`; verifier falls back to `elapsed_secs` for legacy logs.


- **New decoration shapes** (append-only enum): Rectangle, Oval,
  Pentagon, Kite, Rhombus, Trapezoid, Semicircle. Meshes via `fan_mesh`
  in `game_node/src/utils/decorations.rs`.
- **Per-decoration rotation**: `decorations_rotation: [i32; 3]` in SHM
  (`-1` = seeded random with `0xDEC0_DEAD`, else degrees). Applied as
  `face_rotation * Quat::from_rotation_z(decoration.rotation)`.
- **`camera_rotation_sense: i32 ∈ {-1, +1}`** SHM field; multiplies
  `speed_rotate` in `handle_rotation`.
- **Trial editor `_type` bug**: stopped stripping `_type` on export;
  `Array.isArray` backfill on import.
- **`logFrame` keyed by `state.frame_number`** in both controllers.
- **R-press no longer issues `reset`** during `WAITING_FOR_START`. Only
  toggles `stop_rendering`. Fixes the ~1.3 s trial-0 stall.
- **Warmup scene** (`game_node/src/utils/warmup.rs`) pre-compiles GPU
  pipelines + uploads textures during Startup. `check_scene_ready` is
  now gated on `WarmupState.complete`.
- **`increment_timing` now pauses both `frame_number` and `elapsed_secs`
  when `stop_rendering` is true.** First visible frame of each trial
  has `frame_number = 0`, `elapsed_secs ≈ 0`.
- **`increment_render_frame_counter`** runs in `Update` and writes
  `render_frame_number` / `render_elapsed_secs` / `photodiode_white`
  every render frame.
- **Web controller**: replaced `setInterval(1ms)` with `rAF` loop.
- **Web logs**: download as ZIP grouped by level, session name from
  landing page, JSZip vendored at `vendor/jszip.min.js` (CDN+SRI failed).
- **Intermediate-frame logging gated on PLAYING / animation FSM
  states**; frame-tracking re-synced after reset in both controllers.
- **Verifier**: handles `.zip`, recursive dirs, auto-detects web;
  drift labeled "Active-time drift (animation pauses excluded)".
- **Session overview merged into `verify_trial_logs.py`**: emits
  `session_overview.png` (5-panel: outcomes, attempts, durations,
  alignment trajectory, per-trial present-Δt mean+p95) alongside the
  per-trial PNGs. Standalone `visualize_web_log.py` removed.
- **1% / 0.1% low FPS metrics**: added to the per-trial summary table,
  per-platform aggregate, and cross-platform comparison. Visualized in
  the presentation-Δt histogram (plot 5) as orange/red vertical lines.

---

## 14b. Subsequent Changes (post-2026-05-12, undocumented prior to this pass)

### Build / deployment
- **Gzipped WASM shipped**. `game_node/pkg/game_node_bg.wasm.gz` is the
  artefact loaded in production; `controller_main.js::start()` fetches
  the `.wasm.gz` URL and decompresses in-browser via
  `new Response(blob.stream().pipeThrough(new DecompressionStream("gzip")))`.
  Reason: GitHub Pages does not gzip `.wasm` responses, so we
  pre-compress and inflate client-side. Build step:
  `gzip -9 -k -f game_node/pkg/game_node_bg.wasm`.
- **WebGPU feature enabled** in `game_node/Cargo.toml`
  (Bevy features: `pbr_specular_textures, webgpu, jpeg`). Browsers
  without WebGPU still fall back to WebGL2 inside wgpu.
- **Minified controller**. `controller_main.min.js` is the served file;
  produced by
  `npx terser controller_main.js -c drop_console=true,drop_debugger=true -m`.
  `game.html` loads `./controller_main.min.js` (no `?v=` query).
- **Page title** changed in `index.html` from "Monkey 3D Game" /
  "Monkey 3D" → "3D Stimulus Game".

### Input handling (web)
- **Touch from WAITING_FOR_START starts the trial.** `touchstart`
  branch sets `_start = true` when `fsmState === FSM.WAITING_FOR_START`,
  mirroring the keyboard "R" path (PR #1, copilot fix).
- **Touch listeners ignore non-canvas / popup targets.** All three
  (`touchstart` / `touchmove` / `touchend`) early-return when
  `e.target.tagName !== "CANVAS"` and skip events inside
  `#download-popup`. Stops the download-popup buttons from being
  swallowed by `preventDefault`.

### Download / logs (web)
- **Auto-download on level finish.** `showDownloadPopup()` now calls
  `downloadLogs()` immediately, so the ZIP is offered without the user
  pressing the button. The button remains as a manual fallback.
- **iOS / Safari download path explored, then reverted.** Several
  commits tried `navigator.share({ files })`, `window.open(url)`, and
  `document.body.appendChild(a)` to work around iOS WebKit's broken
  `<a download>` for blob URLs. Final state (commit 952ac9a → 6c918d8):
  back to a minimal `Object.assign(document.createElement("a"), { href,
  download }).click()` — works once auto-download is fired from a user
  gesture (`touchend` → `showDownloadPopup` → `downloadLogs`).

### Game (Rust)
- **`handle_door_animation` respects `intensity_factor`.** The emissive
  colour is now scaled per-channel by `intensity_factor` (was hard-coded
  to `color.to_linear()`). Fix for "green light not visible on win" on
  some hardware (commit bfb6fb7 prerequisite).
- **Pyramid spotlights: `shadows_enabled = false`** in
  `spawn_pyramid_base` (`pyramid.rs`). Perf — shadow maps cost more than
  the visual gain.
- **FixedUpdate ordering changed** (commit bc13967): `handle_check_alignment`
  moved up to run right after `handle_reset_command`, and
  `handle_animation_door_command` moved to run after `handle_door_animation`.
  Schedule diagram in §4 has been updated. Rationale: the win/animation
  command must be issued from the same tick that detects alignment, and
  door-animation state must be visible before the door-animation command
  is processed.

### Controller logic
- **Single-green branch no longer gated on `nrAttempts < suggestionThreshold`**
  in both `controller_main.js` and `controller_python/controller.py`.
  Previously a correct alignment past the suggestion threshold did not
  trigger the green light (`bfb6fb7`). Now: `in_win_budget AND
  cosine > threshold` ⇒ single green, regardless of attempt count.

### Trial-log schema (controller, both platforms)
- **`elapsed_time` → `elapsed_time_no_anim` + `elapsed_time_anim`.**
  Per-trial durations are bucketed by the preceding frame's
  `is_animating` flag, summed over `present_elapsed_secs` deltas.
  Verifier and per-trial PNG titles report both. Old `elapsed_time`
  is removed (legacy logs still work via the verifier's fallbacks).
- **Per-trial frame-number rebasing.** `frame_number` and
  `render_frame_number` in logged frames (and the `frameLog` keys) are
  zero-based per trial. `_frame_zero` / `_render_frame_zero` are
  captured on the first logged frame; raw SHM values minus the zero are
  written into the log. `win_event` now stores `win_frame_number`
  (rebased) instead of `frame_number`.
- **Level summary rename + slim**. `trials` → `trials_runs`;
  `duration_s` → `elapsed_time_no_anim` + `elapsed_time_anim` (summed
  from member runs). Removed aggregates: `outcomes`, `total_attempts`,
  `chain_idxs_end`. `level_config` no longer includes the `fixed`
  block (it duplicates the per-trial config).
- **`start_orient` is a sentinel in trial JSON** (editor and saved
  levels store `-1`); the controller randomly picks from
  `START_ORIENTS` at trial init, writes the chosen value into SHM,
  AND records it as a top-level `start_orient` field in the trial log
  and `trials_runs[]` entry. `trial_config` no longer carries
  `start_orient`. Trial editor (`trial_editor.html`) sets and force-
  rewrites `fixed.start_orient = -1` on import so old files don't look
  editable.

### Verifier (`tools/verify_trial_logs.py`)
- **Per-trial PNGs annotate animation spans** (green `axvspan`) and
  **checked-attempt markers** (red dots) on both the camera-angle and
  FPS subplots. Helps eyeball when win checks happened.
- **Output path de-doubles platform prefix.** `_strip_platform_prefix`
  drops a leading `{platform}/` from `source_rel_path` so files don't
  end up under `out/analysis/web/web/...`.
- **ZIP root descent.** If a session ZIP contains a single top-level
  directory (the common case — name matches the zip stem), the loader
  recurses into it, preventing the session name from appearing twice in
  output paths.
- **Per-trial title format** now includes
  `(no_anim=…s, anim=…s, total=…s)` and uses the short
  `level_NNN_trial_NNN_run_NNNN_object_NNN` slug.
- **Session overview** accepts an explicit `title` (no longer a
  free-form `title_suffix`) and prints aggregate no_anim/anim totals.

---

## 15. Known Open Issues / Follow-ups

1. **Warmup reads the now-paused frame counter.** `tick_warmup` in
   `game_node/src/utils/warmup.rs:139` takes `Res<FrameCounterResource>`,
   which only ticks when `!stop_rendering`. During boot this is fine
   (stop_rendering defaults to false until the controller flips it),
   but if the controller toggles `stop_rendering = true` before warmup
   accumulates `WARMUP_FRAMES_AFTER_LOAD = 20` frames after textures
   load, warmup hangs and `is_scene_ready` never flips.
   **Fix**: switch `tick_warmup` to `RenderFrameCounterResource` (or a
   `Time<Real>`-based local counter). Render frames always tick.
2. **Native warmup is unconditional.** It costs ~few hundred ms at
   startup on native, where the original stall was less severe.
   Consider `#[cfg(target_arch = "wasm32")]`-gating the spawn — not
   urgent.
3. **`render_frame_number` reset semantics.** It is zeroed in
   `handle_reset_command`. Verify the controller's re-sync handles
   this — there can be a brief window where the controller sees the
   previous trial's render_frame_number in the snapshot before
   `reset` propagates. Current behavior is acceptable because the ring
   buffer is what's logged, but worth noting.
4. **Trial-0 still shows residual drift in some sessions**, mostly from
   browser vsync alignment on the first few frames. Not blocking.

---

## 16. Assumptions for Sub-1 ms Stimulus Onset Accuracy

### Hardware
- [ ] Display at stable known refresh rate (e.g. 60.000 Hz ±0.01 Hz).
- [ ] `PresentMode::Fifo` active (toggle with **V**).
- [ ] No thermal throttling during the session.
- [ ] Photodiode response ≤0.1 ms; DAQ timestamp ≤0.1 ms.
- [ ] Photodiode at top edge (~0.1 ms scanout vs ~16 ms at bottom).

### Software
- [ ] Native build for timing-critical sessions.
- [ ] No other GPU-heavy apps running.
- [ ] `render_frame_number` has no gaps during the experimental block.
- [ ] Calibration at session start AND end; `T_offset` std-dev <0.5 ms.
- [ ] Controller never writes `READ_ONLY_FIELDS`.

### Analysis
- [ ] Use `render_elapsed_secs + T_offset` for onset, never `elapsed_secs`.
- [ ] Exclude trials with render gaps, anomalous `r_dt`, or
  >1 ms `T_offset` drift.
- [ ] Align using the alternating W/B photodiode pattern, not absolute
  color.

---

## 17. Pre-Experiment Checklist

1. `cargo build --release -p game_node`
2. `cargo build --release -p shared --features python`
3. `cp target/release/libshared.so controller_python/monkey_shared.so`
4. Start game; start controller; start monitor `python controller_python/monitor.py --hz 60`.
5. Monitor sanity:
   - `g_dt` = 16.67 ms with no jitter.
   - `r_dt` ≈ 16.67 ms.
   - `gaps = 0`, `dups = 0`, `frz = 0`, `out% = 0.0%`.
6. Press **B** to enable photodiode. Verify `pdi` alternates.
7. Run photodiode calibration (§10). Record `T_offset`.
8. Verify `T_offset` std-dev <0.5 ms over ≥100 transitions.
9. Run experimental block.
10. Re-calibrate at session end; verify drift <0.5 ms.

---

## 18. Timing Field Reference

```
Timeline:     ──── FixedUpdate ticks ────────────────────────────────
                   │    │    │    │    │    │    │
                   ▼    ▼    ▼    ▼    ▼    ▼    ▼
  frame_number:    1    2    3    4    5    6    7    (game logic; 0 on reset, paused if stop_rendering)
  elapsed_secs:  0.000 .017 .033 .050 .067 .083 .100 (deterministic)

Timeline:     ──── Update ticks (vsync) ─────────────────────────────
                  │         │         │         │
                  ▼         ▼         ▼         ▼
  render_frame:   1         2         3         4    (render; 0 on reset, never paused)
  render_elapsed: 0.001     .018      .034      .051 (real clock ±jitter)
  photodiode:     W         B         W         B    (alternating)

  T_offset:    ◄──────────────────────────────────▶  (constant, photodiode-measured)
```

`render_elapsed_secs + T_offset` ⇒ actual display-onset time with
sub-1 ms accuracy when §16 assumptions hold.
