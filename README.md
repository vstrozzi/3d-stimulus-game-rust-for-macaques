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
2. Gzip the wasm (decompressed in JS via `DecompressionStream`; GitHub Pages doesn't gzip `.wasm`):
   `gzip -9 -k -f game_node/pkg/game_node_bg.wasm`
3. npx terser controller_main.js -c drop_console=true,drop_debugger=true -m -o deploy_frontend/controller_main.min.js # min version served by the frontend
3. Launch (replace in game.html the right controller file)

## How to create levels

Create Custom levels by using trials_editor.html


### Prepare Textures from ambientCG for bevy

python game_node/src/scripts/prepare_bevy_textures.py ./Metal061B_1K-JPG


### Prepare / equalize sounds

When you add or replace any audio in `game_node/assets/sounds/`

python game_node/src/scripts/equalize_audio.py game_node/assets/sounds


### Verify run

python tools/verify_trial_logs.py out/trial_logs/


## Hosted web server (per-trial logging)

Instead of static hosting, `deploy_backend/log_server.py` serves the
`deploy_frontend/` bundle behind a password gate and receives one POST per
trial, writing it to `out/server_logs/` in the exact same folder format
`controller.py` produces. (`deploy_frontend/` holds `index.html`, `login.html`,
`controller_main.min.js`, and symlinks `game_node`, `assets`, `trials_config`
back into the repo so nothing is duplicated and the build paths stay intact.)

Two passwords map to two roles:
* **player** → name → instructions → play; trials are uploaded after each trial.
* **admin** → editor + a name field (for test play) + `upload_trial` (saves a
  `.jsonl` to the library) + `select_trial` (a popup to pick / rename / delete a
  saved trial, or "★ default" it) + a "Make selected the default" button + a
  popup that browses the server data folder (view a file, or download the
  current folder as a server-zipped `.zip`).

The trial library lives in `trials_config/trials/`; the active default is
`trials_config/trials/trials.jsonl` (read by `controller.py`, the web
controller, and the editor). "Make it default" backs up the current
`trials.jsonl` → `trials_old_default.jsonl` and copies the selected one in.

There is no client-side ZIP: the browser keeps each trial in memory only until
the server confirms it (HTTP 200), then drops it. If trials remain unsent at
the end of a game, the player is shown how many are pending and the sender
keeps retrying until they upload.

### Run locally (no TLS — `http://localhost` is already a secure context)
Run from the repo root (so `deploy_backend` imports and the symlinks resolve):
```bash
pip install -r deploy_backend/requirements-server.txt
export SECRET_KEY=$(python -c "import secrets;print(secrets.token_hex(32))")
export PLAYER_PW_HASH=$(python -c "from passlib.hash import argon2;print(argon2.hash('PLAYERPW'))")
export ADMIN_PW_HASH=$(python -c "from passlib.hash import argon2;print(argon2.hash('ADMINPW'))")
python -m uvicorn deploy_backend.log_server:app --port 8000
# open http://localhost:8000  → log in with PLAYERPW (play) or ADMINPW (browse data)
```
The passwords are whatever plaintext you hash above; only the hashes are stored
(in these env vars / the systemd unit), never the plaintext.

### Deploy (HTTPS required for SharedArrayBuffer)
Run on a VM with a **persistent disk** (where `out/server_logs/` lives).
`deploy_backend/monkey-log-server.service` runs uvicorn under systemd;
`deploy_backend/Caddyfile` puts Caddy in front for automatic HTTPS. Back up
`out/server_logs/` (e.g. nightly `rsync`). Read the data with
`python tools/verify_trial_logs.py out/server_logs/<date>/<name>/`. On disk the
layout is
`out/server_logs/<date>/<name>/<name>_<YYYY-MM-DD_HH-MM-SS>/level_NNN/<HHMMSS>/trials/…`
(date first, so a day folder holds every player of that day; each play session
gets its own `<name>_<timestamp>` folder, so repeat plays never collide).