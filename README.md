# Monkey 3D Game - Native and WASM Versions (Decoupled Game Instance and Controller)

A simple environmental 3D game designed to analyze the learning of 3D world models across macaques, teenagers with autism, and ML models. The game consists of distinguishing and learning the shapes of two structures, Type 1 and Type 2, with one level per pair composed of many trials. No indications must be provided to the players. The game logic here (`game_node`) is decoupled from the controller (`controller_py` or `web/controller_main.js`, which use a simple FSM to handle the trials and define the learning phases across each level). Depending on whether it is running on WASM or natively, the two instances communicate via: (1) two processes using Linux shared memory natively (`/tmp`), or (2) the same memory region on the web. We use Bevy to provide the same instance across web and native environments, as we require specialized hardware for the monkeys and broad distribution for patient/tablet usage.

This architecture allows for extremely low-latency, lock-free communication between the game and external controllers, supporting multiple languages and platforms.

## Architecture

```
      Controller                 Shared Memory (lock-free)              Game (Bevy)
  ┌─────────────────┐        ┌──────────────────────────────┐      ┌─────────────────┐
  │ controller.py   │  write │ commands      (ctrl → game)   │ read │ game_node       │
  │   or            │───────▶│ control state (ctrl → game)   │─────▶│  reads cmds,    │
  │ controller_     │        │                               │      │  steps world,  │
  │   main.js (FSM) │◀───────│ game state + 8-slot ring buf  │◀─────│  writes state  │
  └─────────────────┘  read  │   (game → ctrl, per frame)    │ write└─────────────────┘
                             └──────────────────────────────┘
   Native: 2 OS processes, region = mmap'd file in /tmp
   Web:    1 thread,       region = SharedArrayBuffer (controller + WASM game share it)
```

The two sides never call each other. They only read/write atomics in one shared
region: the controller pushes **commands + next-trial config**; the game pushes a
**state snapshot every frame** (plus an 8-slot ring buffer so the controller can
drain frames it polled past). No locks, no IPC syscalls on the hot path — just
atomic loads/stores. `repr(C)` layout means the Rust game and the Python/JS
controller agree on byte offsets. See `implementation.md §3` for the field table.

**Single vsync clock domain.** Every game system (commands, logic, animation,
state writes) runs *once per rendered frame*, locked to the display refresh via
`PresentMode::Fifo` — no separate `FixedUpdate`. Benefits:

* **One timeline.** Logic and rendering can't drift apart, so no judder from a
  fixed step landing between two frames; the transform you simulate is the one
  you display.
* **Explicit state/present pairing.** The finished state gets a render ID in   `Last`; the render world returns that ID with a monotonic marker immediately  after wgpu `present()`. The exact snapshot and marker are then logged   together. This measures software presentation pacing, not photon onset;   compositor/scanout delay remains and the photodiode is necessary.
* **Refresh-rate independent.** The loop ticks at 60/120/144 Hz / VRR; anything
  wall-clock-stable scales by `Δt`. The measured rate is logged per session as a
  sanity check. See `implementation.md §8`.

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

The Python and web controllers both scan the complete selected `trials.jsonl`
and publish the same texture-index manifest through shared memory. The native
and WASM games wait for it, then load only those session textures plus the
always-required base and lid materials.

#### Web Controller

1. Build WASM (`wasm-pack build game_node --target web --out-dir pkg`) #add --dev for no optimizations
2. Gzip the wasm (decompressed in JS via `DecompressionStream`).
   `gzip -9 -f game_node/pkg/game_node_bg.wasm`
3. npx terser controller_main.js -c drop_console=true,drop_debugger=true -m -o deploy_frontend/controller_main.min.js # min version served by the frontend
4. Serve `deploy_frontend/` (see "Hosted web server" below).
5. To reclaim disk space after a build: rm -rf game_node/pkg/debug-analysis game_node/pkg/game_node_bg.wasm

## How to create levels

Create Custom levels by using trials_editor.html


### Prepare Textures from ambientCG for bevy

Download the `<Name>_1K-JPG.zip` from ambientCG, extract it, and run:

python game_node/src/scripts/prepare_bevy_textures.py ./Metal061B_1K-JPG

This also creates `preview.webp` and `preview_tintable.webp` (maximum 128px)
for the trial editor. To generate only those previews for existing processed
textures:

python game_node/src/scripts/prepare_bevy_textures.py --previews-only game_node/assets/textures/*

**Only `bevy_ready/` is used.** The raw ambientCG download can be deleted. Then add t the `Texture` enum in
[shared/src/lib.rs](shared/src/lib.rs). The
name must match the folder name minus the `-JPG` suffix.
Both the WASM bundle and the Python module embed the enum, so rebuild them.


### Prepare / equalize sounds

When you add or replace any audio in `game_node/assets/sounds/`

python game_node/src/scripts/equalize_audio.py game_node/assets/sounds


### Verify run

python tools/verify_trial_logs.py out/trial_logs/


## Heart rate data (optional, Polar H9)

In the web version a participant can connect a **Polar H9** chest band on the
landing page before playing. Web Bluetooth only: Chromium browsers on desktop
and Android (on Linux enable `chrome://flags/#enable-web-bluetooth`); not
Firefox, Safari or any browser on iPhone/iPad. Other bands that offer the
standard heart rate service connect too, with a warning; only the H9 is tested.

- **Source**: the band's standard Bluetooth Heart Rate Measurement
  notifications, logged unmodified (`raw` hex) and decoded. The band detects
  beats from its own ECG and sends only the result: no raw ECG, no
  accelerometer, no timestamp.
- **Rate**: one packet about once a second (not a fixed sampling rate). Each
  packet has `hr`, the band's smoothed bpm (roughly the last 15 beats), and
  `rr_ms`, the 0–2 beat-to-beat intervals since the previous packet, in steps
  of 1/1024 s (about 1 ms).
- **Time**: `arrival_elapsed_secs` is when the packet reached the browser, not
  when the heart beat; a packet is typically about 1 s old on arrival. It is in
  seconds (32-bit float) on the controller clock, the same clock as
  `check_input_event_elapsed_secs`. In the web version that clock starts at
  the game start, so both compare directly with the frames'
  `present_elapsed_secs`, and `session_info.app_start_unix_ns` plus any of
  them gives Unix time. The zero is taken just before the game engine creates
  its own clock, so controller-clock times read late against the frames by
  that short, constant moment (expected milliseconds, not measured).
- **Sessions of 2026-10-05**: recorded before this, when the controller clock
  started 1–1.5 s before the game's. To align them, take the session's median
  of `check_input_event_elapsed_secs − state_read.present_elapsed_secs` over
  the frames where the former is not null, and subtract it from every
  `arrival_elapsed_secs` and `check_input_event_elapsed_secs` (good to about
  ±40 ms).
- **Where**: each trial log has `hr_info` (device name, trial start/end on the
  same clock, units, connection events) and `hr_meas` (the packets). A trial
  log holds every packet since the previous trial log, so reading them in order
  gives the unbroken stream; the first one also holds the packets from the
  connection page, with negative times. Each level summary has
  `hr_measurements` (true if any of its trials saved a packet).
- **No band**: `hr_info` is `null` and `hr_meas` is `[]`. After a band was
  lost mid-session, `hr_info` stays an object and `hr_meas` is `[]`.
- **Strap off**: the band keeps sending for a while with the last `hr` repeated
  and then `hr: 0`, all with empty `rr_ms`. Treat only packets with RR
  intervals as real beats.
- **Size**: about 16 kB per minute, under 1 % of a session's logs.

Details: [implementation.md](implementation.md) §20.


## Hosted web server

The web application is served by a small Python server (`deploy_backend/log_server.py`)
that also receives one POST at the end of each trial and stores it it to disk. 

The stack:

```
  Browser ──HTTPS──▶ Caddy ──HTTP──▶ uvicorn ──▶ FastAPI app (log_server.py)
                     (TLS, cert)     (ASGI       (auth gate, static files,
                                      server)     /log + /admin endpoints)
                                                        │
                                                        ▼
                                                  out/server_logs/  (per-trial JSON)
```

* **FastAPI** — the app: defines the API calls for each user level (login, static bundle, `/log`, `/admin/*`).
* **uvicorn** — the ASGI (asynchronous here) implementation, that allows the server to deploy the FastAPI app and works using the HTTP protocol.
  (FastAPI is just a library; uvicorn is the process listening on a port.)
* **Caddy** — reverse proxy (server of servers)in front, terminates **HTTPS** (auto Let's Encrypt
  cert) and provides individual http request to uvicor per user. HTTPS is mandatory because `SharedArrayBuffer` needs a secure context.
* **passlib[argon2]** — hashes the two passwords (player / admin); only hashes are stored.
* **itsdangerous** — signs the auth cookie so it can't be forged (needs `SECRET_KEY`).

Auth is a single cookie gate: log in once → signed `{role}` cookie → every path is
checked. `player` plays + uploads logs; `admin` also browses/downloads the data.

### Docker (recommended)

#### Test locally
A dev `.env` (player=`player`, admin=`admin`) is provided; otherwise
`cp .env.example .env` and fill it. `http://localhost` is a secure context, so
no TLS is needed locally:
```bash
docker compose up --build        # → http://localhost:8000
```

#### Deploy on an Ubuntu VM
Prereqs: a **domain** with an A-record → the VM's IP (HTTPS is mandatory —
`SharedArrayBuffer` needs a secure context, so a bare `http://<IP>` won't work),
and ports **22/80/443** open.

```bash
# 1. install Docker (+ compose plugin) and make it start on boot
curl -fsSL https://get.docker.com | sh
sudo systemctl enable --now docker
sudo usermod -aG docker $USER && exit        # log back in so 'docker' needs no sudo

# 2. get the code
ssh user@<VM-IP>
sudo mkdir -p /srv/3d-stimulus-game-rust-for-macaques && sudo chown $USER /srv/3d-stimulus-game-rust-for-macaques
git clone <repo-url> /srv/3d-stimulus-game-rust-for-macaques
cd /srv/3d-stimulus-game-rust-for-macaques

# 3. secrets → .env  (gitignored, chmod 600)
cp .env.example .env
#   IMPORTANT: in .env each argon2 hash must have every '$' DOUBLED ('$$') —
#   docker compose eats a single '$'. These generators pre-escape it:
docker compose run --rm app python -c "import secrets;print(secrets.token_hex(32))"                                   # → SECRET_KEY (no '$', paste as-is)
docker compose run --rm app python -c "from passlib.hash import argon2;print(argon2.hash('YOUR_PLAYER_PW').replace('\$','\$\$'))"   # → PLAYER_PW_HASH
docker compose run --rm app python -c "from passlib.hash import argon2;print(argon2.hash('YOUR_ADMIN_PW').replace('\$','\$\$'))"    # → ADMIN_PW_HASH
nano .env && chmod 600 .env

# 4. domain for HTTPS
vim deploy_backend/Caddyfile.docker         # replace your.domain.com

# 5. run (detached + HTTPS)
docker compose --profile tls up -d --build
docker compose logs -f app                   # Ctrl-C stops only the log view, not the server
```

#### Always-up / crash recovery
`restart: unless-stopped` (in `docker-compose.yml`) + `systemctl enable docker`
mean: app crash → auto-restart; VM reboot / power loss → containers come back
exactly as they were. A *startup* bug crash-loops — check `docker compose logs app`.

#### Detaching from SSH
Always start with `-d` (above). Then `exit` freely — containers run under the
Docker daemon, not your SSH session. (If you ever started attached without `-d`,
`Ctrl-C` then re-run with `-d`.)

#### Updating
```bash
cd /srv/3d-stimulus-game-rust-for-macaques && git pull && docker compose --profile tls up -d --build
```
`--build` is required for code changes (frontend + server are baked into the
image). `./data/` is untouched by pulls.

#### Data & backup
Data persists on the host under `./data/` (`server_logs/` + `trials/` bind
mounts), owned by **root** (the container writes as root — use `sudo` to read it).
Set up **one rolling copy every 24 h** in **root's** crontab (root, so it can
read the root-owned data):
```bash
sudo mkdir -p /srv/backup
sudo crontab -e
# add:
0 3 * * * rsync -a --delete /srv/3d-stimulus-game-rust-for-macaques/data/server_logs/ /srv/backup/server_logs/ >> /var/log/monkey-backup.log 2>&1
```
Test it immfediately: `sudo rsync -av --delete /srv/3d-stimulus-game-rust-for-macaques/data/server_logs/ /srv/backup/server_logs/`.
`--delete` keeps exactly one mirror (no history). It's on the same disk, so for
real safety point the destination at a second volume / another host / R2.
Read the data with `python tools/verify_trial_logs.py data/server_logs/<date>/<name>/`.

### Run locally without Docker (no TLS — `http://localhost` is a secure context)
Run from the repo root (so `deploy_backend` imports and the symlinks resolve):
```bash
pip install -r deploy_backend/requirements-server.txt
export SECRET_KEY=$(python -c "import secrets;print(secrets.token_hex(32))")
export PLAYER_PW_HASH=$(python -c "from passlib.hash import argon2;print(argon2.hash('PLAYERPW'))")
export ADMIN_PW_HASH=$(python -c "from passlib.hash import argon2;print(argon2.hash('ADMINPW'))")
python -m uvicorn deploy_backend.log_server:app --port 8000
# open http://localhost:8000  → log in with PLAYERPW (play) or ADMINPW (browse data)
```
The passwords are whatever plaintext you hash above.
