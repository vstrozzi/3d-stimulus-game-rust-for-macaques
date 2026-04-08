    # monkey_3d_game — Project Structure

A Bevy-based 3D game for psychology/neuroscience research. A participant rotates a pyramid to align
it with a target door. A Python controller drives the game loop, logs trial data, and reads game
state — all via shared memory IPC.

---

## Workspace layout

```
monkey_3d_game/
├── shared/                  # Rust crate: IPC types, constants, Python bindings
├── game_node/               # Rust crate: Bevy game (native binary + WASM cdylib)
├── controller_python/       # Python controller + compiled .so
│   ├── controller.py        # Main trial state machine
│   ├── monkey_shared.so     # Built from shared/ via maturin
│   └── trials.jsonl         # Trial definitions (one JSON object per line)
├── trials_config/
│   ├── trials.jsonl         # Source trial definitions
│   └── trial_editor.html    # Browser UI for editing/creating trials
├── out/trial_logs/          # Per-run JSON logs written by the controller
├── assets/                  # Bevy assets (textures, etc.)
├── index.html               # WASM host page
└── controller_main.js       # JS glue for WASM controller path
```

---

## `shared/` crate

Cross-platform IPC layer. Compiled twice: once as a Rust lib (used by `game_node`), once as a
Python extension (`.so`) via pyo3/maturin.

### Files

| File | Purpose |
|---|---|
| `src/lib.rs` | `SharedMemory`, `SharedCommands`, `SharedGameStructure` — all `#[repr(C)]` with atomic fields |
| `src/constants.rs` | Game constants: `REFRESH_RATE_HZ = 60.0`, camera speeds, object sizes, etc. |
| `src/native.rs` | File-backed `mmap(MAP_SHARED)` on Linux. `create_shared_memory` (game node) / `open_shared_memory` (controller) — split to avoid SIGBUS from truncation race |
| `src/python.rs` | pyo3 bindings: exposes `SharedMemoryWrapper` to Python via `monkey_shared.so` |
| `src/web.rs` | WASM shared memory using a JS-side `SharedArrayBuffer` |

### Shared memory layout

```
SharedMemory {
    commands:               SharedCommands        // Controller → Game
    game_structure_control: SharedGameStructure   // Controller → Game (trial config)
    game_structure_game:    SharedGameStructure   // Game → Controller (live state)
}
```

All fields are atomic (`AtomicBool`, `AtomicU32`, `AtomicU64`) — safe for cross-process access
without locks.

---

## `game_node/` crate

Bevy 0.18 game. Builds as:
- Native binary (`main.rs`) for the research lab setup
- WASM `cdylib` for browser demos (`wasm_main()` entry point in `lib.rs`)

### Entry points

| File | Purpose |
|---|---|
| `src/main.rs` | Native entry: calls `build_app().run()` |
| `src/lib.rs` | App builder shared by native + WASM. Configures plugins, window (`PresentMode::Fifo`), and registers all resources |

### `src/shared_memory/`

| File | Purpose |
|---|---|
| `shared_memory_reader.rs` | `SharedMemResource` (Bevy resource wrapping the SHM handle). Reads commands from SHM into `PendingCommands` each `PreUpdate`. Initialises SHM at `Startup` |
| `shared_memory_writer.rs` | Writes live game state back to SHM each `PostUpdate`. Tracks `frame_number` and `elapsed_secs` (wall-clock via `time.delta()`) |
| `shared_memory_web_extension.rs` | WASM adapter: polls JS `SharedArrayBuffer` instead of mmap |

### `src/utils/`

| File | Purpose |
|---|---|
| `systems_logic.rs` | Central plugin. Wires all systems into Bevy schedules (`Startup`, `PreUpdate`, `Update`, `PostUpdate`) |
| `objects.rs` | All Bevy resource/component structs: `GameStateLocal`, `GameConditions`, `PendingCommands`, `DoorWinEntities`, `PyramidConfig`, etc. |
| `setup.rs` | `setup_environment` (static scene: ground, wall, lights). `setup_round` (per-trial reset: despawns entities, reads SHM config, spawns pyramid). `build_pyramid_config` extracts `PyramidConfig` from SHM state |
| `pyramid.rs` | All pyramid mesh generation: faces, top lid, door frame, decorations. `spawn_pyramid(config: &PyramidConfig)` is the main entry point |
| `decorations.rs` | Decoration mesh builders (circle, square, star, triangle) used by `pyramid.rs` |
| `camera.rs` | Spawns the persistent `Camera3d`. Handles rotation and zoom via `PendingCommands` |
| `handle_commands.rs` | One system per command type: reset, blank screen, stop rendering, rotation, zoom, alignment check, door animation |
| `game_functions.rs` | Door win animation logic, score bar updates |
| `ui.rs` | Score bar UI (dot chain at top of screen), `despawn_ui` |
| `load_textures.rs` | Preloads all textures at startup. `check_scene_ready` polls until GPU upload is confirmed |
| `utils.rs` | Mesh helpers (`build_mesh`), `spawn_blank_screen`, `despawn_all_game_and_ui` |
| `debug_functions.rs` | Keyboard shortcut to toggle vsync at runtime |
| `macros.rs` | `log!` macro: routes to `bevy::info!` on native, `console.log` on WASM |

### System schedule overview

```
Startup:       init_shared_memory_system, spawn_persistent_camera,
               setup_environment, preload_all_textures

PreUpdate:     read_shared_memory_commands, read_shared_memory_game_state_local

Update:        update_ui_scale, check_scene_ready,
               handle_reset_command, handle_animation_door_command,
               handle_blank_screen, handle_stop_rendering,
               handle_rotation, handle_zoom, handle_check_alignment,
               handle_door_animation, update_score_bar

PostUpdate:    clear_pending_commands, increment_timing,
               update_shared_memory_local, write_shared_memory_game_state
```

---

## `controller_python/`

Python state machine that drives the game. Communicates with the game exclusively via
`monkey_shared.so` (the compiled `shared/` crate).

- Writes trial configs into `game_structure_control`
- Sends commands (`reset`, `rotate_left`, etc.) via `SharedCommands`
- Reads back live state from `game_structure_game` (frame number, elapsed time, camera angle, alignment)
- Logs per-frame data to `out/trial_logs/trial_NNN_run_MMMM.json`

**Important:** the controller calls `open_shared_memory` (not `create`) — it attaches to the
existing mmap file created by the game node, without truncating it. This avoids a SIGBUS race where
truncating the file while the game node has it mapped causes a bus error.

---

## Build targets

| Target | Command | Output |
|---|---|---|
| Native debug | `cargo run -p game_node` | Binary |
| Native release | `cargo build -p game_node --release` | Binary |
| WASM release | `wasm-pack build` or custom profile `wasm-release` | `.wasm` + JS bindings |
| Python `.so` | `maturin develop` (in `shared/`) | `monkey_shared.so` |

### Cargo profiles

| Profile | Use |
|---|---|
| `dev` | Fast iteration (deps at `opt-level=3`, crate at 1) |
| `release` | Native production (`lto=thin`, single codegen unit) |
| `wasm-release` | WASM production (`lto=fat`, `opt-level=3`, stripped, `panic=abort`) |
| `wasm-dev` | WASM debug (no LTO, incremental, debug info) |

---

## Key design decisions

- **Atomic shared memory** over sockets/pipes: zero-copy, no serialization, minimal latency between Python controller and Bevy game
- **`create` vs `open` split** in `native.rs`: game node creates (truncates + initialises), controller opens (read-only attach with size check) — prevents SIGBUS
- **`PyramidConfig` struct**: replaces a 15-parameter function signature for `spawn_pyramid`
- **`PresentMode::Fifo`**: hard vsync, no tearing on native (unlike `AutoVsync` which can silently degrade on X11)
- **`elapsed_secs` via `time.delta()`**: wall-clock accurate regardless of frame rate; paused intentionally when `stop_rendering` is true
