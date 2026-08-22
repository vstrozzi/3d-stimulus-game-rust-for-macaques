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
├── controller_main.js          web controller source (mirrors controller.py);
│                               minified into deploy_frontend/ by terser
├── deploy_frontend/            served web bundle (see §19)
│   ├── index.html              landing page (role-rendered; loads WASM)
│   ├── login.html              password page
│   ├── controller_main.min.js  minified build output of controller_main.js
│   ├── game_node -> ../game_node      symlink (WASM build dir is game_node/pkg)
│   ├── assets -> ../game_node/assets  symlink
│   └── trials_config -> ../trials_config  symlink
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
wasm-pack build game_node --target web --release --out-dir pkg
gzip -9 -k -f game_node/pkg/game_node_bg.wasm
npx terser controller_main.js -c drop_console=true,drop_debugger=true -m \
  -o deploy_frontend/controller_main.min.js
```
Output lands in `game_node/pkg`, reached through the `deploy_frontend/game_node`
symlink. Serve `deploy_frontend/` over HTTP with COOP/COEP headers (see §6,
§19). The page
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
| `session_time_left` | controller | f32 bits, `1.0` = full session left, `<0` hides the clock. Live-synced. |
| `correct_streak` | controller | Consecutive correct answers in the level (0 at level start). Drives the ambient particle density. Live-synced. |
| `platform_texture`, `background_texture` | controller | `Texture` enum index for the ground plane / curved wall. Per-level. |
| `platform_color_mask`, `background_color_mask` | controller | `[f32; 4]` = `[r, g, b, a]`; `a` is mask strength, `0` = the bare texture. Per-level. |

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
                spawn_warmup_scene
                spawn_score_bar_pool         (level chain, top-center — see §14e)
                spawn_left_score_bar         (trial bar, bottom — see §14e)
                spawn_session_clock          (round clock, blank screen only)
                setup_ambient_motes          (ambient particle pool)

PreUpdate:      read_shared_memory_commands       (chained)
                read_shared_memory_game_state_local  (seq-gated, snapshot copy)
                sync_live_state_from_shm           (every-frame copy of
                                                    progress_bar_*, score_bar_*,
                                                    session_time_left,
                                                    correct_streak, shake_*)

First:          commit_render_sample         (writes prev frame's staged
                                              sample to SHM, stamped with
                                              real-wallclock `now` —
                                              ≈ vsync of the prior flip)

Update:         update_ui_scale
                tick_warmup
                check_scene_ready
                stage_render_sample
                handle_reset_command         (chained)
                handle_check_alignment
                handle_blank_screen
                handle_stop_rendering
                handle_rotation
                handle_zoom
                update_faint_aligned_door    (always-on white hint on aligned door)
                handle_animation_door_command
                handle_door_animation
                update_winning_face_glow     (lights winning pyramid face during win)
                handle_camera_shake          (after handle_zoom — uses base position)
                update_score_bar             (level chain dots + connectors)
                update_left_score_bar        (color-interp trial bar)
                update_session_clock         (conic sweep; blank screen only)
                update_fog, update_fireflies (win-time swarm)
                update_ambient_motes         (streak-driven ambient swarm)

PostUpdate:     clear_pending_commands       (chained)
                increment_timing             (paused if stop_rendering)
                update_shared_memory_local
                write_shared_memory_game_state
```

> **Single vsync schedule.** Every system above runs at the display
> refresh rate (one tick per rendered frame). There is no `FixedUpdate`
> in this app — see §8 for the consolidated clock model and §14c for
> the migration log. `WinitSettings::continuous()` keeps the `Update`
> schedule firing each vsync even without input.

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

The `PreUpdate` ↔ `PostUpdate` round-trip preserves these fields through
the per-frame loop (local reads SHM in `PreUpdate`, writes back in
`PostUpdate`).

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
| `FrameCounterResource` (`frame_number`) | `PostUpdate::increment_timing` | **Yes** (early return) | Yes (= 0) |
| `RenderFrameCounterResource` (`render_frame_number`) | `Update::stage_render_sample` | No | Yes (= 0) |
| `RoundStartTimestamp` (→ `elapsed_secs`) | `PostUpdate::increment_timing` (accumulates `time.delta()`) | Yes | Reset to `Some(Duration::ZERO)` in `setup_round` |

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
- A **Background** section sits between Objects and Trials: texture dropdown +
  colour-mask picker (its alpha input is the mask strength) for the platform
  and the background. `enforceLevel` backfills all four keys on import.
- The toolbar shows the name of the file being edited. The editor loads the
  trial selected in the admin menu (this tab's `sessionStorage`) if there is
  one, else the server default — the same precedence the game uses.

---

## 7. System Architecture Overview

```
┌─────────────────────────┐       ┌───────────────────────────┐
│       Controller        │       │        Game (Bevy)        │
│  (Python native / JS)   │       │                           │
│                         │       │  PreUpdate (vsync)        │
│  Writes commands ──────────────▶│    read commands          │
│                         │       │                           │
│                         │       │  Update (vsync)           │
│                         │       │    game logic, rotation,  │
│                         │       │    animation, rendering   │
│                         │       │                           │
│                         │  SHM  │  First (vsync, next tick) │
│  Reads game state  ◀────────────│    commit render sample   │
│  Logs trial data        │       │    (present_elapsed_secs) │
│                         │       │                           │
│                         │       │  PostUpdate (vsync)       │
│                         │       │    elapsed_secs+=delta    │
│                         │       │    frame_number++         │
│                         │       │    write SHM + ring push  │
└─────────────────────────┘       └───────────────────────────┘
                                           │
                                    GPU pipeline → Compositor → Scanout → Photons
```

---

## 8. Single Clock Domain (vsync)

All game systems — command handling, gameplay logic, animations, score
bar updates, SHM writes — run **once per rendered frame** in Bevy's
standard `PreUpdate` / `Update` / `PostUpdate` schedules. There is no
`FixedUpdate` schedule in this app; `Time::<Fixed>` is not inserted as a
resource. The fixed/render split that earlier versions of this document
described has been removed.

| Field                  | Where written                  | Pauses on `stop_rendering`? |
|------------------------|--------------------------------|-----------------------------|
| `frame_number`         | `PostUpdate::increment_timing` | **Yes** (early return)      |
| `elapsed_secs`         | `PostUpdate::increment_timing` | **Yes**                     |
| `render_frame_number`  | `Update::stage_render_sample`  | No                          |
| `render_elapsed_secs`  | `Update::stage_render_sample`  | No                          |
| `present_elapsed_secs` | `First::commit_render_sample`  | No                          |
| `photodiode_white`     | `First::commit_render_sample`  | No                          |

`frame_number` and `render_frame_number` therefore share the same vsync
tick rate; the only behavioural difference is that `frame_number` and
`elapsed_secs` are pause-aware (skipped while `stop_rendering = true` or
during door animations that flip that flag), giving the controller a
clean "active game time" stream. Both pairs are kept in SHM and in trial
logs for back-compat — verifier code that previously consumed only
`frame_number`/`elapsed_secs` keeps working.

### Refresh-rate independence
Because the loop is vsync-driven, the per-tick cadence equals the
display refresh rate (60 Hz / 120 Hz / 144 Hz / VRR). Anything that
should be wall-clock-stable across displays must multiply by
`time.delta_secs()`. As of this writing:

- **Door animations** use `time.elapsed()` ✓
- **`elapsed_secs` accumulation** uses `time.delta()` ✓
- **Camera rotation / zoom** apply per-tick increments ✗
  ([shared_memory_reader.rs:75](game_node/src/shared_memory/shared_memory_reader.rs#L75-L98)
  adds `camera_speed_rotate`/`CAMERA_3D_SPEED_ZOOM` without a `delta`
  multiplier). At 120 Hz the monkey rotates at 2× the intended speed.
  Tracked as an open issue in §15.

### Refresh rate: OS-reported vs measured
The hardcoded `REFRESH_RATE_HZ` constant has been removed. Both
controllers now record two independent values per session:

- **`session_info.refresh_rate_hz`** — queried directly from the OS /
  browser at startup. Native: `xrandr --current` parsed in
  `_query_display_refresh_rate_hz()` (works on X11 and XWayland;
  returns `None` if `xrandr` is missing or the output doesn't parse).
  Web: `screen.refreshRate` (Chrome 121+; `null` elsewhere — no
  fallback).
- **`timing_health.refresh_rate_hz_measured`** — `1 / mean(Δpresent_elapsed_secs)`
  over the frames actually logged in this level. Used as a sanity check
  against the OS value; mismatches flag VRR sessions or dropped frames
  rather than telling you the "true" rate.

The two should agree to <0.5 Hz on a healthy Fifo session.

---

## 9. Latency Pipeline (game state → photon)

```
 Game logic      Render submit    GPU work         Compositor       Scanout
 (Update)        (end of Update)  (GPU pipeline)   (OS/browser)     (LCD/OLED)
     │               │                 │                 │              │
     ├ <1 ms ──────▶ ├ <1 ms ──────▶  ├ 0.5–8 ms ──▶   ├ 0–16.7 ms ─▶ │
```

| Stage | Typical | Worst |
|---|---|---|
| Game logic → render submit | <1 ms | <1 ms |
| CPU render submission | <1 ms | <1 ms |
| GPU pipeline | 0.5–3 ms | 8 ms |
| Compositor (Fifo) | 0–16.7 ms | 33.4 ms |
| Display scanout (top) | ~0.1 ms | ~0.5 ms |
| **Total** | **~1–20 ms** | **~45 ms** |

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
  rendered frame (`PostUpdate::write_shared_memory_game_state`).
  Controller drains via `read_game_state_since()`.
- Overflow detectable: `current_head - last_head > 8`.
- **Snapshot read** (`read_game_state`) gives the latest fields; the
  ring is the only way to recover frames the controller polled past.

What the monitor cannot capture: actual display onset (photodiode
only), compositor frame drops, inter-poll render frames (aliasing),
sub-frame timing.

---

## 13. Drift Sources & Definition

In trial logs you will see "drift" — the gap between game time and wall
clock. Since the migration to a single vsync-driven schedule (§4, §8),
`elapsed_secs` is just `time.delta()` accumulated per rendered frame, so
the drift definition is simpler than it used to be. Remaining causes:

1. **Intentional pauses** (`stop_rendering`, door animations) where
   `elapsed_secs` / `frame_number` are paused by design but the
   controller wall-clock keeps ticking. The verifier excludes these via
   the `is_animating` flag — see
   `tools/verify_trial_logs.py::_active_drift_series()`.
2. **Display ≠ 60 Hz** on 120 Hz / VRR displays. The game ticks at the
   display refresh rate, so `frame_number / 60` is no longer a faithful
   wall-clock proxy. The measured rate is now reported in
   `timing_health.refresh_rate_hz_measured`; divide by that instead.
3. **Frame-logging key mismatch** (fixed): keying log by local
   `currentFrame` instead of `state.frame_number` caused intermediate
   ticks to overwrite and produced fake gaps. Both controllers now key
   by `state.frame_number`.

"Active drift" in plots = `elapsed_secs − Σ(non-animating Δpresent_elapsed_secs)`.

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
  the visual gain. The main environment spotlight is intentionally separate:
  it casts shadows natively but disables them on WASM through
  `lighting_constants::SHADOWS_ENABLED`.
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

## 14c. Vsync-only schedule + measured refresh rate (this session)

- **`Time::<Fixed>` schedule removed.** All gameplay/command systems
  (`handle_reset_command`, `handle_check_alignment`, `handle_rotation`,
  `handle_zoom`, `handle_door_animation`, `handle_animation_door_command`,
  `update_score_bar`) run in `Update`. `increment_timing` /
  `clear_pending_commands` / `update_shared_memory_local` /
  `write_shared_memory_game_state` run in `PostUpdate`. `Time::<Fixed>`
  is no longer inserted as a resource in [lib.rs](game_node/src/lib.rs).
  Motivation: with FixedUpdate at 60 Hz drifting against vsync, the
  rendered transform was up to one fixed step stale relative to the
  current frame — visible as judder. Single-domain vsync eliminates it.
- **`REFRESH_RATE_HZ` constant deleted** from
  [shared/src/constants.rs](shared/src/constants.rs). Removed PyO3 export
  (`monkey_shared.REFRESH_RATE_HZ`) and wasm-bindgen export
  (`refresh_rate_hz()`). Also deleted unused `timing::frames_to_seconds`
  / `seconds_to_frames` / `WIN_BLANK_DURATION_FRAMES`.
- **Controllers query the OS/browser for refresh rate.**
  `session_info.refresh_rate_hz` is filled once at startup:
  `_query_display_refresh_rate_hz()` shells out to `xrandr --current`
  on Python; `_queryDisplayRefreshRateHz()` reads `screen.refreshRate`
  on JS. Returns `None` / `null` if the OS / browser doesn't expose it
  (no fallback to measurement). The measured value lives separately in
  `timing_health.refresh_rate_hz_measured` as a sanity check.
- **`frame_number` + `render_frame_number` both retained in SHM and trial
  logs** for downstream compatibility. They differ only in pause
  semantics (frame_number pauses on `stop_rendering` / `is_animating`).
- **`WinitSettings::continuous()`** in [lib.rs](game_node/src/lib.rs) keeps
  `Update` firing each vsync even without input. The former
  `force_redraw_every_frame` system was removed because changing the Bevy
  `Window` component did not request a winit redraw and only duplicated work.

---

## 14d. Trial-flow, lighting, and UX additions (this session)

This pass added a batch of features driven by the experimental design.
None of the §8 timing semantics changed.

### Trial-config schema additions
- **`fixed.start_object`** — `-1` = controller picks uniformly at random
  over chains; `>= 0` = specific chain index. Resolved by
  `_level_start_object` (Python) / `_levelStartObject` (JS) on every
  level transition (first level too). Editor exposes a select with
  `Random` plus one option per object.
- **Pseudo-random chain selection (shuffle bag).** The active chain for
  each trial is drawn from a `chain_bag` (Python: `self.chain_bag`; JS:
  `chainBag`) — a shuffled queue of chain indices for the current level.
  Each `complete_trial` pops the next index; when the bag empties it is
  refilled and reshuffled. Net effect: every (eligible) chain is visited
  exactly once before any chain can repeat, eliminating the long-tail
  starvation that the old memoryless `pr_switching_chain` reroll could
  produce. At level start the bag is built from
  `_refill_chain_bag(exclude=active_chain)` / `_refillChainBag(activeChain)`
  so the starter is not re-drawn within its own first cycle. ADVANCE
  still caps the chain position at `n` (see `_chain_pos` in §14e); finished
  chains can be re-visited but cannot advance past their terminal position; a level completes only when every
  chain hits terminal simultaneously. The old `pr_switching_chain` field
  and `_maybe_switch_chain` / `_maybeSwitch` logic are gone; legacy
  trial JSONs that still contain the key are silently ignored (the
  editor strips it on import).
- **`fixed.remove_completed_chains`** — bool, default `false`. When
  `true`, chains whose `chain_idxs[i] >= n_trials` are filtered out of
  the bag at refill time (matches the original "remove finished chains
  from the pool" behaviour). When `false` (default), completed chains
  stay in the bag and can be revisited — they just won't advance.
  Toggle in the editor under Trials.
- **`fixed.random_seed`** — int, default `-1`. Seeds every controller-
  side random draw on this level (chain-bag shuffle in
  `_refill_chain_bag` / `_refillChainBag`, random `start_object` in
  `_level_start_object` / `_levelStartObject`, and per-trial
  `start_orient` pick in `_handle_init`). `-1` = the controller draws
  a fresh u32 from system entropy at level start; any other value is
  used verbatim. Re-seeding happens once at controller startup and
  again on every level transition, so revisiting a level under
  modulo-wrap produces a bit-identical run (assuming the explicit-seed
  case). Python uses a per-instance `random.Random` (`self._rng`); JS
  uses a module-level mulberry32 (`_rand()` / `_seedRng()`) since
  `Math.random` is not seedable. **The resolved seed** (the literal int
  actually used, even in entropy mode) is stamped into each trial log
  and each `trials_runs[]` entry as `level_random_seed` so a recorded
  session can be replayed by copying the int back into the editor.
- **`fixed.score_bar_max`** — editor int (`0` hides the bar). Persists
  across levels; the bar value is session-scoped.
- **`fixed.show_progress_bar`** — *removed in §14e; the key is ignored by both
  controllers and stripped by the editor on import.* Was: bool, default `true`, hides the
  trial-progress dots column without touching chain logic. Implemented
  by gating `_progress_bar_size` / `_progressBarSize` to return `0`
  when the flag is false; the game-side dot visibility predicate
  (`dot.index < progress_bar_size` in
  [game_functions.rs::update_score_bar](game_node/src/utils/game_functions.rs))
  then hides every dot. Toggling back on resumes from the correct
  `progress_bar_cur_size` because that counter is computed from
  `chain_idxs` and was never zeroed.
- **`fixed.shake_amplitude`, `fixed.shake_duration`** — per-level camera-
  shake config. Defaults `0.5` and `1.0`. Forwarded as `f32::to_bits()`
  in the dedicated SHM fields (see below).
- **`trials[i].show_all`** — per-trial bool. Only consulted in the
  retroceed (final-wrong) branch; flips `animation_all_door=true` +
  `animation_colored=true` so the existing game logic paints **red on
  all doors** instead of red on the target. All other branches send
  the historical command tuple unchanged.
- `start_object` and `show_all` are in `CONTROLLER_META_FIELDS` in
  [shared/src/constants.rs](shared/src/constants.rs) so they're filtered
  out of `write_game_state`.
- Both controllers' `_backfill_level_defaults` /
  `backfillLevelDefaults` populate every new field on import so old
  `trials.jsonl` files keep loading.

### SHM additions
- **Commands**: `shake: AtomicBool`. Sent on every wrong-alignment
  check (`cosine < threshold`).
- **State (game_structure_control)**:
  - `score_bar_value: AtomicU32`, `score_bar_max: AtomicU32` —
    controller-owned, live-updated.
  - `shake_amplitude: AtomicU32` (f32 bits), `shake_duration: AtomicU32`
    (f32 bits) — per-level config.
- **`SCORE_BAR_DEFAULT_MAX = 0`** in
  [shared/src/constants.rs](shared/src/constants.rs) so the left bar is
  invisible before the controller writes a real max.

### Live SHM sync (was a latent bug)
`read_shared_memory_game_state_local` is gated by `command_seq`, but
`read_shared_memory_commands` runs first in PreUpdate and already
advances `last_seq.0`, so the second system always returned early —
`local_game_struct.0` was only refreshed when `handle_reset_command` →
`setup_round` ran (i.e., on trial init). This meant the score bar,
shake params, and any future controller-owned live state would only
update at trial boundaries.

Fix: new `sync_live_state_from_shm` system added to PreUpdate (third in
the chain). It bypasses the gate and copies just the live controller-
owned fields each frame:
```rust
local_game_struct.0.score_bar_value = gs.score_bar_value.load(Relaxed);
local_game_struct.0.score_bar_max   = gs.score_bar_max.load(Relaxed);
local_game_struct.0.shake_amplitude = gs.shake_amplitude.load(Relaxed);
local_game_struct.0.shake_duration  = gs.shake_duration.load(Relaxed);
```
The seq-gated full-snapshot copy is still useful for fields that only
need to refresh on trial reset; this system layers on top for the few
fields that need per-frame propagation.

### Light palette
- `LIGHT_RED = #8B0000` and `LIGHT_GREEN = #CCFF00` exposed as `Color`
  consts in [handle_commands.rs](game_node/src/utils/handle_commands.rs).
- `handle_animation_door_command` collapsed to:
  `colored && !all_door → GREEN`, else `RED`. The old four-way
  truth table (with the alpha-0 "invisible" branch) is gone.
- `update_score_bar` / left-bar share the same palette.

### Always-on aligned-door hint (`update_faint_aligned_door`)
The door most-aligned with the camera always glows a faint white. Lives
in [game_functions.rs](game_node/src/utils/game_functions.rs).
- `HoleLight` and `HoleEmissive` now carry a `door_index: usize` so
  the system can filter without joining through `ChildOf`.
- During alignment the door's emissive is set to
  `WHITE * FAINT_ALIGNED_INTENSITY_FACTOR` (default `1/8`) and the
  spotlight is set to
  `max_intensity * FAINT_ALIGNED_SPOTLIGHT_FACTOR` (default `1/64`)
  with `FAINT_ALIGNED_SPOTLIGHT_RANGE = 4.0` instead of the normal
  `HOLE_SPOTLIGHT_RANGE = 25.0`. All four constants live in
  [shared/src/constants.rs::lighting_constants](shared/src/constants.rs)
  so they're tweakable without recompiling the system body.
- When `is_animating` flips true the faint system **clears all emissives
  and spotlights**, restoring spotlight ranges to `HOLE_SPOTLIGHT_RANGE`,
  so the win/wrong animation can paint cleanly from a black baseline.
- The hole pentagon material now uses `base_color: Color::BLACK` (see
  [pyramid.rs](game_node/src/utils/pyramid.rs)) — default `WHITE` was
  picking up ambient and washing out the small emissive deltas.

### `animate_all` spotlights
`handle_door_animation` no longer forces `target_intensity = 0.0` when
`animate_all = true`. Instead spotlights run at
`max_intensity * intensity_factor * FAINT_ALIGNED_SPOTLIGHT_FACTOR`,
which matches the always-on hint's brightness. This lets the "wrong on
all doors" (`show_all=true`) path actually be visible (and dim).

### Winning face glow (`update_winning_face_glow`)
During the win animation only (color == `LIGHT_GREEN` && `!animate_all`),
the triangular pyramid face above the winning door is lit with a soft
green emissive over the same `fade_out / stay_open / fade_in` envelope
as the door spotlight.
- New `PyramidFace { normal: Vec3 }` component on each face entity
  ([pyramid.rs](game_node/src/utils/pyramid.rs)).
- At runtime the system picks the face whose world-space normal best
  matches the target door's world-space normal (both rotate by the
  shared `RotableComponent` yaw, so the dot product is stable).
- `WINNING_FACE_GLOW = 0.4` scales `LIGHT_GREEN.to_linear() *
  intensity_factor`. Other faces are forced to `LinearRgba::BLACK`.

### Camera shake (`handle_camera_shake`)
Damage-style shake on wrong attempts. Lives in
[camera.rs](game_node/src/utils/camera.rs).
- New `CameraShakeState` resource holds `start: Option<Duration>`,
  `amplitude`, `duration`.
- Triggered by SHM cmd `shake`. Amplitude/duration are read from
  `local_game_struct.0` (kept fresh by `sync_live_state_from_shm`).
- Effect: small `rotate_local_x` (pitch) + `rotate_local_z` (roll)
  jitter, exponentially decayed (`exp(-4t/duration)`), two pseudo-random
  sine frequencies (`37 Hz`, `53 Hz`).
- **Pitch and roll only** — yaw (`rotate_local_y`) feeds back into
  `handle_zoom`'s euler-yaw extraction next frame and drifts the orbit
  position; pitch/roll don't. `handle_zoom` calls `look_at(ZERO, Y)`
  each frame which resets rotation, so no accumulation.
- Controllers send `shake=true` whenever a `check` press has
  `cosine_alignment <= threshold` (same predicate that decrements the
  score bar). `shake=false` on correct.

### Left-side score bar
New UI element ([ui.rs](game_node/src/utils/ui.rs)):
`LeftScoreBarRoot` (border + bg) containing a single `LeftScoreBarFill`
that grows from the bottom. `update_left_score_bar`:
- Hides the root when `score_bar_max == 0`.
- Lerps `LIGHT_RED ↔ LIGHT_GREEN` proportional to `value/max`.
- Alpha is `LEFT_SCORE_BAR_ALPHA = 0.30` from
  [shared/src/constants.rs](shared/src/constants.rs).
- Controller maintains `scoreBarValue` (Python: `self.score_bar_value`;
  JS: `scoreBarValue`). Initialized to `score_bar_max / 2` on session
  start, persists across levels, `+1` on correct check, `-1` on wrong
  check, clamped to `[0, max]`.

### Trial-progress dots bar moved to the left
The existing top-of-screen progress bar is now a **vertical** column
just to the right of the score bar (`TRIAL_BAR_LEFT_PX =
LEFT_SCORE_BAR_LEFT_PX + LEFT_SCORE_BAR_WIDTH_PX + 10`). Fixed vertical
span regardless of trial count. Layout:
- Outer node: `FlexDirection::Row` to hold one or more columns.
- Each column: `FlexDirection::ColumnReverse` (fills bottom→top),
  `PROGRESS_BAR_WRAP_AROUND_SIZE` dots per column.
- When `progress_bar_size > WRAP_AROUND_SIZE`, the next 20 dots wrap
  into a new column to the right of the previous one — vertical extent
  is unchanged.
- `update_score_bar` is untouched; it still just toggles
  `Node.display` per dot/chain index.

### Editor changes
[trial_editor.html](trials_config/trial_editor.html) gained: editable
`pr_switch`, `start_object` select (Random + chain indices),
`score_bar_max` input (min 0), `shake_amplitude` + `shake_duration`
inputs (in the Animation group), per-trial `show_all` checkbox.
Backfill in `enforceLevel` populates every new field on import so older
JSONL files still validate.

### Controller-side housekeeping
- Both `_CMD_KEYS` lists (module-level and class-level in Python; the
  JS `CMD_DEFAULTS` and `triggers`) include `shake`.
- Python `write_no_commands` builds the dict from `_CMD_KEYS` rather
  than enumerating keys inline, so future SHM cmd additions only need
  the `_CMD_KEYS` update.
- `session_info.paradigm` is the basename of the loaded `trials.jsonl`
  (Python takes `sys.argv[1]` if provided; JS uses either
  `custom_trials_name` from sessionStorage or the default
  `trials.jsonl`).

### Rebuild order after this session
```
cargo build --release -p shared --features python
cp target/release/libshared.so controller_python/monkey_shared.so
cargo build --release -p game_node
wasm-pack build game_node --target web --release --out-dir pkg      # web only
```

---

## 14e. Progress UI, session clock, particles, backdrops, chain bookkeeping (2026-08-22)

A feature batch driven by experimental feedback. No §8 timing semantics
changed, and the **per-frame trial-log schema is untouched**
(`LOGGED_STATE_FIELDS` is the same list).

### Trial-config schema (`fixed`) — additive, old files keep working
| key | default | meaning |
|---|---|---|
| `platform_texture` | `7` (`Rock024_1K`) | ground-plane texture, `Texture` enum index |
| `platform_color_mask` | `[0,0,0,0]` | `[r,g,b,a]`; `a` = mask strength, `0` = bare texture |
| `background_texture` | `10` (`Tiles017_1K`) | curved back-wall texture |
| `background_color_mask` | `[0,0,0,0]` | same encoding |

Names are identical to the SHM fields, so they flow through
`expand_flat_trial` / `buildTrialState` with no conversion code. Both
controllers' backfills and the editor's `enforceLevel` populate them, so an
older `trials.jsonl` loads unchanged and re-saving through the editor writes
them out. `fixed.show_progress_bar` is now inert (ignored by both controllers,
stripped by the editor). `fixed.score_bar_max` kept its name but is now a
**visibility switch** (`0` hides the trial bar), not a capacity.

### SHM additions (`game_structure_control`)
- `session_time_left: u32` (f32 bits) — fraction of the session left.
- `correct_streak: u32` — consecutive correct answers in the level.
- `platform_texture`, `background_texture: u32`; `platform_color_mask`,
  `background_color_mask: [u32; 4]` (f32 bits).
- `sync_live_state_from_shm` gained `progress_bar_size`,
  `progress_bar_cur_size` (without these the chain could not animate mid-trial),
  `session_time_left` and `correct_streak`.

### Game side
- **Level chain** (`spawn_score_bar_pool` / `update_score_bar`) moved to a
  horizontal row at the **top center**, one dot per level, filled left→right on
  level completion. **Trial bar** (`spawn_left_score_bar` /
  `update_left_score_bar`) moved from the left edge to the **bottom**, showing
  the mean trial position across the level's chains — full = level complete.
  Both blink + swell during the door animation, using the value the controller
  pre-pushes on the terminal attempt (`_projected_progress`), and settle at the
  end of the animation. Rates/scales in `game_constants`
  (`PROGRESS_PULSE_HZ`, `PROGRESS_PULSE_SCALE`, `TRIAL_BAR_PULSE_HZ`).
- **Session clock** (`spawn_session_clock` / `update_session_clock`,
  [ui.rs](game_node/src/utils/ui.rs)): a white-transparent disc whose spent
  wedge sweeps clockwise from noon into dark, drawn with a Bevy
  `Gradient::Conic`. Shown **only on the between-trial black screen**, top
  center. Radius: `SESSION_CLOCK_RADIUS_PX`.
- **Ambient particles** ([fog.rs](game_node/src/utils/fog.rs)): a second,
  always-on swarm (`AmbientMote`) drifting in a ring in front of the back wall,
  independent of the win-time `Firefly` burst. Pool of `AMBIENT_COUNT_MAX` is
  spawned once at startup; density is pure visibility (motes past the current
  count get scale 0), so a change costs nothing. Count follows `correct_streak`
  in `AMBIENT_STEPS` steps from `AMBIENT_COUNT_MIN` to `AMBIENT_COUNT_MAX`;
  streak 0 = no motes. Hidden while the black screen is up. All tuning lives in
  `ambient_particle_constants` — nothing per-level, nothing in the editor.
- **Backdrops** ([setup.rs](game_node/src/utils/setup.rs)): a `Backdrop`
  marker (`Platform` / `Background`) on the ground plane and the curved wall;
  `apply_backdrops`, called from `setup_round`, rewrites each material **in
  place** via `materials.get_mut` (no asset churn per trial). `mask_tint`
  multiplies the natural texture by `WHITE.lerp(rgb, a)`, so `a = 0` is
  byte-identical to the pre-feature look. Defaults + UV tiling in
  `backdrop_constants`.
- **Perf**: both particle swarms are now `NotShadowCaster` +
  `NotShadowReceiver` and use a low-poly sphere (`.mesh().uv(6, 4)`, ~36 tris
  instead of ~600); ambient motes only spawn inside `AMBIENT_ARC_DEG`, the arc
  facing the camera (the wall arc is on −Z, the camera on +Z).
- **Components consolidated**: every `#[derive(Component)]` now lives in
  [objects.rs](game_node/src/utils/objects.rs) — `Firefly`, `AmbientMote`,
  `WarmupEntity`, `PhotodiodeMarker` moved there; their modules import them.
  Resources stayed with their subsystems.

### New constant modules ([shared/src/constants.rs](shared/src/constants.rs))
- `ambient_particle_constants` — `AMBIENT_STEPS`, `AMBIENT_COUNT_MIN/MAX`,
  `AMBIENT_WALL_GAP`, `AMBIENT_INNER_RADIUS`, `AMBIENT_ARC_DEG`,
  `AMBIENT_Y_MIN/MAX`, `AMBIENT_SIZE/SPEED/GLOW/COLOR`.
- `backdrop_constants` — `PLATFORM_TEXTURE`, `BACKGROUND_TEXTURE`,
  `PLATFORM_TILE`, `BACKGROUND_TILE`, `COLOR_MASK_NONE`. The two texture
  defaults are exported to Python (`monkey_shared.PLATFORM_TEXTURE`) and to JS
  (`controller_constants()`), so neither controller nor the editor hard-codes
  an enum index.
- `game_constants` — `SESSION_CLOCK_RADIUS_PX`, `PROGRESS_PULSE_HZ`,
  `PROGRESS_PULSE_SCALE`, `TRIAL_BAR_PULSE_HZ`.

### Chain bookkeeping — position vs. logged trial index
Previously a chain that beat its last trial parked at `idx = n`, a trial that
does not exist: it was logged and named in filenames, and a later RETROCEED
erased the fact that the chain had ever been completed. Now the position is
stored **split**:

```
chain_idxs[i]        trial index, never leaves [0, n-1]  → what gets logged
chain_last_beaten[i] bool, set when the chain beat its last trial
_chain_pos(i)        = chain_idxs[i] + beaten            → 0..n, all the arithmetic
```

`_chain_pos` is used by `_level_complete`, the chain-bag refill filter,
`_level_progress_frac` and `_projected_progress`, so every completion/progress
computation sees exactly the number it saw before. `_trial_idx()` (logs,
filenames, prints) now always names a trial that exists.

`_next_pos(delta)` owns the step rule (`+1` advance, `0` stay, `-1` retroceed)
and is used **both** by `_handle_trial_index_update` and by the value
pre-pushed to the progress bar, so the animation and the real update cannot
disagree. Rule change: **a retroceed steps back from the trial actually
played**, not from the position — a chain that has beaten its last trial drops
to the trial *before* it and needs two consecutive wins to complete again
(previously it only lost the completion and replayed the same last trial).

### Trial-log schema
- **New**: `trial_chain_completed` (bool) in the per-trial log **and** in the
  `trials_runs[]` summary row, true on the run that beat the chain's last
  trial. Computed at save time (`ADVANCE && trial_index_in_chain == n-1`),
  since `save_trial_log` runs before the index update.
- `trial_index_in_chain` now always names a real trial (was `n` for a
  completed chain).
- `trial_config` picks up the four backdrop keys automatically (it is the
  flattened `fixed`).
- `level_config` drops `fixed`, so the level summary carries the backdrop under
  its own `level_config.background` key with the same four field names.
- Per-frame rows: **unchanged**.

### Controller / frontend
- **EN/DE toggle** on the name + instructions pages
  ([index.html](deploy_frontend/index.html)); English is the markup and the
  default, German comes from an `I18N_DE` table. The choice is handed to the
  game through `sessionStorage.lang`; `controller_main.js` uses it for the
  loading/instruction/press/end-popup strings. The title stays
  "Object Manipulation" in both.
- `#start-trial-overlay` now follows `state.is_blank` instead of being opaque
  for the whole break, so game-drawn UI (the clock) is visible on the web
  during breaks.
- `correct_streak` resets to 0 at every level transition.
- **Fixed**: `setSceneConfig` must stamp the backdrop fields. `controllerLoop`
  writes the whole state every frame from a scratch object the reader never
  fills for write-only fields, so leaving them out pushed `0` (=`Bark001_1K`,
  a bark/wood look) over the real values, which the game then picked up at the
  next reset.
- **Fixed**: promoting a library trial to default (`★ default`) now clears the
  tab's `custom_trials_jsonl` selection. `loadLevels()` prefers that selection
  over the default, so without the clear, Play silently kept replaying the
  previously selected file.

### Rebuild order after this session
```
cargo build --release -p shared --features python
cp target/release/libshared.so controller_python/monkey_shared.so
cargo build --release -p game_node
wasm-pack build game_node --target web --release --out-dir pkg
gzip -9 -k -f game_node/pkg/game_node_bg.wasm
npx terser controller_main.js -c drop_console=true,drop_debugger=true -m \
  -o deploy_frontend/controller_main.min.js
```
The SHM layout **and** the PyO3 signature both changed: a stale
`monkey_shared.so` now fails on `monkey_shared.PLATFORM_TEXTURE`, and the web
bundle and the wasm must be deployed together (the JS reads its field offsets
from the wasm).

---

## 15. Known Open Issues / Follow-ups

1. ~~**Warmup reads the now-paused frame counter.**~~ **Fixed.**
   `tick_warmup` now takes `Res<RenderFrameCounterResource>`, which
   ticks every render frame regardless of `stop_rendering`. Warmup
   progresses to completion even if the controller toggles
   `stop_rendering = true` before the 20 post-decode warmup frames
   have rendered. `WarmupState.all_loaded_at_frame` now stores a
   render-frame index, not a FixedUpdate index.
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
5. **The "Pay attention to the object's details" instruction is web-only.**
   It lives in `TEXT_EN` / `TEXT_DE` in
   [controller_main.js](controller_main.js); `controller_python/controller.py`
   has no equivalent line, so a native session does not show it. Mirror it if
   native sessions should read the same instructions.
6. **Camera rotation and zoom are per-tick, not per-second.**
   [shared_memory_reader.rs:75-98](game_node/src/shared_memory/shared_memory_reader.rs#L75-L98)
   increments `pending.rotation`/`pending.zoom` by `camera_speed_rotate`
   / `CAMERA_3D_SPEED_ZOOM` on every read tick, without multiplying by
   `time.delta_secs()`. Since the schedule migration (§4) all systems
   tick at the display refresh rate, so on a 120 Hz monitor the camera
   rotates at 2× the intended angular velocity. Door animations and
   `elapsed_secs` are unaffected (both use `time.elapsed()` /
   `time.delta()`). Fix: multiply both increments by
   `time.delta_secs()` and rescale the speed constants accordingly.

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
   - `g_dt` ≈ `r_dt` ≈ 1/display_refresh (16.67 ms on 60 Hz, 8.33 ms on
     120 Hz). Both clocks are vsync-driven now (§8), so any divergence
     between them is a bug.
   - `gaps = 0`, `dups = 0`, `frz = 0`, `out% = 0.0%`.
6. Press **B** to enable photodiode. Verify `pdi` alternates.
7. Run photodiode calibration (§10). Record `T_offset`.
8. Verify `T_offset` std-dev <0.5 ms over ≥100 transitions.
9. Run experimental block.
10. Re-calibrate at session end; verify drift <0.5 ms.

---

## 18. Timing Field Reference

All counters tick on the same vsync schedule (see §8). The only
difference between the two pairs is the pause behavior under
`stop_rendering` / `is_animating`.

```
Timeline:     ──── Render frames (vsync) ────────────────────────────
                  │         │         │         │
                  ▼         ▼         ▼         ▼
  render_frame:   1         2         3         4    (never paused; 0 on reset)
  render_elapsed: 0.000     .017      .033      .050 (round_start.delta sum, frame N)
  present_elapsed:0.017     .033      .050      .067 (Time::<Real>::now at frame N+1's First)
  photodiode:     W         B         W         B    (alternating; sampled at frame N)

  frame_number:   1         2         3         4    (paused while stop_rendering/animating)
  elapsed_secs:  0.000     .017      .033      .050 (paused with frame_number)

  T_offset:    ◄──────────────────────────────────▶  (constant, photodiode-measured)
```

`render_elapsed_secs + T_offset` ⇒ actual display-onset time with
sub-1 ms accuracy when §16 assumptions hold. For per-trial analysis,
`present_elapsed_secs` deltas yield the real frame interval (1 / measured
refresh rate; see `timing_health.refresh_rate_hz_measured` in level
summaries).

---

## 19. Hosted web server + per-trial logging

`deploy_backend/log_server.py` (FastAPI + uvicorn) replaces static web
hosting. It serves the **`deploy_frontend/`** bundle **behind a cookie gate**
and receives the per-trial logs the web controller used to bundle into a client
ZIP. `deploy_frontend/` holds `index.html`, `login.html`,
`controller_main.min.js`, and symlinks `game_node` / `assets` / `trials_config`
back into the repo (so the frontend's relative paths and the wasm build dir are
unchanged). Static realpaths are contained under `REPO_ROOT` so those symlinks
resolve but nothing outside the project is reachable; `out/server_logs/` is
explicitly excluded from static serving.

### Auth
- Two argon2 password hashes in env (`PLAYER_PW_HASH`, `ADMIN_PW_HASH`) +
  `SECRET_KEY`. `POST /login` verifies (constant-time) and sets a signed,
  HttpOnly, SameSite=Strict cookie (`itsdangerous`) carrying `{role}`.
- A single ASGI middleware gates **every** path (only `/login` is public) and
  stamps COOP/COEP on all responses (so `SharedArrayBuffer` works on
  `localhost` dev and behind Caddy in prod alike — Caddy only does TLS). This
  is why guessing `index.html`/`trial_editor.html` fails without a cookie.
- `role=admin` is additionally required for `/admin/*`. `GET /me` returns the
  role; `index.html` reads it to pick the player vs admin landing.
- Login is rate-limited per IP (`LOGIN_MAX`/`LOGIN_WINDOW`).

### Pages (`index.html`, single file, role-rendered)
- **player**: name → instructions (black bg, fullscreen/main-monitor/20-min/
  per-trial-save notes) → two-step Play (fullscreen → boot).
- **admin**: the original landing + a name field (test play); `upload_trial`
  (validates a `.jsonl`, `POST /admin/trials/save` to the library, selects it);
  `select_trial` (popup over `/admin/trials/list` with per-row use / ★default /
  rename / delete → `/admin/trials/{make_default,rename,delete}`); a "Make
  selected the default" button; and a **data popup** that browses
  `/admin/list` (navigate), `/admin/file` (view inline) + a "Download this
  folder (.zip)" button → `/admin/zip`.
- `login.html` is a standalone password page.

### Trial-config library
- `trials_config/trials/` holds all saved trials; the active default is
  `trials_config/trials/trials.jsonl` and its backup is `trials_old_default.jsonl`.
  `controller.py` (default arg), `controller_main.js` (`TRIALS_PATH`) and
  `trial_editor.html` (`fetch('./trials/trials.jsonl')`) all read that path.
- `/admin/trials/save|delete|rename` operate on names validated by `_trial_path`
  (basename, `.jsonl`, no traversal). `/admin/trials/make_default` renames the
  current `trials.jsonl` → `trials_old_default.jsonl` and copies the selected
  file in (the selected one stays in the library).

### Storage
- `POST /log {relpath, content}` writes
  `out/server_logs/<server-date>/<relpath>` with the same atomic
  tmp+`os.replace` as `controller.py`. The top folder is the **server's date**,
  so a day folder holds every player that played that day. `relpath` is built by
  the client as `<name>/<name>_<YYYY-MM-DD_HH-MM-SS>/level_NNN/<HHMMSS>/…` — a
  per-session `<name>_<timestamp>` folder (captured once per play in
  `_sessionFolderName()`) so repeat plays never collide; inside it the
  `level_NNN/<HHMMSS>/trials` shape is byte-identical to a native run, so
  `tools/verify_trial_logs.py out/server_logs/<date>/<name>/` reads it directly.
  `relpath` is validated against traversal.
- `GET /admin/zip?path=` recursively compresses the selected folder on the fly
  (`path=''` → everything); the zip's internal layout matches the previous
  web-version session ZIP. Nothing is stored zipped.

### Reliability model (`controller_main.js`)
- `pending: Map<relpath, json>` holds **only unconfirmed** trials/summaries.
  `saveTrialLog` enqueues the trial + the running summary (same relpath ⇒
  overwrites the prior unsent summary; mirrors `_flush_level_summary`).
- `flushPending()` POSTs each item; on HTTP 200 it's **deleted from memory**.
  A `setInterval(flushPending, 3000)` retries after transient drops.
- There is **no client ZIP / download** (JSZip, `downloadLogs`, the download
  popup, the `allTrialLogs`/`levelSummaries` retention, the long-press / Ctrl+D
  download triggers, and `vendor/jszip.min.js` were all removed). At end of
  game (last level or 20-min cap) `showEndPopup()` shows `pending.size` and the
  sender keeps retrying until it reaches 0 ("All data saved"). If the player
  closes early, the still-unsent trials are lost (by design).

### Deploy
`deploy_backend/Caddyfile` (HTTPS) + `deploy_backend/monkey-log-server.service`
(systemd, `Restart=always`, runs `python -m uvicorn deploy_backend.log_server:app`
from the repo root). Run on a VM with a **persistent disk** under
`out/server_logs/`; HTTPS is mandatory (SAB / cookies / fullscreen).
