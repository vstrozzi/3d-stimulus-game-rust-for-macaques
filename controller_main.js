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
  shared_game_state_byte_size,
} from "./game_node/pkg/game_node.js";


// ── Constants ──────────────────────────────────────────────────────────────
// Read from WASM after init() — mirrors Python's monkey_shared.REFRESH_RATE_HZ
let REFRESH_RATE_HZ = null;

// Struct layout constants — derived from shared/src/lib.rs SharedGameState / SharedCommands
const N_FACES         = 3;
const N_COLOR_CHANNELS = 4;                    // RGBA
const N_COLOR_FLOATS  = N_FACES * N_COLOR_CHANNELS; // 12
const N_COMMANDS      = 11;

const TRIALS_PATH = "../trials_config/trials.jsonl";

// Six evenly-spaced start orientations (one per door of the hexagonal base)
const START_ORIENTS = Array.from({ length: N_FACES * 2 }, (_, k) => k * 2.0 * Math.PI / (N_FACES * 2));
const COLOR_SUGGESTION_COS_SIM = Math.cos(Math.PI / 6);
const POLLING_INTERVAL_MS = 1; // ~1 ms between FSM ticks (matches Python's 1 ms)
const GAME_UNRESPONSIVENESS_THRESHOLD_S = 3.0;

// Controller-only metadata keys (NOT written to game shared memory)
const CONTROLLER_META_FIELDS = new Set([
  "nr_attempts_to_win",
  "nr_attempts_suggestion",
  "nr_attempts_to_retroceed",
  "elapsed_time_to_win",
  "elapsed_time_to_retroceed",
]);

// ── FSM States (mirrors ControllerState in Python) ─────────────────────────
const FSM = {
  INIT: "INIT",
  WAITING_FOR_START: "WAITING_FOR_START",
  PLAYING: "PLAYING",
  WAITING_ANIMATION_START: "WAITING_ANIMATION_START",
  WAITING_ANIMATION_END: "WAITING_ANIMATION_END",
  TRIAL_COMPLETE: "TRIAL_COMPLETE",
};

const PROCEEDING = {
  ADVANCE: "ADVANCE",
  STAY: "STAY",
  RETROCEED: "RETROCEED",
};

// ── Global State ───────────────────────────────────────────────────────────
let memory = null; // WASM Memory
let sharedMem = null; // WebSharedMemory wrapper
let pointers = { cmd: 0, gsGame: 0, gsControl: 0 };
let offsets = {}; // field offsets within SharedGameState
let cmdOffsets = {}; // field offsets within SharedCommands
let defaultGameState = null; // default SharedGameState values (from Rust)

// Levels / chains (mirrors Python's multi-level two-chain model)
let levels = [];
let currentLevelIndex = 0;
let chainAIdx = 0;
let chainBIdx = 0;
let activeChain = 0; // 0 = chain A (object[0]), 1 = chain B (object[1])

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
  blank_screen: false,
  stop_rendering: false,
  animation_door: false,
  animation_all_door: false,
  animation_colored: false,
};

// Per-trial tracking
let nrAttempts = 0;
let trialStartTime = 0;
let trialStartState = null;
let frameLog = {};
let trialRunCounter = 0;
let currentFrame = -1;
let gameTimeUnresponsive = 0;

// Special flags
let _start = false;
let _timeWinExpired = false;
let _timeRetroceedExpired = false;
let _running = false;

// All accumulated trial logs (for download)
let allTrialLogs = [];

// Pressed keys tracking (to detect one-shot key presses)
let pressedKeys = new Set();

// ── Touch state (OrbitControls-inspired: velocity from touchmove events + inertia) ──
let touchState = {
  singleTouch: {
    active: false,
    startX: 0,
    startY: 0,
    currentX: 0,
    currentY: 0,
    lastMoveX: 0,        // X at previous touchmove (for velocity)
    lastMoveY: 0,        // Y at previous touchmove
    lastMoveTime: 0,     // performance.now() at previous touchmove
    startTime: 0,
    identifier: null,
  },
  twoFingerTouch: {
    active: false,
    initialDistance: 0,
    currentDistance: 0,
    lastMoveDistance: 0, // distance at previous touchmove
    lastMoveTime: 0,     // performance.now() at previous touchmove
  },
  // Output booleans fed to the existing command pipeline
  rotateLeft: false,
  rotateRight: false,
  zoomIn: false,
  zoomOut: false,
  // Velocity tracking (px/s, decayed by inertia after release)
  rotationVelocity: 0,  // positive = right, negative = left
  zoomVelocity: 0,      // positive = zoom in (fingers apart), negative = zoom out
  // Pinch-tap suppression
  wasPinching: false,
  pinchEndTime: 0,
  // Tuning
  tapMaxMove: 10,
  tapMaxTime: 300,
  pinchTapCooldown: 250, // ms: suppress tap detection after pinch gesture
  // Velocity reference (used only to compute inertia duration, not fire rate)
  maxRotationVelocity: 500, // px/s: reference speed for inertia scaling
  maxZoomVelocity: 350,     // px/s: reference speed for inertia scaling
  // Inertia: short, clean coast after release (~0.25s)
  friction: 0.18,            // vel *= (1 - friction) per frame → stops in ~15 frames
  velocityStopThreshold: 60,  // px/s: below this, snap to zero cleanly
  // EMA smoothing for velocity during active drag
  velocitySmoothing: 0.55,   // alpha: higher = more responsive, lower = smoother
};

// Time tracking for consistent inertia regardless of tick rate
let lastTickTime = 0;

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
// SHARED MEMORY READ/WRITE HELPERS
// ═══════════════════════════════════════════════════════════════════════════

/** Read game_structure_game → JS object (mirrors Python's read_game_state) */
function readGameState() {
  const v = new DataView(memory.buffer, pointers.gsGame);
  const o = offsets;
  return {
    base_radius: v.getUint32(o.base_radius, true),
    height: v.getUint32(o.height, true),
    start_orient: v.getUint32(o.start_orient, true),
    target_door: v.getUint32(o.target_door, true),
    // colors: flat [u32; N_COLOR_FLOATS]
    colors: Array.from({ length: N_COLOR_FLOATS }, (_, i) => v.getUint32(o.colors + i * 4, true)),
    decorations_count:  Array.from({ length: N_FACES }, (_, i) => v.getUint32(o.decorations_count  + i * 4, true)),
    decorations_size:   Array.from({ length: N_FACES }, (_, i) => v.getUint32(o.decorations_size   + i * 4, true)),
    decorations_seeds:  Array.from({ length: N_FACES }, (_, i) => readU64(v, o.decorations_seeds   + i * 8)),
    decorations_shape:  Array.from({ length: N_FACES }, (_, i) => v.getUint32(o.decorations_shape  + i * 4, true)),
    cosine_alignment_threshold: v.getUint32(o.cosine_alignment_threshold, true),
    door_anim_fade_out: v.getUint32(o.door_anim_fade_out, true),
    door_anim_stay_open: v.getUint32(o.door_anim_stay_open, true),
    door_anim_fade_in: v.getUint32(o.door_anim_fade_in, true),
    main_spotlight_intensity: v.getUint32(o.main_spotlight_intensity, true),
    ambient_brightness: v.getUint32(o.ambient_brightness, true),
    max_spotlight_intensity: v.getUint32(o.max_spotlight_intensity, true),
    frame_number: readU64(v, o.frame_number),
    elapsed_secs: u32BitsToFloat(v.getUint32(o.elapsed_secs, true)),
    camera_radius: v.getUint32(o.camera_radius, true),
    camera_x: v.getUint32(o.camera_x, true),
    camera_y: v.getUint32(o.camera_y, true),
    camera_z: v.getUint32(o.camera_z, true),
    attempts: v.getUint32(o.attempts, true),
    // Return current_alignment as float for easy comparison
    cosine_alignment: u32BitsToFloat(v.getUint32(o.current_alignment, true)),
    current_angle: u32BitsToFloat(v.getUint32(o.current_angle, true)),
    is_animating: v.getUint8(o.is_animating) !== 0,
    is_blank: v.getUint8(o.is_blank) !== 0,
    is_rendering_stopped: v.getUint8(o.is_rendering_stopped) !== 0,
    is_scene_ready: v.getUint8(o.is_scene_ready) !== 0,
    // win_time as f32 → read bits, interpret as float (0.0 = not won, >0 = won)
    win_elapsed_secs: u32BitsToFloat(v.getUint32(o.win_time, true)),
    // Keep nr_attempts alias for compat with Python's check_has_finished
    nr_attempts: v.getUint32(o.attempts, true),
  };
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

/**
 * Write commands to SharedCommands (mirrors Python's write_commands).
 * @param {Object} cmds - { rotate_left, rotate_right, zoom_in, zoom_out,
 *                           check, reset, blank_screen, stop_rendering,
 *                           animation_door, animation_all_door, animation_colored }
 */
function writeCommands(cmds) {
  const view = new Uint8Array(memory.buffer, pointers.cmd, N_COMMANDS);
  const co = cmdOffsets;
  view[co.rotate_left] = cmds.rotate_left ? 1 : 0;
  view[co.rotate_right] = cmds.rotate_right ? 1 : 0;
  view[co.zoom_in] = cmds.zoom_in ? 1 : 0;
  view[co.zoom_out] = cmds.zoom_out ? 1 : 0;
  view[co.check] = cmds.check ? 1 : 0;
  view[co.reset] = cmds.reset ? 1 : 0;
  view[co.blank_screen] = cmds.blank_screen ? 1 : 0;
  view[co.stop_rendering] = cmds.stop_rendering ? 1 : 0;
  view[co.animation_door] = cmds.animation_door ? 1 : 0;
  view[co.animation_all_door] = cmds.animation_all_door ? 1 : 0;
  view[co.animation_colored] = cmds.animation_colored ? 1 : 0;
  // Match Python's write_commands: always reset triggers after writing
  resetTriggers();
}

/** Write all-false commands (Python's write_no_commands) */
function writeNoCommands() {
  const cmds = makeCmd();
  writeCommands(cmds);
  return cmds;
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
  if (state.colors)              writeU32Array(o.colors,              state.colors,              N_COLOR_FLOATS);
  if (state.decorations_count)   writeU32Array(o.decorations_count,   state.decorations_count,   N_FACES);
  if (state.decorations_size)    writeU32Array(o.decorations_size,    state.decorations_size,     N_FACES);
  if (state.decorations_shape)   writeU32Array(o.decorations_shape,   state.decorations_shape,   N_FACES);
  if (state.textures)            writeU32Array(o.textures,            state.textures,            N_FACES);
  if (state.decorations_texture) writeU32Array(o.decorations_texture, state.decorations_texture, N_FACES);
  if (state.decorations_thickness) writeU32Array(o.decorations_thickness, state.decorations_thickness, N_FACES);
  if (state.decorations_color)   writeU32Array(o.decorations_color,   state.decorations_color,   N_COLOR_FLOATS);
  if (state.decorations_seeds) {
    for (let i = 0; i < N_FACES; i++) writeU64(v, o.decorations_seeds + i * 8, state.decorations_seeds[i]);
  }

  v.setUint32(o.cosine_alignment_threshold, state.cosine_alignment_threshold, true);
  v.setUint32(o.door_anim_fade_out, state.door_anim_fade_out, true);
  v.setUint32(o.door_anim_stay_open, state.door_anim_stay_open, true);
  v.setUint32(o.door_anim_fade_in, state.door_anim_fade_in, true);
  v.setUint32(o.main_spotlight_intensity, state.main_spotlight_intensity, true);
  v.setUint32(o.ambient_brightness, state.ambient_brightness, true);
  v.setUint32(o.max_spotlight_intensity, state.max_spotlight_intensity, true);

  v.setUint32(o.progress_bar_size,     state.progress_bar_size     ?? 0, true);
  v.setUint32(o.progress_bar_cur_size, state.progress_bar_cur_size ?? 0, true);

  // Dynamic fields
  if (state.frame_number !== undefined) writeU64(v, o.frame_number, state.frame_number);
  v.setUint32(o.elapsed_secs, state.elapsed_secs ?? 0, true);
  v.setUint32(o.camera_radius, state.camera_radius, true);
  v.setUint32(o.camera_x, state.camera_x, true);
  v.setUint32(o.camera_y, state.camera_y, true);
  v.setUint32(o.camera_z, state.camera_z, true);
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
 * Copy raw bytes from game_structure_game to game_structure_control.
 * Uses shared_game_state_byte_size() exported from web.rs — zero per-field maintenance.
 * Mirrors Python's self.write_game_state(state) calls during animation states.
 */
function copyGameStateGameToControl() {
  const size = shared_game_state_byte_size();
  new Uint8Array(memory.buffer, pointers.gsControl, size)
    .set(new Uint8Array(memory.buffer, pointers.gsGame, size));
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
  const trialIdx = Math.min(
    activeChain === 0 ? chainAIdx : chainBIdx,
    level.trials.length - 1
  );
  const trialCfg = level.trials[trialIdx];
  const fixed = level.fixed;
  const flat = {};
  for (const [k, v] of Object.entries(obj))    flat[k] = v;
  for (const [k, v] of Object.entries(fixed))  { if (k !== "pr_switching_chain") flat[k] = v; }
  for (const [k, v] of Object.entries(trialCfg)) flat[k] = v;
  return flat;
}

function _trialIdx() {
  return activeChain === 0 ? chainAIdx : chainBIdx;
}

function _setTrialIdx(val) {
  if (activeChain === 0) chainAIdx = val;
  else chainBIdx = val;
}

function _levelComplete() {
  const n = currentLevel().trials.length;
  return chainAIdx >= n && chainBIdx >= n;
}

function _maybeSwitch() {
  const level = currentLevel();
  const pr = level.fixed.pr_switching_chain ?? 0.5;
  const other = 1 - activeChain;
  const otherIdx = activeChain === 0 ? chainBIdx : chainAIdx;
  if (otherIdx < level.trials.length && Math.random() < pr) {
    activeChain = other;
  }
}

function _progressBarCur() { return chainAIdx + chainBIdx; }
function _progressBarSize() { return currentLevel().trials.length * currentLevel().objects.length; }

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
  decorations_size:      "f32[]",
  decorations_thickness: "f32[]",
  colors:                "f32[][]",
  decorations_color:     "f32[][]",
  target_door:           "u32",
  textures:              "u32[]",
  decorations_count:     "u32[]",
  decorations_shape:     "u32[]",
  decorations_texture:   "u32[]",
  decorations_seeds:     "u64[]",
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
    if      (!type)          state[key] = value;                                          // unknown: pass through
    else if (type === "f32")      state[key] = floatToU32Bits(value);
    else if (type === "f32[]")    state[key] = value.map(floatToU32Bits);
    else if (type === "f32[][]")  state[key] = value.flatMap(face => face.map(floatToU32Bits));
    else                          state[key] = value;                                     // u32 / u32[] / u64[]
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
  check: false, reset: false, blank_screen: false, stop_rendering: false,
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

// ═══════════════════════════════════════════════════════════════════════════
// LOGGING
// ═══════════════════════════════════════════════════════════════════════════
const LOGGED_STATE_FIELDS = new Set([
  "frame_number", "elapsed_secs", "camera_radius",
  "nr_attempts", "cosine_alignment", "current_angle",
  "is_animating", "win_elapsed_secs",
]);

function logFrame(stateRead, commandsSent) {
  const filtered = {};
  for (const [k, v] of Object.entries(stateRead)) {
    if (LOGGED_STATE_FIELDS.has(k)) filtered[k] = v;
  }
  frameLog[String(currentFrame)] = { state_read: filtered, commands_sent: commandsSent };
}

function saveTrialLog(outcome) {
  const elapsed = (Date.now() - trialStartTime) / 1000;
  const log = {
    level_index: currentLevelIndex,
    active_chain: activeChain,
    trial_index_in_chain: _trialIdx(),
    trial_config: flatTrial(),
    outcome,
    nr_attempts: nrAttempts,
    elapsed_time: Math.round(elapsed * 10000) / 10000,
    timestamp_start: new Date(trialStartTime).toISOString(),
    timestamp_end: new Date().toISOString(),
    frames: frameLog,
  };
  allTrialLogs.push(log);
  console.log(`[LOG] Level ${currentLevelIndex} chain ${activeChain} trial ${_trialIdx()} (run ${trialRunCounter}) → ${outcome}, ${nrAttempts} attempts, ${elapsed.toFixed(1)}s`);
}

function downloadLogs() {
  const blob = new Blob([JSON.stringify(allTrialLogs, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `trial_logs_${new Date().toISOString().replace(/[:.]/g, "-")}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

// ═══════════════════════════════════════════════════════════════════════════
// CHECK HAS FINISHED (mirrors Python's check_has_finished)
// ═══════════════════════════════════════════════════════════════════════════
function checkHasFinished(state) {
  const trial = flatTrial();
  const nrAttemptsToRetroceed = trial.nr_attempts_to_retroceed ?? 0;
  const elapsedTimeToRetroceed = trial.elapsed_time_to_retroceed ?? 0;
  // Use the local nrAttempts counter (maintained by the controller) instead of
  // state.nr_attempts (from game shared memory) to stay consistent with the
  // isWin / isStay logic in handlePlaying which also uses nrAttempts.
  return (
    state.win_elapsed_secs !== 0.0 ||
    nrAttempts >= nrAttemptsToRetroceed ||
    state.elapsed_secs >= elapsedTimeToRetroceed
  );
}

// ═══════════════════════════════════════════════════════════════════════════
// FSM HANDLERS (1:1 mapping from controller.py)
// ═══════════════════════════════════════════════════════════════════════════

function handleInit() {
  console.log("[FSM] INIT → issuing blank_screen + stop_rendering");

  const trialCfg = flatTrial();
  console.log(`[FSM] Level ${currentLevelIndex} chain ${activeChain} trial ${_trialIdx()}:`, trialCfg);

  // Build fresh default state and overlay trial config
  const trialState = buildTrialState(trialCfg);

  // Randomise start orientation (mirrors Python's random.choice(START_ORIENTS))
  trialState.start_orient = floatToU32Bits(START_ORIENTS[Math.floor(Math.random() * START_ORIENTS.length)]);

  // Progress bar
  trialState.progress_bar_cur_size = _progressBarCur();
  trialState.progress_bar_size = _progressBarSize();

  // Read previous game state (to check is_blank / is_rendering_stopped)
  const stateOld = readGameState();

  // Write config to game_structure_control
  writeGameStateControl(trialState);
  trialStartState = trialState;

  console.log(`[FSM] state old is_blank=${stateOld.is_blank} is_rendering_stopped=${stateOld.is_rendering_stopped}`);

  // Commands: reset + ensure blank + ensure stopped
  writeCommands(makeCmd({
    reset: true,
    blank_screen: !stateOld.is_blank,
    stop_rendering: !stateOld.is_rendering_stopped,
  }));

  // Reset per-trial tracking
  nrAttempts = 0;
  trialStartTime = Date.now();
  frameLog = {};
  trialRunCounter += 1;
  _timeWinExpired = false;
  _timeRetroceedExpired = false;

  fsmState = FSM.WAITING_FOR_START;
  resetAllCommands();

  // Show start overlay with loading text until is_scene_ready comes back true
  showStartOverlay(true);
  setOverlayPrompt("Loading…");
  updateStatusBar(`Level ${currentLevelIndex + 1}/${levels.length} chain ${activeChain} trial ${_trialIdx() + 1}/${currentLevel().trials.length} — Loading scene…`);
  console.log("[FSM] → WAITING_FOR_START");
}

function handleWaitingForStart(state) {
  // Block start until the game confirms all trial textures are on the GPU
  if (!state.is_scene_ready) {
    writeNoCommands();
    return;
  }

  // Textures just became ready — flip overlay and status bar to "Press START" (runs once per trial)
  const statusEl = document.getElementById("status-bar");
  if (statusEl && statusEl.innerText.includes("Loading scene")) {
    setOverlayPrompt("Press the screen<br>or press space bar");
    updateStatusBar(`Level ${currentLevelIndex + 1}/${levels.length} chain ${activeChain} trial ${_trialIdx() + 1}/${currentLevel().trials.length} — Press START`);
  }

  if (_start) {
    _start = false;
    // Turn off black screen and start rendering
    const cmds = makeCmd({ reset: true, blank_screen: true, stop_rendering: true });
    writeCommands(cmds);
    fsmState = FSM.PLAYING;
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

  const isWin =
    timeElapsed < (trial.elapsed_time_to_win ?? 0) &&
    nrAttempts < (trial.nr_attempts_to_win ?? 0);

  const isStay =
    !isWin &&
    timeElapsed < (trial.elapsed_time_to_retroceed ?? 0) &&
    nrAttempts < (trial.nr_attempts_to_retroceed ?? 0);

  // Set advancement state
  if (isWin) trialProceeding = PROCEEDING.ADVANCE;
  else if (isStay) trialProceeding = PROCEEDING.STAY;
  else trialProceeding = PROCEEDING.RETROCEED;

  // ── Time-to-win expired (one-shot) ──
  if (timeElapsed > (trial.elapsed_time_to_win ?? 0) && !_timeWinExpired) {
    console.log(`[TIME] Time to win exceeded (${timeElapsed.toFixed(1)}s)`);
    _timeWinExpired = true;
    const cmds = makeCmd({ check: true, stop_rendering: true, animation_door: true, animation_all_door: true });
    writeCommands(cmds);
    fsmState = FSM.WAITING_ANIMATION_START;
    console.log("[FSM] → WAITING_ANIMATION_START");
    logFrame(state, cmds);
    return;
  }

  // ── Time-to-retroceed expired (one-shot) ──
  if (timeElapsed > (trial.elapsed_time_to_retroceed ?? 0) && !_timeRetroceedExpired) {
    console.log(`[TIME] Time to retroceed exceeded (${timeElapsed.toFixed(1)}s)`);
    _timeRetroceedExpired = true;
    const cmds = makeCmd({ check: true, stop_rendering: true, animation_door: true, animation_all_door: true, animation_colored: true });
    writeCommands(cmds);
    fsmState = FSM.WAITING_ANIMATION_START;
    console.log("[FSM] → WAITING_ANIMATION_START");
    logFrame(state, cmds);
    return;
  }

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
    const cosineAlignment = state.cosine_alignment;
    const cosineThreshold = trial.cosine_alignment_threshold ?? 0;

    let cmds;

    // Branch 1: Last attempt AND fail → all red
    if ((nrAttempts + 1) === retroceedThreshold && cosineAlignment < cosineThreshold) {
      console.log(`[PLAY] Attempt ${nrAttempts} == ${retroceedThreshold} → retroceed (all red)`);
      cmds = makeCmd({ check: true, stop_rendering: true, animation_door: true, animation_all_door: true, animation_colored: true });
      writeCommands(cmds);
      fsmState = FSM.WAITING_ANIMATION_START;
      logFrame(state, cmds);
      return;
    }
    // Branch 2: Single hint (below suggestion & miss, OR STAY & correct)
    // colored if close enough (cosine > cos(π/6)), white otherwise
    else if (
      (nrAttempts < suggestionThreshold && cosineAlignment < cosineThreshold) ||
      (trialProceeding === PROCEEDING.STAY && cosineAlignment > cosineThreshold)
    ) {
      const coloredLight = cosineAlignment > COLOR_SUGGESTION_COS_SIM;
      console.log(`[PLAY] Attempt ${nrAttempts + 1} → hint (single ${coloredLight ? "colored" : "white"})`);
      cmds = makeCmd({ check: true, stop_rendering: true, animation_door: true, animation_colored: coloredLight });
    }
    // Branch 3: Single green (isWin & correct & below suggestion)
    else if (isWin && cosineAlignment > cosineThreshold && nrAttempts < suggestionThreshold) {
      console.log(`[PLAY] Attempt ${nrAttempts + 1} → win with hint (single green)`);
      cmds = makeCmd({ check: true, stop_rendering: true, animation_door: true, animation_colored: true });
    }
    // Branch 4: All doors white
    else {
      console.log(`[PLAY] Attempt ${nrAttempts + 1} → check (all white)`);
      cmds = makeCmd({ check: true, stop_rendering: true, animation_door: true, animation_all_door: true });
    }

    nrAttempts += 1;
    writeCommands(cmds);
    fsmState = FSM.WAITING_ANIMATION_START;
    console.log("[FSM] → WAITING_ANIMATION_START");
    logFrame(state, cmds);
    return;
  }

  // ── Normal playing: relay combined keyboard + touch inputs ──
  // Compute time delta for consistent inertia
  const now = performance.now();
  const dt = lastTickTime > 0 ? Math.min((now - lastTickTime) / 1000, 0.1) : 1 / 60;
  lastTickTime = now;

  // Drive touch: decay inertia when fingers are up, dispatch booleans
  processTouchInput(dt);

  const cmds = makeCmd({
    rotate_left:  inputs.rotate_left  || touchState.rotateLeft,
    rotate_right: inputs.rotate_right || touchState.rotateRight,
    zoom_in:      inputs.zoom_in      || touchState.zoomIn,
    zoom_out:     inputs.zoom_out     || touchState.zoomOut,
  });
  writeCommands(cmds);
  logFrame(state, cmds);
}

function handleWaitingAnimationStart(state) {
  if (state.is_animating) {
    console.log("[FSM] Animation started → WAITING_ANIMATION_END");
    fsmState = FSM.WAITING_ANIMATION_END;
  }
  // Send all-false commands
  const cmds = writeNoCommands();
  // Advance/stay/retroceed chain index if the trial is already decided
  // (mirrors Python's _handle_waiting_animation_start → _handle_trial_index_update)
  if (checkHasFinished(state)) {
    handleTrialIndexUpdate();
  }
  // Sync game state to control (matches Python's self.write_game_state(state))
  copyGameStateGameToControl();
  logFrame(state, cmds);
}

function handleWaitingAnimationEnd(state) {
  // Default to true (still animating) if field is missing, matching Python's
  // state.get("is_animating", True) — avoids premature exit on undefined.
  if (!(state.is_animating ?? true)) {
    // Animation done → resume rendering
    console.log("[FSM] Animation finished → issuing stop_rendering (resume)");
    resetAllCommands();
    const cmds = makeCmd({ stop_rendering: true });
    writeCommands(cmds);
    // Sync game state to control (matches Python's self.write_game_state(state))
    copyGameStateGameToControl();
    logFrame(state, cmds);
    fsmState = FSM.PLAYING;
    console.log("[FSM] → PLAYING");
    return;
  }
  // Still animating – send no commands
  const cmds = writeNoCommands();
  // Sync game state to control (matches Python's self.write_game_state(state))
  copyGameStateGameToControl();
  logFrame(state, cmds);
}

function handleTrialIndexUpdate() {
  const n = currentLevel().trials.length;
  const idx = _trialIdx();

  let newIdx;
  if (trialProceeding === PROCEEDING.ADVANCE)    newIdx = idx + 1;
  else if (trialProceeding === PROCEEDING.RETROCEED) newIdx = Math.max(0, idx - 1);
  else                                           newIdx = idx; // STAY

  _setTrialIdx(newIdx);

  // Both chains exhausted → advance to next level
  if (_levelComplete()) {
    currentLevelIndex = (currentLevelIndex + 1) % levels.length;
    chainAIdx = 0;
    chainBIdx = 0;
    activeChain = 0;
    console.log(`[LEVEL] Level complete → level ${currentLevelIndex}`);
    return;
  }

  // If active chain is done, force-switch to the other
  if (_trialIdx() >= n) {
    activeChain = 1 - activeChain;
    console.log(`[CHAIN] Chain exhausted, switching to chain ${activeChain}`);
  } else {
    _maybeSwitch();
  }
}

function handleTrialComplete(state) {
  const trial = flatTrial();
  const nrToWin   = trial.nr_attempts_to_win ?? 999;
  const nrToRetro = trial.nr_attempts_to_retroceed ?? 999;
  const timeToWin   = trial.elapsed_time_to_win ?? 9999;
  const timeToRetro = trial.elapsed_time_to_retroceed ?? 9999;

  console.log(`[EVAL] attempts=${nrAttempts} elapsed=${state.elapsed_secs?.toFixed(1)}s | win<=${nrToWin}/${timeToWin}s  retro>=${nrToRetro}/${timeToRetro}s`);

  const outcome = {
    [PROCEEDING.ADVANCE]:   "advance",
    [PROCEEDING.STAY]:      "stay",
    [PROCEEDING.RETROCEED]: "retroceed",
  }[trialProceeding];

  console.log(`[EVAL] ${outcome.toUpperCase()} → level ${currentLevelIndex} chain ${activeChain} trial ${_trialIdx()}`);

  saveTrialLog(outcome);
  resyncWithGame();
}

function resyncWithGame() {
  currentFrame = -1;
  gameTimeUnresponsive = 0;
  fsmState = FSM.INIT;
}

// ═══════════════════════════════════════════════════════════════════════════
// MAIN CONTROLLER LOOP (mirrors Python's loop())
// ═══════════════════════════════════════════════════════════════════════════
function controllerLoop() {
  if (!memory || !_running) return;

  // Read game state from game_structure_game
  const state = readGameState();
  const frameNum = state.frame_number;

  // Sync: first frame
  if (currentFrame === -1) {
    currentFrame = frameNum;
    console.log(`[FSM] Starting at frame ${currentFrame}`);
    return;
  }

  // Wait for new frame
  if (frameNum === currentFrame) {
    gameTimeUnresponsive += POLLING_INTERVAL_MS / 1000;
    // Only resync when in states where the game should be producing frames.
    // In WAITING_FOR_START the game is paused (blank + stop_rendering) so it may
    // legitimately not advance frame_number. Same for animation-wait states.
    // Resyncing here would re-enter INIT and re-toggle blank/stop, causing the
    // screen to get stuck black (toggle-undo loop).
    const canResync = fsmState !== FSM.WAITING_FOR_START &&
                      fsmState !== FSM.WAITING_ANIMATION_START &&
                      fsmState !== FSM.WAITING_ANIMATION_END;
    if (canResync && (gameTimeUnresponsive >= GAME_UNRESPONSIVENESS_THRESHOLD_S || currentFrame === 0)) {
      console.log(`[FSM] Game unresponsive for ${gameTimeUnresponsive.toFixed(1)}s, resyncing...`);
      resyncWithGame();
    }
    return;
  }

  // New frame
  currentFrame = frameNum;
  gameTimeUnresponsive = 0;

  // Dispatch FSM
  switch (fsmState) {
    case FSM.INIT:
      handleInit();
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
}

// ═══════════════════════════════════════════════════════════════════════════
// TOUCH HANDLERS (velocity-based with inertia & proportional control)
// ═══════════════════════════════════════════════════════════════════════════

function getTouchDistance(t1, t2) {
  const dx = t2.clientX - t1.clientX;
  const dy = t2.clientY - t1.clientY;
  return Math.sqrt(dx * dx + dy * dy);
}

// ═══════════════════════════════════════════════════════════════════════════
// TOUCH → VELOCITY → BOOLEAN COMMAND PIPELINE
//
// Velocity is computed from touchmove events (not per-tick position deltas).
// processTouchInput() is called once per game frame from handlePlaying():
//   - when finger is down: velocity was already set by touchmove handler
//   - when finger is up: decay velocity via inertia (OrbitControls-style)
//   - always: dispatch velocity to rotate/zoom booleans via accumulator
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Single entry point called once per game frame from handlePlaying().
 * Decays velocities when fingers are lifted, then dispatches boolean commands.
 * @param {number} dt - seconds since last game frame
 */
function processTouchInput(dt) {
  const decay = Math.pow(1 - touchState.friction, dt * 60);

  // Decay rotation velocity when finger is NOT touching
  if (!touchState.singleTouch.active) {
    touchState.rotationVelocity *= decay;
  }
  // Decay zoom velocity when fingers are NOT pinching
  if (!touchState.twoFingerTouch.active) {
    touchState.zoomVelocity *= decay;
  }

  // Dispatch velocities → boolean commands (exactly once per frame)
  applyRotationFromVelocity();
  applyZoomFromVelocity();
}

/**
 * Convert rotationVelocity into rotateLeft/rotateRight booleans.
 * Fires every frame while velocity is above threshold — no accumulator stutter.
 * During active drag velocity is set by touchmove; after release it decays via
 * friction until it falls below velocityStopThreshold and snaps to zero.
 */
function applyRotationFromVelocity() {
  const vel = touchState.rotationVelocity;
  const absVel = Math.abs(vel);

  if (absVel < touchState.velocityStopThreshold) {
    touchState.rotateLeft = false;
    touchState.rotateRight = false;
    touchState.rotationVelocity = 0;
  } else {
    touchState.rotateLeft = vel < 0;
    touchState.rotateRight = vel > 0;
  }
  setKeyUI("left", inputs.rotate_left || touchState.rotateLeft);
  setKeyUI("right", inputs.rotate_right || touchState.rotateRight);
}

/**
 * Convert zoomVelocity into zoomIn/zoomOut booleans.
 * Fires every frame while velocity is above threshold — no accumulator stutter.
 */
function applyZoomFromVelocity() {
  const vel = touchState.zoomVelocity;
  const absVel = Math.abs(vel);

  if (absVel < touchState.velocityStopThreshold) {
    touchState.zoomIn = false;
    touchState.zoomOut = false;
    touchState.zoomVelocity = 0;
  } else {
    touchState.zoomIn = vel > 0;
    touchState.zoomOut = vel < 0;
  }
  setKeyUI("up", inputs.zoom_in || touchState.zoomIn);
  setKeyUI("down", inputs.zoom_out || touchState.zoomOut);
}

/** Reset touch tracking state. Velocities are preserved for inertia coast-down. */
function clearAllTouchState() {
  touchState.singleTouch.active = false;
  touchState.twoFingerTouch.active = false;
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

function setOverlayPrompt(html) {
  const el = document.querySelector("#start-trial-overlay .prompt");
  if (el) el.innerHTML = html;
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
    if (fsmState !== FSM.PLAYING) return;
    e.preventDefault();
    if (e.touches.length >= 2) {
      touchState.singleTouch.active = false;
      touchState.twoFingerTouch.active = true;
      touchState.wasPinching = true;
      const dist = getTouchDistance(e.touches[0], e.touches[1]);
      touchState.twoFingerTouch.initialDistance = dist;
      touchState.twoFingerTouch.currentDistance = dist;
      touchState.twoFingerTouch.lastMoveDistance = dist;
      touchState.twoFingerTouch.lastMoveTime = performance.now();
    } else if (e.touches.length === 1) {
      const t = e.touches[0];
      const now = performance.now();
      touchState.singleTouch.active = true;
      touchState.singleTouch.identifier = t.identifier;
      touchState.singleTouch.startX = t.clientX;
      touchState.singleTouch.startY = t.clientY;
      touchState.singleTouch.currentX = t.clientX;
      touchState.singleTouch.currentY = t.clientY;
      touchState.singleTouch.lastMoveX = t.clientX;
      touchState.singleTouch.lastMoveY = t.clientY;
      touchState.singleTouch.lastMoveTime = now;
      touchState.singleTouch.startTime = Date.now();
      touchState.twoFingerTouch.active = false;
      // Kill lingering inertia when starting a fresh touch
      touchState.rotationVelocity = 0;
      touchState.zoomVelocity = 0;
    }
  }, { passive: false });

  window.addEventListener("touchmove", (e) => {
    if (fsmState !== FSM.PLAYING) return;
    e.preventDefault();
    const now = performance.now();

    if (e.touches.length >= 2 && touchState.twoFingerTouch.active) {
      const dist = getTouchDistance(e.touches[0], e.touches[1]);
      touchState.twoFingerTouch.currentDistance = dist;
      // Compute zoom velocity from consecutive touchmove events
      const dt = (now - touchState.twoFingerTouch.lastMoveTime) / 1000;
      if (dt > 0 && dt < 0.15) {
        const instantVel = (dist - touchState.twoFingerTouch.lastMoveDistance) / dt;
        const alpha = touchState.velocitySmoothing;
        touchState.zoomVelocity = touchState.zoomVelocity * (1 - alpha) + instantVel * alpha;
      }
      touchState.twoFingerTouch.lastMoveDistance = dist;
      touchState.twoFingerTouch.lastMoveTime = now;

    } else if (e.touches.length === 1 && touchState.singleTouch.active) {
      const t = e.touches[0];
      touchState.singleTouch.currentX = t.clientX;
      touchState.singleTouch.currentY = t.clientY;
      // Compute rotation velocity from consecutive touchmove events
      const dt = (now - touchState.singleTouch.lastMoveTime) / 1000;
      if (dt > 0 && dt < 0.15) {
        const dx = t.clientX - touchState.singleTouch.lastMoveX;
        const instantVel = dx / dt; // px/s
        const alpha = touchState.velocitySmoothing;
        touchState.rotationVelocity = touchState.rotationVelocity * (1 - alpha) + instantVel * alpha;
      }
      touchState.singleTouch.lastMoveX = t.clientX;
      touchState.singleTouch.lastMoveY = t.clientY;
      touchState.singleTouch.lastMoveTime = now;
    }
  }, { passive: false });

  window.addEventListener("touchend", (e) => {
    e.preventDefault();
    if (e.touches.length === 0) {
      // ── Tap detection (with pinch-tap suppression) ──
      // wasPinching is set when a two-finger gesture occurred in this touch
      // sequence — reliably suppresses false taps after zoom gestures.
      if (
        touchState.singleTouch.active &&
        fsmState === FSM.PLAYING &&
        !touchState.wasPinching
      ) {
        const now = Date.now();
        const elapsed = now - touchState.singleTouch.startTime;
        const dx = Math.abs(touchState.singleTouch.currentX - touchState.singleTouch.startX);
        const dy = Math.abs(touchState.singleTouch.currentY - touchState.singleTouch.startY);
        if (
          elapsed < touchState.tapMaxTime &&
          dx < touchState.tapMaxMove &&
          dy < touchState.tapMaxMove
        ) {
          triggers.check = true;
          console.log("Tap → check alignment");
        }
      }
      // Velocities are preserved for inertia coast-down
      clearAllTouchState();
      touchState.wasPinching = false;
    } else if (e.touches.length === 1) {
      // 2→1 fingers: record pinch end time, switch to single-finger tracking
      touchState.pinchEndTime = Date.now();
      const now = performance.now();
      const t = e.touches[0];
      touchState.twoFingerTouch.active = false;
      touchState.singleTouch.active = true;
      touchState.singleTouch.identifier = t.identifier;
      touchState.singleTouch.startX = t.clientX;
      touchState.singleTouch.startY = t.clientY;
      touchState.singleTouch.currentX = t.clientX;
      touchState.singleTouch.currentY = t.clientY;
      touchState.singleTouch.lastMoveX = t.clientX;
      touchState.singleTouch.lastMoveY = t.clientY;
      touchState.singleTouch.lastMoveTime = now;
      touchState.singleTouch.startTime = Date.now();
      // Zero rotation velocity — clean start for the remaining finger
      touchState.rotationVelocity = 0;
    }
  }, { passive: false });

  window.addEventListener("touchcancel", () => {
    clearAllTouchState();
    // On cancel, also kill velocities (abnormal interruption)
    touchState.rotationVelocity = 0;
    touchState.zoomVelocity = 0;
    touchState.wasPinching = false;
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

async function loadLevels() {
  try {
    const resp = await fetch(TRIALS_PATH);
    const text = await resp.text();
    const lines = text.trim().split("\n").filter((l) => l.trim());
    levels = [];
    for (let i = 0; i < lines.length; i++) {
      const level = JSON.parse(lines[i]);
      if (!level.objects || !level.trials || !level.fixed) {
        console.warn(`Line ${i + 1} missing objects/trials/fixed, skipping`);
        continue;
      }
      if (level.objects.length < 2) {
        console.warn(`Line ${i + 1} needs at least 2 objects, skipping`);
        continue;
      }
      levels.push(level);
    }
    console.log(`Loaded ${levels.length} levels from trials.jsonl`);
  } catch (e) {
    console.error("Failed to load trials.jsonl:", e);
    levels = [];
  }
}

// ═══════════════════════════════════════════════════════════════════════════
// INITIALIZATION
// ═══════════════════════════════════════════════════════════════════════════

async function start() {
  updateStatusBar("Loading WASM...");

  // Initialize WASM
  const wasm = await init();
  memory = wasm.memory;
  REFRESH_RATE_HZ = refresh_rate_hz(); // read from Rust constants, like Python's monkey_shared.REFRESH_RATE_HZ

  updateStatusBar("Loading levels...");
  await loadLevels();

  if (levels.length === 0) {
    updateStatusBar("ERROR: No levels loaded");
    return;
  }

  // Create shared memory
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
  pointers.cmd = sharedMem.get_commands_ptr();
  pointers.gsGame = sharedMem.get_game_structure_game_ptr();
  pointers.gsControl = sharedMem.get_game_structure_control_ptr();

  // Start Bevy game (WASM main on same thread)
  wasm_main();

  // Setup input handlers
  setupInput();

  // Setup download button
  // const dlBtn = document.getElementById("btn-download-logs");
  // if (dlBtn) dlBtn.addEventListener("click", downloadLogs);

  // Setup start-trial overlay: tap/click anywhere on it to start
  const startOverlay = document.getElementById("start-trial-overlay");
  if (startOverlay) {
    startOverlay.addEventListener("click", () => {
      if (fsmState === FSM.WAITING_FOR_START) _start = true;
    });
    startOverlay.addEventListener("touchend", (e) => {
      e.preventDefault();
      if (fsmState === FSM.WAITING_FOR_START) _start = true;
    }, { passive: false });
  }

  _running = true;
  updateStatusBar("Ready");

  // Start controller FSM loop (~1ms tick, matching Python's POLLING_RATE_TIME_S)
  setInterval(controllerLoop, POLLING_INTERVAL_MS);
}

// ── Entry point ────────────────────────────────────────────────────────────
start().catch(console.error);
