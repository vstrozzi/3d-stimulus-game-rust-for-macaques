# Monkey 3D Game - Native and WASM Versions (Decoupled Game Instance and Controller)

A simple environmental 3D game designed to analyze the learning of 3D world models across macaques, teenagers with autism, and ML models. The game consists of distinguishing and learning the shapes of two structures, Type 1 and Type 2, with one level per pair composed of many trials. No indications must be provided to the players. The game logic here (`game_node`) is decoupled from the controller (`controller_py` or `web/controller_main.js`, which use a simple FSM to handle the trials and define the learning phases across each level). Depending on whether it is running on WASM or natively, the two instances communicate via: (1) two processes using Linux shared memory natively (`/tmp`), or (2) the same memory region on the web. We use Bevy to provide the same instance across web and native environments, as we require specialized hardware for the monkeys and broad distribution for patient/tablet usage.

This architecture allows for extremely low-latency, lock-free communication between the game and external controllers, supporting multiple languages and platforms.

## Architecture

* **Shared Library (`shared`)**: Defines the atomic data structures (`SharedCommands`, `SharedGameState`) and handles platform-specific shared memory creation (mmap on Native, SharedArrayBuffer on Web).
* **Game Node (`game_node`)**: The Bevy application. It reads commands from shared memory and writes the game state to shared memory every frame.
* **Controllers**:
    * **Python (`controller_python`)**: Tkinter + transitions GUI built on the `monkey_shared` PyO3 bindings for interactive control.
    * **Web (`controller_web`)**: HTML/JS interface. Loads the WASM game and interacts via shared memory buffers.

### For Web Build
* `wasm-pack`: `cargo install wasm-pack`

### For Python Controller
* Python 3.10+
* `pip install transitions`
* (Linux) `sudo apt install python3-tk` if Tkinter is missing

## How to Run

**Important**: You must run the `game_node` and the `controller` in separate terminals.

### 1. Start the Game Node
Terminal 1:
```bash
cargo run -p game_node
```

### 2. Start a Controller (Terminal 2)


#### Python Controller
```bash
# Build shared library with Python bindings
cargo build --release -p shared --features python

# Copy the module next to the controller
cp target/release/libshared.so controller_python/monkey_shared.so

# Run the GUI controller
python controller_python/controller.py
```

#### Web Controller
1. Build WASM (`wasm-pack build game_node --target web --out-dir pkg`) #add --dev for no optimizations
2. Launch

## How to create levels

Create Custom levels by using trials_editor.html


### Prepare Textures from ambientCG for bevy

python prepare_bevy_textures.py ./Metal061B_1K-JPG


### Verify run

python tools/verify_trial_logs.py out/trial_logs/

## Trial log schema (what's recorded, where it comes from)

### Folder layout (same on native and web)

```
<participant>/<YYYY-MM-DD>/level_<NNN>/<HHMMSS>/
    <participant>_level_<NNN>_summary_run_<KKK>_<YYYYMMDD-HHMMSS>.json        level metadata summary
    trials/
        <participant>_level_<NNN>_trial_<NNN>_run_<MMMM>_<YYYYMMDD-HHMMSS>.json   one per trial
```

- Native writes the tree directly under `./out/logs/`.
- Web emits the same tree inside a single ZIP downloaded by the browser
  when the session completes.
- `MMMM` is the in-session trial run counter; `KKK` is the in-session level
  run counter; both are zero-padded.
- `<HHMMSS>` is the folder name for the level run start time.
- `<YYYYMMDD-HHMMSS>` is the start time embedded in the filenames.

### Level file (`*_summary_*.json`)

One per level run, written atomically after each trial and finalized when
the level ends (or when the controller is interrupted).

| Field                | Purpose                                                                 |
|----------------------|-------------------------------------------------------------------------|
| `participant`, `level_index`, `level_name`, `level_run_counter` | identity |
| `session_info`       | mirrors the per-trial block (see below)                                 |
| `level_config`       | frozen copy of the level spec at run time                               |
| `timestamp_start`/`timestamp_end`, `duration_s` | level-run wall clock                |
| `level_completed`    | `"completed"` / `"interrupted"` / `null` (in progress)                  |
| `trials`             | list of `{trial_index_in_chain, active_chain, trial_run_counter, outcome, nr_attempts, elapsed_time, win_event, file}` — `file` points to the per-trial JSON in the same folder |
| `outcomes`           | `{advance: N, stay: N, retroceed: N}`                                   |
| `total_attempts`     | sum of `nr_attempts` over the level run                                 |
| `chain_idxs_end`     | per-chain index at level end; useful as a resume hint                   |
| `timing_health`      | `{present_dt_mean_ms, present_dt_std_ms, render_gaps, freeze_events, drift_max_s}` |
| `prev_file`, `next_file` | basenames of the adjacent summary files; null at session ends       |

The verifier skips files matching `*_summary_*.json` because they're not
per-trial records.

### Per-trial file

Each trial writes one JSON file (or one entry in the web ZIP). Three
top-level blocks: **session metadata** (constant across trials in a
session), **trial header** (per trial), and **per-frame state** (one row
per game tick the controller saw).

### Session metadata — `session_info`

Captured once when the controller starts; copied into every trial log so the
file is self-describing.

| Field                 | Source                    | Why it matters                                                       |
|-----------------------|---------------------------|----------------------------------------------------------------------|
| `app_start_unix_ns`   | `time.time_ns()` / `Date.now()*1e6` at startup | Anchors `present_elapsed_secs` (relative) to Unix time for cross-session/-machine alignment |
| `platform`            | `"native"` or `"wasm"`    | Distinguishes Python-controller runs from web runs                   |
| `os` (native only)    | `sys.platform`            | Identifies the OS/compositor stack                                   |
| `user_agent` (web only) | `navigator.userAgent`   | Identifies the browser + version                                     |
| `refresh_rate_hz`     | `shared::REFRESH_RATE_HZ` | Target fixed-timestep rate (60 Hz today)                             |
| `cross_origin_isolated` (web) | `self.crossOriginIsolated` | If `false`, `present_elapsed_secs` precision is ~1 ms; if `true`, ~5 µs |
| `present_mode`        | `"fifo"`                  | Render present mode used by Bevy (vsync-locked)                      |

### Trial header (top-level keys alongside `frames`)

| Field                 | Source                                             | Purpose                                  |
|-----------------------|----------------------------------------------------|------------------------------------------|
| `session_name` (web)  | landing-page input → `localStorage.session_name`   | Participant / session identifier         |
| `level_index`, `active_chain`, `trial_index_in_chain`, `trial_run_counter` | controller FSM bookkeeping | Locate this trial within the session     |
| `trial_config`        | the controller's flattened trial spec              | Inputs that produced this trial          |
| `outcome`             | controller FSM decision                            | `advance` / `stay` / `retroceed`         |
| `nr_attempts`         | controller counter                                 | How many alignment attempts in trial     |
| `elapsed_time`        | controller wall clock (`time.time()` / `Date.now()`) | Trial duration                         |
| `timestamp_start` / `timestamp_end` | OS calendar clock                    | When the trial happened                  |
| `win_event`           | first frame where `state.win_elapsed_secs != 0`    | `{win_elapsed_secs, frame_number, present_elapsed_secs}` or `null` |

### Per-frame `state_read` (one row per game tick)

Every value is read from shared memory; the game writes it, the controller
copies into the log. **Two frame counters with distinct meanings.**

| Field                 | Written by game in        | Meaning                                                                  |
|-----------------------|---------------------------|--------------------------------------------------------------------------|
| `frame_number`        | `FixedPostUpdate` (60 Hz fixed) | Simulation-tick index. Determines game-logic time. Independent of the display refresh rate |
| `render_frame_number` | `Update` (one per vsync / rAF) | Render-frame index. Counts what the display actually drew. A gap here = a dropped render |
| `present_elapsed_secs`| `First` of the *next* frame    | Wall-clock seconds since app start, sampled when the previous frame's pixels were latched by the compositor. The closest software proxy for photon-onset time |
| `photodiode_white`    | `Update` (staged), committed in `First` | Logical state of the photodiode calibration square at submit time |
| `camera_radius`, `camera_position` | `FixedPostUpdate`     | Pose at the simulation tick                                              |
| `cosine_alignment`, `current_angle` | `FixedPostUpdate`    | Camera-vs-target-door alignment (the angle is `acos` of the cosine)      |
| `nr_attempts`         | `FixedUpdate` (game-side) | Game's authoritative alignment-attempt counter                           |
| `is_animating`        | `FixedUpdate`             | True during door-open animations (game-logic clock is frozen here)       |
| `is_blank`            | `FixedUpdate`             | True while the blank-screen entity exists                                |
| `is_rendering_stopped`| `FixedUpdate`             | Mirror of `GameConditions.stop_rendering`. Lets you tell intentional pauses apart from real stalls |

### What scenarios the schema covers

| Question                                          | How the log answers it                                                            |
|---------------------------------------------------|-----------------------------------------------------------------------------------|
| Was the display 60 Hz / 120 Hz / VRR?             | `mean` and `std` of `diff(present_elapsed_secs)`                                  |
| Were renders dropped?                             | Gaps in `render_frame_number`                                                     |
| Did the fixed-step simulation fall behind?        | `frame_number` Δ > 1 across consecutive rows                                      |
| Was there an intentional pause vs a stall?        | `is_rendering_stopped` / `is_animating` true → intentional. Frozen counters with both false → stall |
| When did the trial succeed?                       | `win_event.present_elapsed_secs` and `win_event.frame_number`                     |
| How does this session relate to others / external recordings? | `session_info.app_start_unix_ns + present_elapsed_secs` = Unix-time photon proxy |
| Web-only: is timing precision µs or ms today?     | `session_info.cross_origin_isolated`                                              |

### What is **not** in the log (and where to get it)

- **Absolute photon-onset time on a neural-acquisition clock** — must come from a photodiode trace recorded on the neural amplifier (or a TTL strobe). Software cannot measure compositor + scanout latency.
- **Pixel response time** — physical display property; absorbed into the photodiode `T_offset`.