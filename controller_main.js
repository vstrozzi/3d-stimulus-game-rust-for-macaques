// ==========================================================================
// Web Controller – 1:1 port of controller_python/controller.py
// ==========================================================================
//
// Architecture:
//   Main thread  – controller FSM + input + UI overlay
//   WASM/Bevy    – runs on the same thread via winit/requestAnimationFrame
//   Communication – WASM linear memory (SharedMemory struct)
//
// Shared memory layout (see shared/src/lib.rs):
//   SharedMemory {
//     commands:                SharedCommands,   // Controller → Game
//     game_structure_game:    SharedGameState,   // Game → Controller
//     game_structure_control: SharedGameState,   // Controller → Game (config)
//   }
// ==========================================================================

import init, {
  create_shared_memory_wasm,
  WebSharedMemory,
  wasm_main,
  refresh_rate_hz,
  controller_constants,
} from "./game_node/pkg/game_node.js";


// ── Constants ──────────────────────────────────────────────────────────────
// All values below are populated from Rust (shared/src/constants.rs) after
// init() — controller.py reads the same source via PyO3, so both controllers
// stay in lockstep.
let REFRESH_RATE_HZ = null;
let N_FACES = null;
let N_COLOR_FLOATS = null;
let N_START_ORIENTS = null;
let START_ORIENTS = null;
let COLOR_SUGGESTION_COS_SIM = null;
let GAME_UNRESPONSIVENESS_THRESHOLD_S = null;
let DEFAULT_CAMERA_Y = null;
let CAMERA_3D_INITIAL_RADIUS = null;
let LOGGED_STATE_FIELDS = null;       // Set<string>
let CONTROLLER_META_FIELDS = null;    // Set<string>
let FSM_STATES = null;                // string[]
let PROCEEDING_VALUES = null;         // string[]

const APP_START_UNIX_NS = Date.now() * 1_000_000;

const TRIALS_PATH = "./trials_config/trials.jsonl";

// ── FSM States and proceeding outcomes ─────────────────────────────────────
// Populated from Rust at startup so the labels stay in lockstep with
// controller.py's ControllerState / TrialProceeding enums.
const FSM = {};
const PROCEEDING = {};

// ── Global State ───────────────────────────────────────────────────────────
let memory = null; // WASM Memory
let sharedMem = null; // WebSharedMemory wrapper
let pointers = { cmd: 0, gsGame: 0, gsControl: 0, ringWriteHead: 0, ringEntries: 0, commandSeq: 0, commandAck: 0 };
let ringEntryStride = 0;
let ringBufferSize = 0;
let lastWriteHead = 0;
let offsets = {}; // field offsets within SharedGameState
let cmdOffsets = {}; // field offsets within SharedCommands
let defaultGameState = null; // default SharedGameState values (from Rust)

// Levels / chains (mirrors Python's multi-level multi-chain model)
let levels = [];
let currentLevelIndex = 0;
let chainIdxs = [];      // one entry per object (chain)
let activeChain = 0;

// FSM
let fsmState = FSM.INIT;
let trialProceeding = PROCEEDING.ADVANCE;

// Continuous inputs (from keyboard/touch) – mirrors Python's self.inputs
let inputs = {
  rotate_left: false,
  rotate_right: false,
  zoom_in: false,
  zoom_out: false,
};

// One-shot triggers – mirrors Python's self.triggers
let triggers = {
  check: false,
  reset: false,
  toggle_blank: false,
  toggle_stop_rendering: false,
  animation_door: false,
  animation_all_door: false,
  animation_colored: false,
};

// Command sequence counter (mirrors Python's shm_wrapper.command_seq_counter)
let commandSeqCounter = 0;

// Per-trial tracking
let nrAttempts = 0;
let trialStartTime = 0;
let trialStartOrient = null;
let frameLog = {};
let frameZero = null;
let renderFrameZero = null;
let winEvent = null;
let trialRunCounter = 0;

// Per-level-run tracking — mirrors controller.py
let levelRunCounter = 0;
let currentLevelSummary = null;
let lastSummaryFilename = null;
const levelSummaries = [];
let currentFrame = -1;
let gameTimeUnresponsive = 0;

// Special flags
let _start = false;
let _playingStartTime = 0;  // Date.now() when FSM enters PLAYING — used for tap grace period
let _running = false;
let _sceneReadyPromptShown = false;
let _downloadPopupShown = false;

// All accumulated trial logs (for download)
let allTrialLogs = [];

// Pressed keys tracking (to detect one-shot key presses)
let pressedKeys = new Set();

// ── Touch tuning ────────────────────────────────────────────────────────────
const SWIPE = {
  velocityWindow: 100,  // ms — sliding window for velocity from recent samples (replaces EMA)
  maxEnergy: 2000,      // px/s — cap for stored coast energy (hard flicks bank long spin-time)
  dutyMaxEnergy: 600,   // px/s — velocity at which duty saturates to 1.0 (responsive drag)
  friction: 0.993,      // fixed per-frame @ 60fps → half-life ~100 frames (~1.6s of decay)
  minEnergy: 15,        // px/s → below this, snap to zero cleanly
  coastCutoff: 0.08,    // duty below this during coast → snap off (avoids low-duty PWM stutter)
  tapMaxMove: 10,
  tapMaxTime: 300,
  referenceFPS: 60,     // friction exponent is calibrated to this rate
};

// ── Swipe state (single finger → rotation) ──────────────────────────────────
let swipe = {
  active: false,
  startX: 0, startY: 0,
  startTime: 0,
  samples: [],      // ring of {v: clientX, t: ms} within velocityWindow
  velocity: 0,      // windowed px/s during drag (computed from samples)
  energy: 0,        // px/s after release (decays via friction)
  accumulator: 0,
  rotateLeft: false,
  rotateRight: false,
};

// ── Pinch state (two fingers → zoom) ────────────────────────────────────────
let pinch = {
  active: false,
  wasPinching: false,   // suppresses false tap after zoom gesture
  samples: [],          // ring of {v: dist, t: ms} within velocityWindow
  velocity: 0,
  energy: 0,
  accumulator: 0,
  zoomIn: false,
  zoomOut: false,
};

// ── Framerate-independent friction tracking ─────────────────────────────────
let lastProcessTime = 0;

// ═══════════════════════════════════════════════════════════════════════════
// UTILITY: float ↔ u32 bit conversion
// ═══════════════════════════════════════════════════════════════════════════
const _f32Buf = new ArrayBuffer(4);
const _f32View = new Float32Array(_f32Buf);
const _u32View = new Uint32Array(_f32Buf);

function floatToU32Bits(f) {
  _f32View[0] = f;
  return _u32View[0];
}
function u32BitsToFloat(u) {
  _u32View[0] = u;
  return _f32View[0];
}

// ═══════════════════════════════════════════════════════════════════════════
// LOADING OVERLAY HELPERS
// ═══════════════════════════════════════════════════════════════════════════

function setLoadingStep(label) {
  const el = document.getElementById("loading-step");
  if (el) el.textContent = label;
}

function setLoadingProgress(pct) {
  const fill = document.getElementById("loading-bar-fill");
  const pctEl = document.getElementById("loading-pct");
  if (!fill) return;
  if (pct < 0) {
    // indeterminate
    fill.classList.add("indeterminate");
    if (pctEl) pctEl.textContent = "";
  } else {
    fill.classList.remove("indeterminate");
    fill.style.width = `${pct}%`;
    if (pctEl) pctEl.textContent = pct < 100 ? `${pct}%` : "";
  }
}

function hideLoadingOverlay() {
  const el = document.getElementById("loading-overlay");
  if (el) el.style.display = "none";
}

// Fetch a URL while reporting download progress.
// onProgress(loadedBytes, totalBytes) — totalBytes is 0 if Content-Length is absent.
// Returns an ArrayBuffer of the full response body.
async function fetchWithProgress(url, onProgress) {
  const resp = await fetch(url);
  if (!resp.ok) throw new Error(`Fetch failed: ${url} (${resp.status})`);
  const total = parseInt(resp.headers.get("content-length") || "0", 10);
  const reader = resp.body.getReader();
  const chunks = [];
  let received = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    onProgress(received, total);
  }
  const buffer = new Uint8Array(received);
  let pos = 0;
  for (const chunk of chunks) { buffer.set(chunk, pos); pos += chunk.length; }
  return buffer.buffer;
}

// ═══════════════════════════════════════════════════════════════════════════
// SHARED MEMORY READ/WRITE HELPERS
// ═══════════════════════════════════════════════════════════════════════════

/** Read game_structure_game → JS object (mirrors Python's read_game_state) */
function readGameState() {
  return readGameStateAt(pointers.gsGame);
}

/** Helper to read u64 from DataView (little-endian) */
function readU64(view, offset) {
  const lo = view.getUint32(offset, true);
  const hi = view.getUint32(offset + 4, true);
  return hi * 0x100000000 + lo;
}

/** Write u64 to DataView (little-endian) */
function writeU64(view, offset, val) {
  view.setUint32(offset, val & 0xFFFFFFFF, true);
  view.setUint32(offset + 4, Math.floor(val / 0x100000000) & 0xFFFFFFFF, true);
}

/** Read a single game state from a given base pointer (reuses offsets). */
function readGameStateAt(basePtr) {
  const v = new DataView(memory.buffer, basePtr);
  const o = offsets;
  return {
    base_radius: v.getUint32(o.base_radius, true),
    height: v.getUint32(o.height, true),
    start_orient: v.getUint32(o.start_orient, true),
    target_door: v.getUint32(o.target_door, true),
    colors: Array.from({ length: N_COLOR_FLOATS }, (_, i) => v.getUint32(o.colors + i * 4, true)),
    decorations_count: Array.from({ length: N_FACES }, (_, i) => v.getUint32(o.decorations_count + i * 4, true)),
    decorations_size: Array.from({ length: N_FACES }, (_, i) => v.getUint32(o.decorations_size + i * 4, true)),
    decorations_seeds: Array.from({ length: N_FACES }, (_, i) => readU64(v, o.decorations_seeds + i * 8)),
    decorations_shape: Array.from({ length: N_FACES }, (_, i) => v.getUint32(o.decorations_shape + i * 4, true)),
    // SHM stores i32 in u32 slots; expose as signed to JS so -1 (random)
    // round-trips through the trial log without surprise.
    decorations_rotation: Array.from({ length: N_FACES }, (_, i) => v.getInt32(o.decorations_rotation + i * 4, true)),
    camera_rotation_sense: v.getInt32(o.camera_rotation_sense, true),
    cosine_alignment_threshold: v.getUint32(o.cosine_alignment_threshold, true),
    door_anim_fade_out: v.getUint32(o.door_anim_fade_out, true),
    door_anim_stay_open: v.getUint32(o.door_anim_stay_open, true),
    door_anim_fade_in: v.getUint32(o.door_anim_fade_in, true),
    main_spotlight_intensity: v.getUint32(o.main_spotlight_intensity, true),
    ambient_brightness: v.getUint32(o.ambient_brightness, true),
    max_spotlight_intensity: v.getUint32(o.max_spotlight_intensity, true),
    frame_number: readU64(v, o.frame_number),
    render_frame_number: readU64(v, o.render_frame_number),
    elapsed_secs: u32BitsToFloat(v.getUint32(o.elapsed_secs, true)),
    render_elapsed_secs: u32BitsToFloat(v.getUint32(o.render_elapsed_secs, true)),
    present_elapsed_secs: u32BitsToFloat(v.getUint32(o.present_elapsed_secs, true)),
    photodiode_white: v.getUint8(o.photodiode_white) !== 0,
    camera_radius: u32BitsToFloat(v.getUint32(o.camera_radius, true)),
    camera_x: u32BitsToFloat(v.getUint32(o.camera_x, true)),
    camera_y: u32BitsToFloat(v.getUint32(o.camera_y, true)),
    camera_z: u32BitsToFloat(v.getUint32(o.camera_z, true)),
    camera_speed_rotate: u32BitsToFloat(v.getUint32(o.camera_speed_rotate, true)),
    attempts: v.getUint32(o.attempts, true),
    current_alignment: u32BitsToFloat(v.getUint32(o.current_alignment, true)),
    current_angle: u32BitsToFloat(v.getUint32(o.current_angle, true)),
    is_animating: v.getUint8(o.is_animating) !== 0,
    is_blank: v.getUint8(o.is_blank) !== 0,
    is_rendering_stopped: v.getUint8(o.is_rendering_stopped) !== 0,
    is_scene_ready: v.getUint8(o.is_scene_ready) !== 0,
    win_elapsed_secs: u32BitsToFloat(v.getUint32(o.win_time, true)),
  };
}

/**
 * Read all game states written since lastWriteHead.
 * Returns { newHead, states: [...] }.
 */
function readGameStateSince() {
  const headView = new DataView(memory.buffer, pointers.ringWriteHead);
  const currentHead = readU64(headView, 0);

  if (currentHead <= lastWriteHead) {
    return { newHead: currentHead, states: [] };
  }

  const start = (currentHead - lastWriteHead > ringBufferSize)
    ? currentHead - ringBufferSize
    : lastWriteHead;

  const states = [];
  for (let i = start; i < currentHead; i++) {
    const idx = Number(i) % ringBufferSize;
    const entryPtr = pointers.ringEntries + idx * ringEntryStride;
    states.push(readGameStateAt(entryPtr));
  }

  return { newHead: currentHead, states };
}

/**
 * Write commands to SharedCommands (mirrors Python's write_commands).
 * Does NOT increment command_seq — that is done by incrementCommandSeq()
 * in the main loop after game state is also written.
 * @param {Object} cmds - { rotate_left, rotate_right, zoom_in, zoom_out,
 *                           check, reset, toggle_blank, toggle_stop_rendering,
 *                           animation_door, animation_all_door, animation_colored }
 */
function writeCommands(cmds) {
  const view = new Uint8Array(memory.buffer, pointers.cmd);
  const co = cmdOffsets;
  view[co.rotate_left] = cmds.rotate_left ? 1 : 0;
  view[co.rotate_right] = cmds.rotate_right ? 1 : 0;
  view[co.zoom_in] = cmds.zoom_in ? 1 : 0;
  view[co.zoom_out] = cmds.zoom_out ? 1 : 0;
  view[co.check] = cmds.check ? 1 : 0;
  view[co.reset] = cmds.reset ? 1 : 0;
  view[co.toggle_blank] = cmds.toggle_blank ? 1 : 0;
  view[co.toggle_stop_rendering] = cmds.toggle_stop_rendering ? 1 : 0;
  view[co.animation_door] = cmds.animation_door ? 1 : 0;
  view[co.animation_all_door] = cmds.animation_all_door ? 1 : 0;
  view[co.animation_colored] = cmds.animation_colored ? 1 : 0;

  // Match Python's write_commands: always reset triggers after writing
  resetTriggers();
}

/* Write all-false commands (Python's write_no_commands).*/
function writeNoCommands() {
  const view = new Uint8Array(memory.buffer, pointers.cmd);
  const co = cmdOffsets;
  view[co.rotate_left] = 0;
  view[co.rotate_right] = 0;
  view[co.zoom_in] = 0;
  view[co.zoom_out] = 0;
  view[co.check] = 0;
  view[co.reset] = 0;
  view[co.toggle_blank] = 0;
  view[co.toggle_stop_rendering] = 0;
  view[co.animation_door] = 0;
  view[co.animation_all_door] = 0;
  view[co.animation_colored] = 0;
  return { ...CMD_DEFAULTS };
}

/** Read current commands from SHM (mirrors Python's shm_wrapper.read_commands).
 *  Returns a dict with the same keys as makeCmd so logs stay schema-consistent. */
function readCommands() {
  const view = new Uint8Array(memory.buffer, pointers.cmd);
  const co = cmdOffsets;
  return {
    rotate_left: view[co.rotate_left] !== 0,
    rotate_right: view[co.rotate_right] !== 0,
    zoom_in: view[co.zoom_in] !== 0,
    zoom_out: view[co.zoom_out] !== 0,
    check: view[co.check] !== 0,
    reset: view[co.reset] !== 0,
    toggle_blank: view[co.toggle_blank] !== 0,
    toggle_stop_rendering: view[co.toggle_stop_rendering] !== 0,
    animation_door: view[co.animation_door] !== 0,
    animation_all_door: view[co.animation_all_door] !== 0,
    animation_colored: view[co.animation_colored] !== 0,
  };
}

/** Increment command_seq so the game knows there are new commands.
 *  Mirrors Python's shm_wrapper.increment_command_seq(). */
function incrementCommandSeq() {
  commandSeqCounter++;
  const seqView = new DataView(memory.buffer, pointers.commandSeq);
  seqView.setBigUint64(0, BigInt(commandSeqCounter), true);
}

/**
 * Write game state config to game_structure_control.
 * Mirrors Python's write_game_state — writes ALL SharedGameState fields.
 * @param {Object} state - key→value (u32 bits for floats, u64 for seeds, etc.)
 */
function writeGameStateControl(state) {
  const v = new DataView(memory.buffer, pointers.gsControl);
  const o = offsets;

  v.setUint32(o.base_radius, state.base_radius, true);
  v.setUint32(o.height, state.height, true);
  v.setUint32(o.start_orient, state.start_orient, true);
  v.setUint32(o.target_door, state.target_door, true);

  const writeU32Array = (base, arr, n) => { for (let i = 0; i < n; i++) v.setUint32(base + i * 4, arr[i], true); };
  if (state.colors) writeU32Array(o.colors, state.colors, N_COLOR_FLOATS);
  if (state.decorations_count) writeU32Array(o.decorations_count, state.decorations_count, N_FACES);
  if (state.decorations_size) writeU32Array(o.decorations_size, state.decorations_size, N_FACES);
  if (state.decorations_shape) writeU32Array(o.decorations_shape, state.decorations_shape, N_FACES);
  if (state.textures) writeU32Array(o.textures, state.textures, N_FACES);
  if (state.decorations_texture) writeU32Array(o.decorations_texture, state.decorations_texture, N_FACES);
  if (state.decorations_thickness) writeU32Array(o.decorations_thickness, state.decorations_thickness, N_FACES);
  if (state.decorations_color) writeU32Array(o.decorations_color, state.decorations_color, N_COLOR_FLOATS);
  if (state.decorations_seeds) {
    for (let i = 0; i < N_FACES; i++) writeU64(v, o.decorations_seeds + i * 8, state.decorations_seeds[i]);
  }
  if (state.decorations_rotation) {
    // Negative values (e.g. -1 = random) are written as their two's-complement
    // u32 bits; the game reads them back via `as i32` in Rust.
    for (let i = 0; i < N_FACES; i++) {
      v.setInt32(o.decorations_rotation + i * 4, state.decorations_rotation[i] | 0, true);
    }
  }

  v.setUint32(o.cosine_alignment_threshold, state.cosine_alignment_threshold, true);
  v.setUint32(o.door_anim_fade_out, state.door_anim_fade_out, true);
  v.setUint32(o.door_anim_stay_open, state.door_anim_stay_open, true);
  v.setUint32(o.door_anim_fade_in, state.door_anim_fade_in, true);
  v.setUint32(o.main_spotlight_intensity, state.main_spotlight_intensity, true);
  v.setUint32(o.ambient_brightness, state.ambient_brightness, true);
  v.setUint32(o.max_spotlight_intensity, state.max_spotlight_intensity, true);

  v.setUint32(o.progress_bar_size, state.progress_bar_size ?? 0, true);
  v.setUint32(o.progress_bar_cur_size, state.progress_bar_cur_size ?? 0, true);

  // Dynamic fields
  if (state.frame_number !== undefined) writeU64(v, o.frame_number, state.frame_number);
  v.setUint32(o.elapsed_secs, state.elapsed_secs ?? 0, true);
  v.setUint32(o.camera_radius, state.camera_radius, true);
  v.setUint32(o.camera_x, state.camera_x, true);
  v.setUint32(o.camera_y, state.camera_y, true);
  v.setUint32(o.camera_z, state.camera_z, true);
  v.setUint32(o.camera_speed_rotate, state.camera_speed_rotate ?? 0, true);
  // i32 — `setInt32` keeps the two's-complement bit pattern for negative values.
  v.setInt32(o.camera_rotation_sense, (state.camera_rotation_sense ?? 1) | 0, true);
  v.setUint32(o.attempts, state.attempts ?? 0, true);
  v.setUint32(o.current_alignment, state.current_alignment ?? 0, true);
  v.setUint32(o.current_angle, state.current_angle ?? 0, true);

  // Booleans
  const boolView = new Uint8Array(memory.buffer, pointers.gsControl);
  boolView[o.is_animating] = state.is_animating ? 1 : 0;
  boolView[o.is_blank] = state.is_blank ? 1 : 0;
  boolView[o.is_rendering_stopped] = state.is_rendering_stopped ? 1 : 0;
  boolView[o.is_scene_ready] = state.is_scene_ready ? 1 : 0;

  v.setUint32(o.win_time, state.win_time ?? 0, true);
}

/**
 * Write current state (as returned by readGameStateAt, mixed format) to game_structure_control.
 * Mirrors Python's self.write_game_state(self.current_state) in the main loop.
 *
 * readGameStateAt returns decoded floats for: elapsed_secs, current_alignment,
 * current_angle, win_elapsed_secs. This function re-encodes them back to u32 bits.
 */
function writeCurrentStateToControl(state) {
  const s = { ...state };

  // Re-encode decoded float fields back to u32 bits
  s.elapsed_secs = floatToU32Bits(s.elapsed_secs ?? 0);
  s.current_alignment = floatToU32Bits(s.current_alignment ?? 0);
  s.current_angle = floatToU32Bits(s.current_angle ?? 0);
  s.win_time = floatToU32Bits(s.win_elapsed_secs ?? 0);

  s.camera_x = floatToU32Bits(s.camera_x ?? 0);
  s.camera_y = floatToU32Bits(s.camera_y ?? 0);
  s.camera_z = floatToU32Bits(s.camera_z ?? 0);
  s.camera_radius = floatToU32Bits(s.camera_radius ?? 0);
  s.camera_speed_rotate = floatToU32Bits(s.camera_speed_rotate ?? 0);

  writeGameStateControl(s);
}

// ═══════════════════════════════════════════════════════════════════════════
// TRIAL CONFIG HELPERS
// ═══════════════════════════════════════════════════════════════════════════

// ── Level / chain helpers (mirrors Python's properties and helpers) ─────────

function currentLevel() {
  return levels[currentLevelIndex];
}

/** Flat trial: merges object[activeChain] + fixed + trial_cfg.
 *  Mirrors Python's expand_flat_trial + flat_trial property. */
function flatTrial() {
  const level = currentLevel();
  const obj = level.objects[activeChain];
  const trialIdx = Math.min(chainIdxs[activeChain], level.trials.length - 1);
  const trialCfg = level.trials[trialIdx];
  const fixed = level.fixed;
  const flat = {};
  for (const [k, v] of Object.entries(obj)) flat[k] = v;
  for (const [k, v] of Object.entries(fixed)) {
    if (k !== "pr_switching_chain") flat[k] = v;
  }
  for (const [k, v] of Object.entries(trialCfg)) flat[k] = v;
  return flat;
}

function _trialIdx() {
  return chainIdxs[activeChain];
}
function _setTrialIdx(val) {
  chainIdxs[activeChain] = val;
}

function _levelStartTrial() {
  return currentLevel().fixed.start_trial ?? 0;
}

function _levelComplete() {
  const n = currentLevel().trials.length;
  return chainIdxs.every(idx => idx >= n);
}

function _maybeSwitch() {
  const level = currentLevel();
  const nObjects = level.objects.length;
  if (nObjects <= 1) return;
  const pr = level.fixed.pr_switching_chain ?? (1.0 / nObjects);
  if (Math.random() >= pr) return;
  const nTrials = level.trials.length;
  const candidates = [];
  for (let i = 0; i < nObjects; i++) {
    if (i !== activeChain && chainIdxs[i] < nTrials) candidates.push(i);
  }
  if (candidates.length > 0) {
    const rand = Math.floor(Math.random() * candidates.length);
    activeChain = candidates[rand];
  }
}

function _progressBarCur() {
  return chainIdxs.reduce((sum, v) => sum + Math.max(0, v), 0);
}
function _progressBarSize() {
  const level = currentLevel();
  const remaining = Math.max(0, level.trials.length);
  return remaining * level.objects.length;
}

// Mirrors Python's state_schema — drives buildTrialState conversion without a switch.
// "f32"     → floatToU32Bits(value)
// "f32[]"   → value.map(floatToU32Bits)
// "f32[][]" → value.flatMap(face => face.map(floatToU32Bits))   (nested → flat u32 array)
// "u32" / "u32[]" / "u64[]" → pass through unchanged
const FIELD_SCHEMA = {
  base_radius: "f32", height: "f32", start_orient: "f32",
  cosine_alignment_threshold: "f32",
  door_anim_fade_out: "f32", door_anim_stay_open: "f32", door_anim_fade_in: "f32",
  main_spotlight_intensity: "f32", ambient_brightness: "f32", max_spotlight_intensity: "f32",
  decorations_size: "f32[]",
  decorations_thickness: "f32[]",
  colors: "f32[][]",
  decorations_color: "f32[][]",
  target_door: "u32",
  textures: "u32[]",
  decorations_count: "u32[]",
  decorations_shape: "u32[]",
  decorations_texture: "u32[]",
  decorations_seeds: "u64[]",
  // i32 array stored as u32 bits (negative values become 0xFFFFFFFF on write
  // and are decoded back via `(u32 << 0) >> 0` ↔ raw bit cast).
  // -1 means "random rotation per decoration".
  decorations_rotation: "u32[]",
  camera_radius: "f32",
  camera_y: "f32",
  camera_speed_rotate: "f32",
  // i32 stored as u32. Valid runtime values are +1 (default) or -1.
  camera_rotation_sense: "u32",
};

/** Build a game-state object from default + trial config overlay.
 *  Mirrors Python's: default_state = read_default_game_state(); overlay(trial, default_state)
 *  Conversion is driven by FIELD_SCHEMA — add a new field there, not here.
 */
function buildTrialState(trialCfg) {
  const state = JSON.parse(JSON.stringify(defaultGameState));
  for (const [key, value] of Object.entries(trialCfg)) {
    if (CONTROLLER_META_FIELDS.has(key)) continue;
    const type = FIELD_SCHEMA[key];
    if (!type) state[key] = value;                                          // unknown: pass through
    else if (type === "f32") state[key] = floatToU32Bits(value);
    else if (type === "f32[]") state[key] = value.map(floatToU32Bits);
    else if (type === "f32[][]") state[key] = value.flatMap(face => face.map(floatToU32Bits));
    else state[key] = value;                                     // u32 / u32[] / u64[]
  }
  return state;
}

// ═══════════════════════════════════════════════════════════════════════════
// COMMAND HELPERS (mirrors Python's write_commands / reset_triggers)
// ═══════════════════════════════════════════════════════════════════════════

// All-false baseline — mirrors Python's write_no_commands dict.
// Add new command fields here; FSM handlers pick them up automatically via makeCmd().
const CMD_DEFAULTS = Object.freeze({
  rotate_left: false, rotate_right: false, zoom_in: false, zoom_out: false,
  check: false, reset: false, toggle_blank: false, toggle_stop_rendering: false,
  animation_door: false, animation_all_door: false, animation_colored: false,
});

/** Build a command object: all false except the given overrides. */
function makeCmd(overrides = {}) {
  return { ...CMD_DEFAULTS, ...overrides };
}

function resetTriggers() {
  for (const k of Object.keys(triggers)) triggers[k] = false;
}

function resetAllCommands() {
  for (const k of Object.keys(inputs)) inputs[k] = false;
  resetTriggers();
}

/** Read command_ack from SHM (game writes this after processing commands). */
function readCommandAck() {
  const v = new DataView(memory.buffer, pointers.commandAck);
  return Number(v.getBigUint64(0, true));
}

/** Check if there are unacknowledged commands pending. */
function hasPendingCommands() {
  return readCommandAck() < commandSeqCounter;
}

/** Reset seq counter to current ack (for resync after game restart). */
function resyncSeq() {
  commandSeqCounter = readCommandAck();
}

// ═══════════════════════════════════════════════════════════════════════════
// LOGGING
// ═══════════════════════════════════════════════════════════════════════════

function logFrame(stateRead, commandsSent) {
  const rawFn = Number(stateRead?.frame_number ?? currentFrame);
  const rawRfn = Number(stateRead?.render_frame_number ?? 0);

  if (frameZero === null) frameZero = rawFn;
  if (renderFrameZero === null) renderFrameZero = rawRfn;

  const loggedFn = rawFn - frameZero;
  const loggedRfn = rawRfn - renderFrameZero;

  const winSecs = stateRead?.win_elapsed_secs ?? 0;
  if (winSecs !== 0 && winEvent === null) {
    winEvent = {
      win_elapsed_secs: winSecs,
      win_frame_number: loggedFn,
      present_elapsed_secs: stateRead?.present_elapsed_secs ?? 0,
    };
  }
  const filtered = {};
  for (const [k, v] of Object.entries(stateRead)) {
    if (LOGGED_STATE_FIELDS.has(k)) filtered[k] = v;
  }
  if ("frame_number" in filtered) filtered.frame_number = loggedFn;
  if ("render_frame_number" in filtered) filtered.render_frame_number = loggedRfn;
  frameLog[String(loggedFn)] = { state_read: filtered, commands_sent: commandsSent };
}

function _participantName() {
  const raw = (localStorage.getItem("session_name") || "").trim();
  return sanitizeSessionName(raw) || "unknown";
}

function _sessionInfo() {
  return {
    app_start_unix_ns: APP_START_UNIX_NS,
    platform: "wasm",
    user_agent: (typeof navigator !== "undefined" ? navigator.userAgent : null),
    refresh_rate_hz: REFRESH_RATE_HZ,
    cross_origin_isolated: (typeof self !== "undefined" ? !!self.crossOriginIsolated : null),
    present_mode: "fifo",
  };
}

function _stampCompact(d) {
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}` +
         `-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

function _dateFolder(d) {
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function _timeFolder(d) {
  const p = (n) => String(n).padStart(2, "0");
  return `${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

function _levelName(idx) {
  return `level_${String(idx).padStart(3, "0")}`;
}

function _summaryFilename(idx, startedAt) {
  return `${_participantName()}_${_levelName(idx)}_summary_${_stampCompact(startedAt)}.json`;
}

function _trialFilename(idx, trialIdxInChain, activeChainIdx, trialRunCtr, startedAt) {
  return `${_participantName()}_${_levelName(idx)}` +
         `_trial_${String(trialIdxInChain).padStart(3, "0")}` +
         `_run_${String(trialRunCtr).padStart(4, "0")}` +
          `_object_${String(activeChainIdx).padStart(3, "0")}` +
         `_${_stampCompact(startedAt)}.json`;
}

function _levelDirRel(idx, startedAt) {
  return `${_participantName()}/${_dateFolder(startedAt)}/${_levelName(idx)}/${_timeFolder(startedAt)}`;
}

function _startLevelRunIfNeeded() {
  if (currentLevelSummary !== null) return;
  const started = new Date();
  const filename = _summaryFilename(currentLevelIndex, started);
  const { fixed: _dropFixed, ...levelCfg } = currentLevel();
  currentLevelSummary = {
    participant: _participantName(),
    level_index: currentLevelIndex,
    level_name: _levelName(currentLevelIndex),
    level_run_counter: levelRunCounter,
    session_info: _sessionInfo(),
    level_config: levelCfg,
    timestamp_start: started.toISOString(),
    timestamp_end: null,
    elapsed_time_no_anim: null,
    elapsed_time_anim: null,
    level_completed: null,
    trials_runs: [],
    timing_health: null,
    prev_file: lastSummaryFilename,
    next_file: null,
    _dir: _levelDirRel(currentLevelIndex, started),
    _filename: filename,
    _startedMs: started.getTime(),
  };
  if (lastSummaryFilename && levelSummaries.length > 0) {
    levelSummaries[levelSummaries.length - 1].next_file = filename;
  }
  levelSummaries.push(currentLevelSummary);
}

function _computeTimingHealth(summary) {
  const present = [];
  let renderGaps = 0;
  let lastRfn = null;
  for (const t of summary.trials_runs) {
    for (const fr of (t._frames_for_health || [])) {
      if (fr.present_elapsed_secs != null) present.push(fr.present_elapsed_secs);
      const rfn = fr.render_frame_number;
      if (lastRfn !== null && rfn != null && rfn - lastRfn > 1) renderGaps += rfn - lastRfn - 1;
      if (rfn != null) lastRfn = rfn;
    }
  }
  const deltas = [];
  for (let i = 1; i < present.length; i++) {
    const d = present[i] - present[i - 1];
    if (d > 0) deltas.push(d);
  }
  let mean = 0, std = 0;
  if (deltas.length > 0) {
    mean = deltas.reduce((a, b) => a + b, 0) / deltas.length;
    std = Math.sqrt(deltas.reduce((s, d) => s + (d - mean) ** 2, 0) / deltas.length);
  }
  return {
    present_dt_mean_ms: Math.round(mean * 1e6) / 1e3,
    present_dt_std_ms:  Math.round(std  * 1e6) / 1e3,
    render_gaps: renderGaps,
    freeze_events: 0,
    drift_max_s: 0.0,
  };
}

function _finalizeLevelRun(status) {
  if (currentLevelSummary === null) return;
  const end = new Date();
  currentLevelSummary.timestamp_end = end.toISOString();
  const runs = currentLevelSummary.trials_runs;
  const noAnim = runs.reduce((s, t) => s + (t.elapsed_time_no_anim || 0), 0);
  const anim = runs.reduce((s, t) => s + (t.elapsed_time_anim || 0), 0);
  currentLevelSummary.elapsed_time_no_anim = noAnim;
  currentLevelSummary.elapsed_time_anim = anim;
  currentLevelSummary.level_completed = status;
  currentLevelSummary.timing_health = _computeTimingHealth(currentLevelSummary);
  for (const t of runs) delete t._frames_for_health;
  lastSummaryFilename = currentLevelSummary._filename;
  currentLevelSummary = null;
  levelRunCounter += 1;
}

function showDownloadPopup() {
  if (_downloadPopupShown) return;
  const popup = document.getElementById("download-popup");
  if (!popup) return;
  popup.hidden = false;
  _downloadPopupShown = true;
}

function hideDownloadPopup() {
  const popup = document.getElementById("download-popup");
  if (!popup) return;
  popup.hidden = true;
  _downloadPopupShown = false;
}

function saveTrialLog(outcome) {
  _startLevelRunIfNeeded();
  const trialStartDt = new Date(trialStartTime);
  const trialEndDt = new Date();
  const filename = _trialFilename(currentLevelIndex, _trialIdx(), activeChain, trialRunCounter, trialStartDt);

  // Bucket present_elapsed_secs deltas by preceding frame's is_animating.
  const rows = Object.entries(frameLog)
    .map(([k, fr]) => [Number(k), fr])
    .filter(([_, fr]) => fr && fr.state_read)
    .sort((a, b) => a[0] - b[0]);
  let elapsedAnim = 0;
  let elapsedNoAnim = 0;
  for (let i = 1; i < rows.length; i++) {
    const prev = rows[i - 1][1].state_read;
    const cur = rows[i][1].state_read;
    const p0 = prev?.present_elapsed_secs;
    const p1 = cur?.present_elapsed_secs;
    if (p0 == null || p1 == null) continue;
    const dt = p1 - p0;
    if (dt <= 0) continue;
    if (prev.is_animating) elapsedAnim += dt;
    else elapsedNoAnim += dt;
  }

  const log = {
    level_index: currentLevelIndex,
    active_chain: activeChain,
    trial_index_in_chain: _trialIdx(),
    trial_run_counter: trialRunCounter,
    trial_config: (() => { const { start_orient: _drop, ...rest } = flatTrial(); return rest; })(),
    start_orient: trialStartOrient,
    outcome,
    nr_attempts: nrAttempts,
    elapsed_time_no_anim: elapsedNoAnim,
    elapsed_time_anim: elapsedAnim,
    timestamp_start: trialStartDt.toISOString(),
    timestamp_end: trialEndDt.toISOString(),
    session_info: _sessionInfo(),
    win_event: winEvent,
    frames: frameLog,
    _filename: filename,
    _dir: `${currentLevelSummary._dir}/trials`,
  };
  allTrialLogs.push(log);

  currentLevelSummary.trials_runs.push({
    trial_index_in_chain: _trialIdx(),
    active_chain: activeChain,
    trial_run_counter: trialRunCounter,
    outcome,
    nr_attempts: nrAttempts,
    elapsed_time_no_anim: elapsedNoAnim,
    elapsed_time_anim: elapsedAnim,
    start_orient: trialStartOrient,
    win_event: winEvent,
    file: filename,
    _frames_for_health: Object.values(frameLog).map(fr => ({
      present_elapsed_secs: fr?.state_read?.present_elapsed_secs ?? null,
      render_frame_number:  fr?.state_read?.render_frame_number ?? null,
    })),
  });
  console.log(`[LOG] Level ${currentLevelIndex} chain ${activeChain} trial ${_trialIdx()} (run ${trialRunCounter}) → ${outcome}, ${nrAttempts} attempts, no_anim=${elapsedNoAnim.toFixed(6)}s anim=${elapsedAnim.toFixed(6)}s`);
}

/** Sanitize a session name for use in a filename: strip path separators,
 *  collapse whitespace, and clamp to a sane length. Empty after cleanup → null. */
function sanitizeSessionName(raw) {
  if (!raw) return null;
  const cleaned = String(raw)
    .replace(/[\/\\:*?"<>|]+/g, "")
    .replace(/\s+/g, "_")
    .trim()
    .slice(0, 64);
  return cleaned || null;
}

/** Local ISO-ish timestamp safe for filenames (YYYY-MM-DD_HH-MM-SS, local TZ). */
function localTimestamp() {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
         `_${pad(d.getHours())}-${pad(d.getMinutes())}-${pad(d.getSeconds())}`;
}

/** Bundle every accumulated trial log into a zip with per-level folders and
 *  per-trial JSON files matching controller.py's on-disk format. After a
 *  successful download, clear the in-memory buffer so the next download only
 *  contains trials run since this point. */
async function downloadLogs() {
  if (typeof JSZip === "undefined") {
    alert("JSZip failed to load — cannot download logs as a zip.");
    return;
  }
  if (allTrialLogs.length === 0 && levelSummaries.length === 0) {
    alert("No trial logs to download yet.");
    return;
  }
  // Finalize any in-progress level run as "interrupted" so it has a footer.
  if (currentLevelSummary !== null) _finalizeLevelRun("interrupted");

  const stamp = localTimestamp();
  const baseName = `${_participantName()}_${stamp}`;
  const zip = new JSZip();

  for (const trial of allTrialLogs) {
    const path = `${baseName}/${trial._dir}/${trial._filename}`;
    const { _filename, _dir, ...clean } = trial;
    zip.file(path, JSON.stringify(clean, null, 2));
  }
  for (const summary of levelSummaries) {
    const path = `${baseName}/${summary._dir}/${summary._filename}`;
    const { _filename, _dir, _startedMs, ...clean } = summary;
    zip.file(path, JSON.stringify(clean, null, 2));
  }

  const blob = await zip.generateAsync({ type: "blob", compression: "DEFLATE" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${baseName}.zip`;
  a.click();
  URL.revokeObjectURL(url);

  allTrialLogs.length = 0;
  levelSummaries.length = 0;
  lastSummaryFilename = null;
  hideDownloadPopup();
}

// ═══════════════════════════════════════════════════════════════════════════
// CHECK HAS FINISHED (mirrors Python's check_has_finished)
// ═══════════════════════════════════════════════════════════════════════════
function checkHasFinished(state) {
  const trial = flatTrial();
  const nrAttemptsToRetroceed = trial.nr_attempts_to_retroceed ?? 0;
  const elapsedTimeToRetroceed = trial.elapsed_time_to_retroceed ?? 0;
  console.log(`[CHECK] win_elapsed_secs=${state.win_elapsed_secs} attempts=${state.attempts} elapsed_secs=${state.elapsed_secs} | thresholds: win_time=${trial.elapsed_time_to_win} nr_attempts_to_win=${trial.nr_attempts_to_win} nr_attempts_to_retroceed=${nrAttemptsToRetroceed} elapsed_time_to_retroceed=${elapsedTimeToRetroceed}`);
  return (
    state.win_elapsed_secs !== 0.0 ||
    nrAttempts > nrAttemptsToRetroceed ||
    state.elapsed_secs > elapsedTimeToRetroceed
  );
}

// ═══════════════════════════════════════════════════════════════════════════
// FSM HANDLERS (1:1 mapping from controller.py)
// ═══════════════════════════════════════════════════════════════════════════

function handleInit(state) {
  console.log("[FSM] INIT → issuing toggle_blank + toggle_stop_rendering");

  const trialCfg = flatTrial();
  console.log(`[FSM] Level ${currentLevelIndex} chain ${activeChain} trial ${_trialIdx()}:`, trialCfg);

  // Build fresh default state and overlay trial config (raw u32 format)
  const trialState = buildTrialState(trialCfg);

  // Randomise start orientation (mirrors Python's random.choice(START_ORIENTS)).
  // Editor stores a sentinel (-1) for this field; the real value is chosen
  // here and recorded into the trial log so analyses can recover it.
  const chosenStartOrient = START_ORIENTS[Math.floor(Math.random() * START_ORIENTS.length)];
  trialState.start_orient = floatToU32Bits(chosenStartOrient);
  trialStartOrient = chosenStartOrient;

  // Camera position from fixed — written directly into the SHM-layout
  // camera_x/y/z fields (no array intermediate, matches Python).
  const level = currentLevel();
  const camY = level.fixed.camera_y ?? DEFAULT_CAMERA_Y;
  const camR = level.fixed.camera_radius ?? CAMERA_3D_INITIAL_RADIUS;
  trialState.camera_x = floatToU32Bits(0.0);
  trialState.camera_y = floatToU32Bits(camY);
  trialState.camera_z = floatToU32Bits(camR);

  // Progress bar
  trialState.progress_bar_cur_size = _progressBarCur();
  trialState.progress_bar_size = _progressBarSize();

  // Read previous game state (to check is_blank / is_rendering_stopped)
  const stateOld = readGameState();

  // Merge trial state into current state (mirrors Python's self.current_state.update(trial_state))
  Object.assign(state, trialState);
  // Re-decode dynamic float fields so state stays in readGameStateAt mixed format
  // (buildTrialState returns raw u32 for these; the loop's writeCurrentStateToControl
  //  expects the decoded-float format from readGameStateAt)
  state.elapsed_secs = u32BitsToFloat(trialState.elapsed_secs ?? 0);
  state.current_alignment = u32BitsToFloat(trialState.current_alignment ?? 0);
  state.current_angle = u32BitsToFloat(trialState.current_angle ?? 0);
  state.win_elapsed_secs = u32BitsToFloat(trialState.win_time ?? 0);
  state.attempts = trialState.attempts ?? 0;

  state.camera_x = u32BitsToFloat(trialState.camera_x ?? 0);
  state.camera_y = u32BitsToFloat(trialState.camera_y ?? 0);
  state.camera_z = u32BitsToFloat(trialState.camera_z ?? 0);
  state.camera_speed_rotate = u32BitsToFloat(trialState.camera_speed_rotate ?? 0);
  state.camera_radius = u32BitsToFloat(trialState.camera_radius ?? 0);

  console.log(`[FSM] state old is_blank=${stateOld.is_blank} is_rendering_stopped=${stateOld.is_rendering_stopped}`);

  // Commands: reset + ensure blank + ensure stopped
  writeCommands(makeCmd({
    reset: true,
    toggle_blank: !stateOld.is_blank,
    toggle_stop_rendering: !stateOld.is_rendering_stopped,
  }));

  // Reset per-trial tracking
  nrAttempts = 0;
  trialStartTime = Date.now();
  frameLog = {};
  frameZero = null;
  renderFrameZero = null;
  winEvent = null;

  // Re-sync frame tracking with the game's fresh counter. handle_reset_command
  // zeros frame_number (and elapsed_secs) on the game side via setup_round.
  // Without this re-sync, lastWriteHead and currentFrame still point at
  // pre-reset ring-buffer slots, which causes a window of stale states to
  // be processed and was part of the "two timelines" artefact in trial_000.
  currentFrame = -1;
  const headView = new DataView(memory.buffer, pointers.ringWriteHead);
  lastWriteHead = readU64(headView, 0);

  fsmState = FSM.WAITING_FOR_START;
  resetAllCommands();

  // Show start overlay with loading text until is_scene_ready comes back true
  showStartOverlay(true);
  setOverlayPrompt("Loading scene & textures…", true);
  updateStatusBar(`Level ${currentLevelIndex + 1}/${levels.length} chain ${activeChain} trial ${_trialIdx() + 1}/${currentLevel().trials.length} — Loading scene…`);
  _sceneReadyPromptShown = false;
  console.log("[FSM] → WAITING_FOR_START");
}

function handleWaitingForStart(state) {
  // Block start until the game confirms all trial textures are on the GPU
  if (!state.is_scene_ready) {
    return;
  }

  // Textures just became ready — flip overlay and status bar to "Press START" (runs once per trial)
  if (!_sceneReadyPromptShown) {
    _sceneReadyPromptShown = true;
    setOverlayPrompt("Press the screen<br>or press space bar", false);
    updateStatusBar(`Level ${currentLevelIndex + 1}/${levels.length} chain ${activeChain} trial ${_trialIdx() + 1}/${currentLevel().trials.length} — Press START`);
  }

  if (_start) {
    _start = false;
    const cmds = makeCmd({ reset: true, toggle_blank: true, toggle_stop_rendering: state.is_rendering_stopped });
    writeCommands(cmds);
    fsmState = FSM.PLAYING;
    _playingStartTime = Date.now();
    logFrame(state, cmds);
    showStartOverlay(false);
    updateStatusBar(`Level ${currentLevelIndex + 1}/${levels.length} chain ${activeChain} trial ${_trialIdx() + 1}/${currentLevel().trials.length} — Playing`);
    console.log(`[FSM] START pressed → PLAYING (level ${currentLevelIndex} chain ${activeChain} trial ${_trialIdx()})`);
    return;
  }
  // Otherwise send no commands
  writeNoCommands();
}

function handlePlaying(state) {
  const trial = flatTrial();
  const timeElapsed = state.elapsed_secs;
  const hasWon = (state.win_elapsed_secs ?? 0) !== 0;

  const inWinBudget =
    timeElapsed <= (trial.elapsed_time_to_win ?? 0) &&
    nrAttempts <= (trial.nr_attempts_to_win ?? 0);

  const inStayBudget =
    !inWinBudget &&
    timeElapsed <= (trial.elapsed_time_to_retroceed ?? 0) &&
    nrAttempts <= (trial.nr_attempts_to_retroceed ?? 0);

  // Set advancement state from ground-truth win signal
  if (hasWon && inWinBudget) trialProceeding = PROCEEDING.ADVANCE;
  else if (hasWon) trialProceeding = PROCEEDING.STAY;
  else trialProceeding = PROCEEDING.RETROCEED;

  // ── Terminate if finished ──
  if (checkHasFinished(state)) {
    console.log(`[FSM] Finished with outcome ${trialProceeding} → TRIAL_COMPLETE`);
    logFrame(state, { ...inputs, ...triggers });
    fsmState = FSM.TRIAL_COMPLETE;
    return;
  }

  // ── Check triggered (space / tap) ──
  if (triggers.check) {
    const suggestionThreshold = trial.nr_attempts_suggestion ?? 0;
    const retroceedThreshold = trial.nr_attempts_to_retroceed ?? 0;
    const cosineAlignment = state.current_alignment;
    const cosineThreshold = trial.cosine_alignment_threshold ?? 0;

    let cmds;

    // Stop rendering for the duration of the animation (only toggle if currently running)
    const stopRenderForAnim = !state.is_rendering_stopped;

    // Branch 1: Last attempt AND fail → single door (no all-door, no colored), mirrors Python
    if ((nrAttempts) === retroceedThreshold && cosineAlignment < cosineThreshold) {
      console.log(`[PLAY] Attempt ${nrAttempts} == ${retroceedThreshold} → retroceed`);
      cmds = makeCmd({ check: true, toggle_stop_rendering: stopRenderForAnim, animation_door: true });
      writeCommands(cmds);
      fsmState = FSM.WAITING_ANIMATION_START;
      nrAttempts += 1;
      logFrame(state, cmds);
      return;
    }
    // Branch 2: Single hint (below suggestion & miss, OR STAY & correct)
    // colored if close enough (cosine > cos(π/6)), white otherwise
    else if (
      (nrAttempts < suggestionThreshold && cosineAlignment < cosineThreshold) ||
      (inStayBudget && cosineAlignment > cosineThreshold)
    ) {
      const coloredLight = cosineAlignment > COLOR_SUGGESTION_COS_SIM;
      console.log(`[PLAY] Attempt ${nrAttempts + 1} → hint (single ${coloredLight ? "colored" : "white"})`);
      cmds = makeCmd({ check: true, toggle_stop_rendering: stopRenderForAnim, animation_door: true, animation_colored: coloredLight });
    }
    // Branch 3: Single green (in win budget & correct & below suggestion)
    else if (inWinBudget && cosineAlignment > cosineThreshold) {
      console.log(`[PLAY] Attempt ${nrAttempts + 1} → win with hint (single green)`);
      cmds = makeCmd({ check: true, toggle_stop_rendering: stopRenderForAnim, animation_door: true, animation_colored: true });
    }
    // Branch 4: All doors white
    else {
      console.log(`[PLAY] Attempt ${nrAttempts + 1} → check (all white)`);
      cmds = makeCmd({ check: true, toggle_stop_rendering: stopRenderForAnim, animation_door: true, animation_all_door: true });
    }

    nrAttempts += 1;
    writeCommands(cmds);
    fsmState = FSM.WAITING_ANIMATION_START;
    console.log("[FSM] → WAITING_ANIMATION_START");
    logFrame(state, cmds);
    return;
  }

  // ── Normal playing: relay combined keyboard + touch inputs ──
  processTouchInput();

  const cmds = makeCmd({
    rotate_left: inputs.rotate_left || swipe.rotateLeft,
    rotate_right: inputs.rotate_right || swipe.rotateRight,
    zoom_in: inputs.zoom_in || pinch.zoomIn,
    zoom_out: inputs.zoom_out || pinch.zoomOut,
  });
  writeCommands(cmds);
  logFrame(state, cmds);
}

function handleWaitingAnimationStart(state) {
  if (state.is_animating) {
    console.log("[FSM] Animation started → WAITING_ANIMATION_END");
    fsmState = FSM.WAITING_ANIMATION_END;
  }

  logFrame(state, readCommands());
}

function handleWaitingAnimationEnd(state) {
  // Default to true (still animating) if field is missing, matching Python's
  // state.get("is_animating", True) — avoids premature exit on undefined.
  if (!(state.is_animating ?? true)) {
    // Animation done → resume rendering (only toggle if currently stopped)
    console.log("[FSM] Animation finished → issuing toggle_stop_rendering (resume)");
    resetAllCommands();
    writeCommands(makeCmd({ toggle_stop_rendering: state.is_rendering_stopped }));
    logFrame(state, readCommands());
    fsmState = FSM.PLAYING;
    _playingStartTime = Date.now();
    console.log("[FSM] → PLAYING");
    return;
  }
  // Still animating – send no commands
  const cmds = makeCmd();
  console.log("[FSM] Waiting for animation to end (still animating)...");
  logFrame(state, cmds);
}

function handleTrialIndexUpdate() {
  const level = currentLevel();
  const n = level.trials.length;
  const idx = _trialIdx();

  let newIdx;
  if (trialProceeding === PROCEEDING.ADVANCE) newIdx = idx + 1;
  else if (trialProceeding === PROCEEDING.RETROCEED) newIdx = Math.max(0, idx - 1);
  else newIdx = idx; // STAY

  trialRunCounter += 1;

  _setTrialIdx(newIdx);

  if (_levelComplete()) {
    _finalizeLevelRun("completed");
    const wasLastLevel = currentLevelIndex === levels.length - 1;
    if (wasLastLevel) {
      console.log("[SESSION] All levels finished → showing download popup");
      _running = false;
      updateStatusBar("Session complete");
      showDownloadPopup();
      return;
    }
    currentLevelIndex = (currentLevelIndex + 1) % levels.length;
    const newLevel = currentLevel();
    const start = newLevel.fixed.start_trial ?? 0;
    chainIdxs = new Array(newLevel.objects.length).fill(start);
    activeChain = 0;
    console.log(`[LEVEL] Level complete → level ${currentLevelIndex}`);
    return;
  }

  // If active chain is exhausted, switch to a random non‑exhausted chain
  if (_trialIdx() >= n) {
    const candidates = [];
    for (let i = 0; i < level.objects.length; i++) {
      if (chainIdxs[i] < n) candidates.push(i);
    }
    if (candidates.length > 0) {
      const rand = Math.floor(Math.random() * candidates.length);
      activeChain = candidates[rand];
    }
    console.log(`[CHAIN] Chain exhausted, switching to chain ${activeChain}`);
  } else {
    _maybeSwitch();
  }
}

function handleTrialComplete(state) {
  const trial = flatTrial();
  const nrToWin = trial.nr_attempts_to_win ?? 999;
  const nrToRetro = trial.nr_attempts_to_retroceed ?? 999;
  const timeToWin = trial.elapsed_time_to_win ?? 9999;
  const timeToRetro = trial.elapsed_time_to_retroceed ?? 9999;

  console.log(`[EVAL] attempts=${nrAttempts} elapsed=${state.elapsed_secs?.toFixed(1)}s | win<=${nrToWin}/${timeToWin}s  retro>=${nrToRetro}/${timeToRetro}s`);

  const outcome = {
    [PROCEEDING.ADVANCE]: "advance",
    [PROCEEDING.STAY]: "stay",
    [PROCEEDING.RETROCEED]: "retroceed",
  }[trialProceeding];

  console.log(`[EVAL] ${outcome.toUpperCase()} → level ${currentLevelIndex} chain ${activeChain} trial ${_trialIdx()}`);

  saveTrialLog(outcome);
  handleTrialIndexUpdate();
  resyncWithGame();
}

function resyncWithGame() {
  currentFrame = -1;
  // Advance lastWriteHead to current so we don't replay stale entries
  const headView = new DataView(memory.buffer, pointers.ringWriteHead);
  lastWriteHead = readU64(headView, 0);
  resyncSeq(); // Reset seq counter to current ack (handles game restart)
  gameTimeUnresponsive = 0;
  fsmState = FSM.INIT;
}

// ═══════════════════════════════════════════════════════════════════════════
// MAIN CONTROLLER LOOP (mirrors Python's loop())
// ═══════════════════════════════════════════════════════════════════════════
// Estimated wall-clock interval between controller ticks. With rAF this is
// ~1 / display_refresh_rate (≈16.67 ms on a 60 Hz display). Used only for the
// "game unresponsive" timeout accounting — exact value doesn't matter.
const CONTROLLER_TICK_INTERVAL_S_ESTIMATE = 1 / 60;

function controllerLoop() {
  if (!memory || !_running) return;
  const { newHead, states } = readGameStateSince();

  if (states.length === 0) {
    gameTimeUnresponsive += CONTROLLER_TICK_INTERVAL_S_ESTIMATE;
    if ((gameTimeUnresponsive >= GAME_UNRESPONSIVENESS_THRESHOLD_S || currentFrame === 0)) {
      console.log(`[FSM] Game unresponsive for ${gameTimeUnresponsive.toFixed(1)}s, resyncing...`);
    resyncWithGame();  
    }
    return;
  }

  lastWriteHead = newHead;

  // Sync: first frame
  if (currentFrame === -1) {
    currentFrame = states[states.length - 1].frame_number;
    console.log(`[FSM] Starting at frame ${currentFrame}`);
    return;
  }

  // Log intermediate frames that rAF cadence might have collapsed — but only
  // while the trial is actually in flight. Before PLAYING, the game is
  // pushing ring-buffer entries with frame_number ticking but elapsed_secs
  // frozen at 0 (stop_rendering is on during loading/blank-screen). Logging
  // those, then later having handleWaitingForStart's reset re-zero
  // frame_number, produces a "two timelines" artefact in the trial log.
  const inPlay =
    fsmState === FSM.PLAYING ||
    fsmState === FSM.WAITING_ANIMATION_START ||
    fsmState === FSM.WAITING_ANIMATION_END;
  if (inPlay) {
    for (let i = 0; i < states.length - 1; i++) {
      const fn = states[i].frame_number;
      if (!(String(fn) in frameLog)) {
        logFrame(states[i], {});
      }
    }
  }

  // Use the latest state for FSM dispatch (mirrors Python's self.current_state = states[-1])
  const state = states[states.length - 1];
  currentFrame = state.frame_number;
  gameTimeUnresponsive = 0;

  console.log(`[DEBUG] elapsed_secs=${state.elapsed_secs}, nrAttempts=${nrAttempts}, retroThreshold=${flatTrial().elapsed_time_to_retroceed}`);
  
  // Set progress bar on state (mirrors Python lines 413-414)
  state.progress_bar_cur_size = _progressBarCur();
  state.progress_bar_size = _progressBarSize();

  writeNoCommands();

  // Dispatch FSM — handlers may modify state in-place
  switch (fsmState) {
    case FSM.INIT:
      handleInit(state);
      break;
    case FSM.WAITING_FOR_START:
      handleWaitingForStart(state);
      break;
    case FSM.PLAYING:
      handlePlaying(state);
      break;
    case FSM.WAITING_ANIMATION_START:
      handleWaitingAnimationStart(state);
      break;
    case FSM.WAITING_ANIMATION_END:
      handleWaitingAnimationEnd(state);
      break;
    case FSM.TRIAL_COMPLETE:
      handleTrialComplete(state);
      break;
  }

  // Write game state to control (mirrors Python's self.write_game_state(self.current_state))
  writeCurrentStateToControl(state);

  // Always publish a new command sequence each loop (match Python behavior).
  // This ensures the game processes the controller's latest toggle writes
  // instead of leaving stale commands active when an ack is pending.
  incrementCommandSeq();
}

// ═══════════════════════════════════════════════════════════════════════════
// TOUCH → VELOCITY → BOOLEAN COMMAND PIPELINE
// ═══════════════════════════════════════════════════════════════════════════

function touchDist(t1, t2) {
  const dx = t2.clientX - t1.clientX, dy = t2.clientY - t1.clientY;
  return Math.sqrt(dx * dx + dy * dy);
}

/** Velocity from a position-sample ring, over the most recent `windowMs`.
 *  Trims samples older than the window in-place, then returns (last.v - first.v) / dt. */
function windowedVel(samples, now, windowMs) {
  while (samples.length > 0 && now - samples[0].t > windowMs) samples.shift();
  if (samples.length < 2) return 0;
  const first = samples[0], last = samples[samples.length - 1];
  const dt = (last.t - first.t) / 1000;
  if (dt <= 0) return 0;
  return (last.v - first.v) / dt;
}

/** Decay energy when fingers are up, dispatch velocity/energy → PWM booleans. */
function processTouchInput() {
  const now = performance.now();
  // Framerate-independent dt (clamped to avoid spiral after tab-away)
  const dtRaw = lastProcessTime > 0 ? (now - lastProcessTime) / 1000 : 1 / SWIPE.referenceFPS;
  const dt = Math.min(dtRaw, 0.1);
  lastProcessTime = now;

  // Frames-equivalent for this dt (friction^frames keeps feel identical at any fps)
  const frames = dt * SWIPE.referenceFPS;

  // Fixed exponential decay — consistent half-life at any energy level.
  if (!swipe.active) {
    swipe.energy *= Math.pow(SWIPE.friction, frames);
    if (Math.abs(swipe.energy) < SWIPE.minEnergy) swipe.energy = 0;
  }
  if (!pinch.active) {
    pinch.energy *= Math.pow(SWIPE.friction, frames);
    if (Math.abs(pinch.energy) < SWIPE.minEnergy) pinch.energy = 0;
  }

  // Rotation: active drag uses live velocity; inertia uses decaying energy.
  // dutyMaxEnergy < maxEnergy on purpose — duty saturates early for snappy drag,
  // while the higher energy cap lets hard flicks bank extra spin-time.
  swipe.rotateLeft = false;
  swipe.rotateRight = false;
  const rv = swipe.active ? swipe.velocity : swipe.energy;
  const rDuty = Math.min(1, Math.abs(rv) / SWIPE.dutyMaxEnergy);
  if (!swipe.active && rDuty > 0 && rDuty < SWIPE.coastCutoff) {
    swipe.energy = 0;
    swipe.accumulator = 0;
  } else {
    swipe.accumulator += rDuty;
    if (swipe.accumulator >= 1) {
      swipe.accumulator -= 1;
      swipe.rotateLeft = rv < 0;
      swipe.rotateRight = rv > 0;
    }
  }
  setKeyUI("left", inputs.rotate_left || swipe.rotateLeft);
  setKeyUI("right", inputs.rotate_right || swipe.rotateRight);

  // Zoom — same duty/coast model as rotation.
  pinch.zoomIn = false;
  pinch.zoomOut = false;
  const pv = pinch.active ? pinch.velocity : pinch.energy;
  const pDuty = Math.min(1, Math.abs(pv) / SWIPE.dutyMaxEnergy);
  if (!pinch.active && pDuty > 0 && pDuty < SWIPE.coastCutoff) {
    pinch.energy = 0;
    pinch.accumulator = 0;
  } else {
    pinch.accumulator += pDuty;
    if (pinch.accumulator >= 1) {
      pinch.accumulator -= 1;
      pinch.zoomIn = pv > 0;
      pinch.zoomOut = pv < 0;
    }
  }
  setKeyUI("up", inputs.zoom_in || pinch.zoomIn);
  setKeyUI("down", inputs.zoom_out || pinch.zoomOut);
}

// ═══════════════════════════════════════════════════════════════════════════
// UI HELPERS
// ═══════════════════════════════════════════════════════════════════════════

function setKeyUI(key, active) {
  const el = document.getElementById(`key-${key}`);
  if (el) {
    if (active) el.classList.add("active");
    else el.classList.remove("active");
  }
}

function showStartOverlay(show) {
  const el = document.getElementById("start-trial-overlay");
  if (el) el.style.display = show ? "flex" : "none";
}

function setOverlayPrompt(html, isLoading = false) {
  const el = document.getElementById("trial-prompt");
  if (el) el.innerHTML = html;
  const spinner = document.getElementById("trial-spinner");
  if (spinner) spinner.classList.toggle("visible", isLoading);
}


function updateStatusBar(text) {
  const el = document.getElementById("status-bar");
  if (el) el.innerText = text;
}

// ═══════════════════════════════════════════════════════════════════════════
// INPUT SETUP
// ═══════════════════════════════════════════════════════════════════════════

function setupInput() {
  // ── TOUCH ──────────────────────────────────────────────────────────────
  window.addEventListener("touchstart", (e) => {
    e.preventDefault();
    if (fsmState === FSM.WAITING_FOR_START) {
      _start = true;
      return;
    }
    if (fsmState !== FSM.PLAYING) return;
    const now = performance.now();
    if (e.touches.length >= 2) {
      swipe.active = false;
      pinch.active = true;
      pinch.wasPinching = true;
      pinch.samples = [{ v: touchDist(e.touches[0], e.touches[1]), t: now }];
      pinch.velocity = 0;
    } else {
      const t = e.touches[0];
      swipe.active = true;
      swipe.startX = t.clientX;
      swipe.startY = t.clientY;
      swipe.startTime = Date.now();
      swipe.samples = [{ v: t.clientX, t: now }];
      swipe.velocity = 0;
      swipe.energy = 0; // kill lingering inertia on fresh touch
    }
  }, { passive: false });

  window.addEventListener("touchmove", (e) => {
    e.preventDefault();
    if (fsmState !== FSM.PLAYING) return;
    const now = performance.now();

    if (e.touches.length >= 2 && pinch.active) {
      pinch.samples.push({ v: touchDist(e.touches[0], e.touches[1]), t: now });
      pinch.velocity = windowedVel(pinch.samples, now, SWIPE.velocityWindow);
    } else if (e.touches.length === 1) {
      const t = e.touches[0];
      // Recovery: touch started before PLAYING (e.g. on start overlay) — adopt it now
      if (!swipe.active) {
        swipe.active = true;
        swipe.startX = t.clientX;
        swipe.startY = t.clientY;
        swipe.startTime = Date.now();
        swipe.samples = [];
        swipe.velocity = 0;
      }
      swipe.samples.push({ v: t.clientX, t: now });
      swipe.velocity = windowedVel(swipe.samples, now, SWIPE.velocityWindow);
    }
  }, { passive: false });

  window.addEventListener("touchend", (e) => {
    e.preventDefault();
    const now = performance.now();
    if (e.touches.length === 0) {
      // Tap detection (suppress after pinch, suppress in tap grace period)
      const tapGraceOk = (Date.now() - _playingStartTime) > 500;
      if (swipe.active && fsmState === FSM.PLAYING && !pinch.wasPinching && tapGraceOk) {
        const ct = e.changedTouches[0];
        const elapsed = Date.now() - swipe.startTime;
        const dx = Math.abs(ct.clientX - swipe.startX);
        const dy = Math.abs(ct.clientY - swipe.startY);
        if (elapsed < SWIPE.tapMaxTime && dx < SWIPE.tapMaxMove && dy < SWIPE.tapMaxMove) {
          triggers.check = true;
          console.log("Tap → check alignment");
        }
      }
      // Re-derive release velocity from the window — ignores a late pause before lift.
      const swipeRelease = windowedVel(swipe.samples, now, SWIPE.velocityWindow);
      const pinchRelease = windowedVel(pinch.samples, now, SWIPE.velocityWindow);
      swipe.energy = Math.max(-SWIPE.maxEnergy, Math.min(SWIPE.maxEnergy, swipeRelease));
      pinch.energy = Math.max(-SWIPE.maxEnergy, Math.min(SWIPE.maxEnergy, pinchRelease));
      swipe.active = false;
      pinch.active = false;
      pinch.wasPinching = false;
    } else if (e.touches.length === 1) {
      // 2→1 fingers: drop pinch, start fresh single-finger tracking
      pinch.energy = 0;
      pinch.active = false;
      const t = e.touches[0];
      swipe.active = true;
      swipe.startX = t.clientX;
      swipe.startY = t.clientY;
      swipe.startTime = Date.now();
      swipe.samples = [{ v: t.clientX, t: now }];
      swipe.velocity = 0;
    }
  }, { passive: false });

  window.addEventListener("touchcancel", () => {
    swipe.active = false; swipe.energy = 0; swipe.velocity = 0; swipe.samples = [];
    pinch.active = false; pinch.energy = 0; pinch.velocity = 0; pinch.samples = [];
    pinch.wasPinching = false;
  });

  // ── KEYBOARD ───────────────────────────────────────────────────────────
  // Use capture phase so we intercept keys BEFORE Bevy's canvas handler
  // can call preventDefault() and swallow them.
  window.addEventListener("keydown", (e) => {
    // 'q' exits from any state
    if (e.code === "KeyQ") {
      _running = false;
      updateStatusBar("Stopped (Q pressed)");
      return;
    }

    // Space bar starts the trial from WAITING_FOR_START
    if (e.code === "Space" && fsmState === FSM.WAITING_FOR_START) {
      _start = true;
      e.preventDefault();
      e.stopPropagation();
      return;
    }

    // Only process gameplay inputs when PLAYING
    if (fsmState !== FSM.PLAYING) return;

    let handled = false;
    switch (e.code) {
      case "ArrowLeft":
        inputs.rotate_left = true;
        setKeyUI("left", true);
        handled = true;
        break;
      case "ArrowRight":
        inputs.rotate_right = true;
        setKeyUI("right", true);
        handled = true;
        break;
      case "ArrowUp":
        inputs.zoom_in = true;
        setKeyUI("up", true);
        handled = true;
        break;
      case "ArrowDown":
        inputs.zoom_out = true;
        setKeyUI("down", true);
        handled = true;
        break;
      case "Space":
        if (!pressedKeys.has("Space")) {
          triggers.check = true;
        }
        handled = true;
        break;
    }
    pressedKeys.add(e.code);
    if (handled) {
      e.preventDefault();
      e.stopPropagation();  // prevent Bevy/winit from also handling it
    }
  }, true);  // ← capture phase

  window.addEventListener("keyup", (e) => {
    let handled = false;
    pressedKeys.delete(e.code);
    switch (e.code) {
      case "ArrowLeft":
        inputs.rotate_left = false;
        setKeyUI("left", false);
        handled = true;
        break;
      case "ArrowRight":
        inputs.rotate_right = false;
        setKeyUI("right", false);
        handled = true;
        break;
      case "ArrowUp":
        inputs.zoom_in = false;
        setKeyUI("up", false);
        handled = true;
        break;
      case "ArrowDown":
        inputs.zoom_out = false;
        setKeyUI("down", false);
        handled = true;
        break;
      case "Space":
        triggers.check = false;
        handled = true;
        break;
    }
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  }, true);  // ← capture phase
}

// ═══════════════════════════════════════════════════════════════════════════
// TRIALS LOADING
// ═══════════════════════════════════════════════════════════════════════════

/** Backfill fields added after the original schema so older trials.jsonl
 *  files keep working without re-export. Mirrors `_backfill_level_defaults`
 *  in controller.py. */
function backfillLevelDefaults(level) {
  level.fixed ??= {};
  if (level.fixed.camera_rotation_sense == null) level.fixed.camera_rotation_sense = 1;
  for (const obj of (level.objects || [])) {
    if (!Array.isArray(obj.decorations_rotation)) obj.decorations_rotation = [0, 0, 0];
  }
}

async function loadLevels() {
  try {
    // Check sessionStorage for custom trials loaded from the landing page
    const customTrials = sessionStorage.getItem('custom_trials_jsonl');
    const customName = sessionStorage.getItem('custom_trials_name');
    let text;
    if (customTrials) {
      text = customTrials;
      console.log(`Using custom trials from sessionStorage: ${customName}`);
    } else {
      const resp = await fetch(TRIALS_PATH);
      text = await resp.text();
    }
    const lines = text.trim().split("\n").filter((l) => l.trim());
    levels = [];
    for (let i = 0; i < lines.length; i++) {
      const level = JSON.parse(lines[i]);
      if (!level.objects || !level.trials || !level.fixed) {
        console.warn(`Line ${i + 1} missing objects/trials/fixed, skipping`);
        continue;
      }
      if (level.objects.length < 1) {
        console.warn(`Line ${i + 1} needs at least 2 objects, skipping`);
        continue;
      }
      backfillLevelDefaults(level);
      levels.push(level);
    }
    const source = customTrials ? `custom (${customName})` : 'trials.jsonl';
    console.log(`Loaded ${levels.length} levels from ${source}`);
  } catch (e) {
    console.error("Failed to load trials:", e);
    levels = [];
  }
}

// ═══════════════════════════════════════════════════════════════════════════
// INITIALIZATION
// ═══════════════════════════════════════════════════════════════════════════

async function start() {
  // ── Step 1: Download WASM with progress bar ──────────────────────────────
  setLoadingStep("Downloading game (WASM)...");
  setLoadingProgress(0);
  updateStatusBar("Loading WASM...");

  const wasmUrl = new URL("./game_node/pkg/game_node_bg.wasm.gz", import.meta.url);
  let wasmBuffer;
  try {
    const gzBuffer = await fetchWithProgress(wasmUrl, (loaded, total) => {
      const mb = (loaded / 1048576).toFixed(1);
      if (total > 0) {
        const pct = Math.min(99, Math.round((loaded / total) * 100));
        setLoadingProgress(pct);
        setLoadingStep(`Downloading game (WASM) — ${mb} / ${(total / 1048576).toFixed(1)} MB`);
      } else {
        setLoadingStep(`Downloading game (WASM) — ${mb} MB`);
      }
    });
    // Decompress gzip in-browser (GitHub Pages does not gzip .wasm responses).
    wasmBuffer = await new Response(
      new Blob([gzBuffer]).stream().pipeThrough(new DecompressionStream("gzip"))
    ).arrayBuffer();
  } catch (e) {
    setLoadingStep("Failed to download game: " + e.message);
    console.error(e);
    return;
  }

  // ── Step 2: Instantiate WASM ─────────────────────────────────────────────
  setLoadingStep("Initializing WASM...");
  setLoadingProgress(-1); // indeterminate while JS compiles
  const wasm = await init({ module_or_path: wasmBuffer });
  memory = wasm.memory;
  REFRESH_RATE_HZ = refresh_rate_hz(); // read from Rust constants, like Python's monkey_shared.REFRESH_RATE_HZ

  // Pull every cross-controller constant from Rust so this file stays in
  // lockstep with controller.py. controller.py imports the same names from
  // monkey_shared (PyO3); both originate from shared/src/constants.rs.
  const cc = controller_constants();
  N_FACES = cc.N_FACES;
  N_COLOR_FLOATS = cc.N_COLOR_FLOATS;
  N_START_ORIENTS = cc.N_START_ORIENTS;
  START_ORIENTS = Array.from({ length: N_START_ORIENTS },
                              (_, k) => k * 2.0 * Math.PI / N_START_ORIENTS);
  COLOR_SUGGESTION_COS_SIM = cc.COLOR_SUGGESTION_COS_SIM;
  GAME_UNRESPONSIVENESS_THRESHOLD_S = cc.GAME_UNRESPONSIVENESS_THRESHOLD_S;
  DEFAULT_CAMERA_Y = cc.DEFAULT_CAMERA_Y;
  CAMERA_3D_INITIAL_RADIUS = cc.CAMERA_3D_INITIAL_RADIUS;
  LOGGED_STATE_FIELDS = new Set(cc.LOGGED_STATE_FIELDS);
  CONTROLLER_META_FIELDS = new Set(cc.CONTROLLER_META_FIELDS);
  FSM_STATES = cc.FSM_STATES;
  PROCEEDING_VALUES = cc.PROCEEDING_VALUES;
  for (const name of FSM_STATES) FSM[name] = name;
  for (const name of PROCEEDING_VALUES) PROCEEDING[name] = name;
  fsmState = FSM.INIT;
  trialProceeding = PROCEEDING.ADVANCE;

  // ── Step 3: Load levels ──────────────────────────────────────────────────
  setLoadingStep("Loading levels...");
  setLoadingProgress(-1);
  updateStatusBar("Loading levels...");
  await loadLevels();

  if (levels.length > 0) {
    const startIdx = levels[0].fixed.start_trial ?? 0;
    chainIdxs = new Array(levels[0].objects.length).fill(startIdx);
  }

  if (levels.length === 0) {
    setLoadingStep("ERROR: No levels loaded");
    updateStatusBar("ERROR: No levels loaded");
    return;
  }

  // ── Step 4: Set up shared memory ─────────────────────────────────────────
  setLoadingStep("Setting up game memory...");
  const sharedPtr = create_shared_memory_wasm();
  sharedMem = new WebSharedMemory(sharedPtr);

  // Get offsets
  try {
    offsets = sharedMem.get_game_state_offsets();
    cmdOffsets = sharedMem.get_commands_offsets();
    defaultGameState = sharedMem.get_default_game_state();
    console.log("Loaded offsets:", offsets);
    console.log("Command offsets:", cmdOffsets);
  } catch (e) {
    console.error("Failed to load offsets:", e);
  }

  // Pointers
  pointers.commandSeq = sharedMem.get_command_seq_ptr();
  pointers.commandAck = sharedMem.get_command_ack_ptr();
  pointers.cmd = sharedMem.get_commands_ptr();
  pointers.gsGame = sharedMem.get_game_structure_game_ptr();
  pointers.gsControl = sharedMem.get_game_structure_control_ptr();
  pointers.ringWriteHead = sharedMem.get_frame_buffer_write_head_ptr();
  pointers.ringEntries = sharedMem.get_frame_buffer_entries_ptr();
  ringEntryStride = sharedMem.get_frame_buffer_entry_stride();
  ringBufferSize = sharedMem.get_frame_buffer_size();

  // Catch log-schema drift early: every name in LOGGED_STATE_FIELDS must be
  // a real key in readGameStateAt's output. Mirrors controller.py's check.
  {
    const sampleKeys = new Set(Object.keys(readGameStateAt(pointers.gsGame)));
    const missing = [...LOGGED_STATE_FIELDS].filter(k => !sampleKeys.has(k));
    if (missing.length) {
      throw new Error(
        `LOGGED_STATE_FIELDS references SHM keys readGameStateAt doesn't expose: ${missing.join(", ")}`
      );
    }
  }

  // ── Step 5: Start Bevy ───────────────────────────────────────────────────
  wasm_main();

  // Setup input handlers
  setupInput();

  // Setup download popup button (shown only after the last level finishes)
  const dlBtn = document.getElementById("download-logs-btn");
  if (dlBtn) {
    dlBtn.addEventListener("click", downloadLogs);
  }
  
  _running = true;
  setLoadingStep("Ready!");
  setLoadingProgress(100);
  // Small delay so "Ready!" is visible, then hide the overlay
  setTimeout(hideLoadingOverlay, 300);
  updateStatusBar("Ready");


  resyncWithGame();

  function rafLoop() {
    if (!_running) return;
    try { controllerLoop(); } catch (e) { console.error("[FSM] controllerLoop threw:", e); }
    requestAnimationFrame(rafLoop);
  }
  requestAnimationFrame(rafLoop);
}

// ── Entry point ────────────────────────────────────────────────────────────
start().catch(console.error);
