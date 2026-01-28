// Web Controller + Glue Code
// Adapted for Monkey 3D Game Root Entry

import init, {
  create_shared_memory_wasm,
  WebSharedMemory,
  wasm_main,
} from "./game_node/pkg/game_node.js";

// Shared timing constants (matching shared::timing in Rust)
const REFRESH_RATE_HZ = 60;
const WIN_BLANK_DURATION_FRAMES = 60; // 1 second at 60fps

// Win state machine states
const WinState = {
  PLAYING: "PLAYING",
  WAITING_FOR_ANIMATION_END: "WAITING_FOR_ANIMATION_END",
  BLANK_SCREEN_ACTIVE: "BLANK_SCREEN_ACTIVE",
};

// Application State
let appState = "MENU"; // MENU, GAME
let running = false;
let inputs = {
  left: 0,
  right: 0,
  up: 0,
  down: 0,
  reset: 0,
  check: 0,
  blankScreen: 0,
  stopRendering: 0,
  resumeRendering: 0,
};

// Win state machine
let winState = WinState.PLAYING;
let blankStartFrame = 0;

// Touch state - virtual button system (instant like keyboard)
let touchState = {
  // Single finger touch (for rotation)
  singleTouch: {
    active: false,
    startX: 0,
    startY: 0,
    currentX: 0,
    currentY: 0,
    startTime: 0,
    identifier: null,
  },

  // Two finger touch (for zoom)
  twoFingerTouch: {
    active: false,
    initialDistance: 0,
    currentDistance: 0,
  },

  // Output states - directly mapped like button presses
  rotateLeft: false,
  rotateRight: false,
  zoomIn: false,
  zoomOut: false,

  // Thresholds - small values for instant response
  deadZone: 15, // pixels from start before rotation activates
  zoomDeadZone: 20, // pixels of pinch change before zoom activates
  tapMaxMove: 10, // max pixels movement for tap detection
  tapMaxTime: 300, // max ms for tap detection
};
let sharedMem = null;
let pointers = { cmd: 0, gameStructure: 0 };
let memory = null;

// Trials System
let trials = [];
let currentTrialIndex = 0;

// Default Config for pyramid spawn (matching Python's DEFAULT_CONFIG)
const DEFAULT_CONFIG = {
  seed: 69,
  pyramidType: 0,
  baseRadius: 2.5,
  height: 4.0,
  startOrient: 0.0,
  targetDoor: 5,
  colors: [
    [1.0, 0.2, 0.2, 1.0], // Face 1 - Red
    [0.2, 0.5, 1.0, 1.0], // Face 2 - Blue
    [0.2, 1.0, 0.3, 1.0], // Face 3 - Green
  ],
};

async function loadTrials() {
  try {
    const response = await fetch("./trials.jsonl");
    const text = await response.text();
    const lines = text
      .trim()
      .split("\n")
      .filter((line) => line.trim());
    trials = lines.map((line) => {
      const t = JSON.parse(line);
      return {
        seed: t.seed,
        pyramidType: t.pyramid_type,
        baseRadius: t.base_radius,
        height: t.height,
        startOrient: t.start_orient,
        targetDoor: t.target_door,
        colors: t.colors,
      };
    });
    console.log(`Loaded ${trials.length} trials from trials.jsonl`);
  } catch (e) {
    console.warn("Failed to load trials.jsonl, using DEFAULT_CONFIG:", e);
    trials = [DEFAULT_CONFIG];
  }
}

async function start() {
  document.getElementById("status-bar").innerText = "Loading WASM...";

  // Initialize WASM
  const wasm = await init();
  memory = wasm.memory;

  document.getElementById("status-bar").innerText = "Loading trials...";

  // Load trials configuration
  await loadTrials();

  document.getElementById("status-bar").innerText = "Ready";
  console.log(
    `Frame-based timing: ${WIN_BLANK_DURATION_FRAMES} frames = ${(WIN_BLANK_DURATION_FRAMES / REFRESH_RATE_HZ).toFixed(2)}s at ${REFRESH_RATE_HZ}Hz`
  );

  // Create shared memory
  const sharedPtr = create_shared_memory_wasm();
  sharedMem = new WebSharedMemory(sharedPtr);

  // Use helpers from shared/src/web.rs - updated for new structure
  pointers.cmd = sharedMem.get_commands_ptr();
  pointers.gameStructure = sharedMem.get_game_structure_ptr();

  // Start the Bevy game (after shared memory is ready)
  wasm_main();

  setupUI();
  setupInput();

  // Start Logic Loop
  setInterval(gameLoop, 16); // ~60Hz logic
}

function setupUI() {
  const btnStart = document.getElementById("btn-start");
  const btnExit = document.getElementById("btn-exit");

  btnStart.addEventListener("click", startGame);
  btnExit.addEventListener("click", stopGame);

  // Key listener for Enter to start
  window.addEventListener("keydown", (e) => {
    if (appState === "MENU" && e.code === "Enter") {
      startGame();
    }
  });
}

function getCurrentTrial() {
  if (trials.length === 0) return DEFAULT_CONFIG;
  return trials[currentTrialIndex % trials.length];
}

function advanceToNextTrial() {
  currentTrialIndex = (currentTrialIndex + 1) % trials.length;
  console.log(`Advancing to trial ${currentTrialIndex + 1}/${trials.length}`);
}

function startGame() {
  appState = "GAME";
  running = true;
  winState = WinState.PLAYING;

  // Write current trial config before triggering reset
  const trial = getCurrentTrial();
  writeGameStructure(trial);
  inputs.reset = 1; // Reset position on start

  document.getElementById("menu-screen").classList.remove("active");
  document.getElementById("game-screen").classList.add("active");
  document.getElementById("status-bar").innerText =
    `Trial ${currentTrialIndex + 1}/${trials.length}`;

  // Remove focus from button
  document.getElementById("btn-start").blur();

  // Focus window
  window.focus();
}

function stopGame() {
  appState = "MENU";
  running = false;

  document.getElementById("game-screen").classList.remove("active");
  document.getElementById("menu-screen").classList.add("active");
  document.getElementById("status-bar").innerText = "Stopped";
}

// Helper to get key UI element
function getKeyElement(id) {
  return document.getElementById(`key-${id}`);
}

// Helper to set key UI visual state (used by both keyboard and touch)
function setKeyUI(key, active) {
  const el = getKeyElement(key);
  if (el) {
    if (active) el.classList.add("active");
    else el.classList.remove("active");
  }
}

// =========================================================================
// TOUCH HELPER FUNCTIONS - Virtual Button System
// =========================================================================

function getTouchDistance(touch1, touch2) {
  const dx = touch2.clientX - touch1.clientX;
  const dy = touch2.clientY - touch1.clientY;
  return Math.sqrt(dx * dx + dy * dy);
}

// Update rotation state based on current finger position vs start position
// This runs every frame - gives instant button-like feel
function updateRotationFromTouch() {
  if (!touchState.singleTouch.active) {
    touchState.rotateLeft = false;
    touchState.rotateRight = false;
    setKeyUI("left", false);
    setKeyUI("right", false);
    return;
  }

  const deltaX =
    touchState.singleTouch.currentX - touchState.singleTouch.startX;

  // Simple threshold check - like pressing a button
  if (deltaX < -touchState.deadZone) {
    // Finger is LEFT of start point = LEFT button pressed
    touchState.rotateLeft = true;
    touchState.rotateRight = false;
    setKeyUI("left", true);
    setKeyUI("right", false);
  } else if (deltaX > touchState.deadZone) {
    // Finger is RIGHT of start point = RIGHT button pressed
    touchState.rotateLeft = false;
    touchState.rotateRight = true;
    setKeyUI("left", false);
    setKeyUI("right", true);
  } else {
    // Finger in dead zone = no button pressed
    touchState.rotateLeft = false;
    touchState.rotateRight = false;
    setKeyUI("left", false);
    setKeyUI("right", false);
  }
}

// Update zoom state based on pinch distance change
function updateZoomFromTouch() {
  if (!touchState.twoFingerTouch.active) {
    touchState.zoomIn = false;
    touchState.zoomOut = false;
    setKeyUI("up", false);
    setKeyUI("down", false);
    return;
  }

  const delta =
    touchState.twoFingerTouch.currentDistance -
    touchState.twoFingerTouch.initialDistance;

  if (delta > touchState.zoomDeadZone) {
    // Fingers spread apart = zoom IN (get closer)
    touchState.zoomIn = true;
    touchState.zoomOut = false;
    setKeyUI("up", true);
    setKeyUI("down", false);
  } else if (delta < -touchState.zoomDeadZone) {
    // Fingers pinched together = zoom OUT (get further)
    touchState.zoomIn = false;
    touchState.zoomOut = true;
    setKeyUI("up", false);
    setKeyUI("down", true);
  } else {
    // In dead zone
    touchState.zoomIn = false;
    touchState.zoomOut = false;
    setKeyUI("up", false);
    setKeyUI("down", false);
  }
}

function clearAllTouchState() {
  touchState.singleTouch.active = false;
  touchState.singleTouch.identifier = null;
  touchState.twoFingerTouch.active = false;
  touchState.rotateLeft = false;
  touchState.rotateRight = false;
  touchState.zoomIn = false;
  touchState.zoomOut = false;
  setKeyUI("left", false);
  setKeyUI("right", false);
  setKeyUI("up", false);
  setKeyUI("down", false);
}

function setupInput() {
  // =========================================================================
  // TOUCH INPUT HANDLERS - Virtual Button System
  // Instant response like keyboard - position-based, not gesture-based
  // =========================================================================

  window.addEventListener(
    "touchstart",
    (e) => {
      if (appState !== "GAME" || winState !== WinState.PLAYING) return;
      e.preventDefault();

      if (e.touches.length >= 2) {
        // Two or more fingers - switch to zoom mode
        touchState.singleTouch.active = false;
        touchState.twoFingerTouch.active = true;
        touchState.twoFingerTouch.initialDistance = getTouchDistance(
          e.touches[0],
          e.touches[1],
        );
        touchState.twoFingerTouch.currentDistance =
          touchState.twoFingerTouch.initialDistance;
        // Immediately update states
        updateRotationFromTouch();
        updateZoomFromTouch();
      } else if (e.touches.length === 1) {
        // Single finger - rotation mode
        const touch = e.touches[0];
        touchState.singleTouch.active = true;
        touchState.singleTouch.identifier = touch.identifier;
        touchState.singleTouch.startX = touch.clientX;
        touchState.singleTouch.startY = touch.clientY;
        touchState.singleTouch.currentX = touch.clientX;
        touchState.singleTouch.currentY = touch.clientY;
        touchState.singleTouch.startTime = Date.now();
        touchState.twoFingerTouch.active = false;
        // Immediately update states
        updateRotationFromTouch();
        updateZoomFromTouch();
      }
    },
    { passive: false },
  );

  window.addEventListener(
    "touchmove",
    (e) => {
      if (appState !== "GAME" || winState !== WinState.PLAYING) return;
      e.preventDefault();

      if (e.touches.length >= 2 && touchState.twoFingerTouch.active) {
        // Update pinch distance - zoom follows finger distance INSTANTLY
        touchState.twoFingerTouch.currentDistance = getTouchDistance(
          e.touches[0],
          e.touches[1],
        );
        updateZoomFromTouch();
      } else if (e.touches.length === 1 && touchState.singleTouch.active) {
        // Update finger position - rotation follows finger position INSTANTLY
        const touch = e.touches[0];
        touchState.singleTouch.currentX = touch.clientX;
        touchState.singleTouch.currentY = touch.clientY;
        updateRotationFromTouch();
      }
    },
    { passive: false },
  );

  window.addEventListener(
    "touchend",
    (e) => {
      if (appState !== "GAME") return;
      e.preventDefault();

      if (e.touches.length === 0) {
        // All fingers lifted - check for tap first
        if (touchState.singleTouch.active && winState === WinState.PLAYING) {
          const elapsed = Date.now() - touchState.singleTouch.startTime;
          const deltaX = Math.abs(
            touchState.singleTouch.currentX - touchState.singleTouch.startX,
          );
          const deltaY = Math.abs(
            touchState.singleTouch.currentY - touchState.singleTouch.startY,
          );

          if (
            elapsed < touchState.tapMaxTime &&
            deltaX < touchState.tapMaxMove &&
            deltaY < touchState.tapMaxMove
          ) {
            // This was a tap - trigger check alignment
            inputs.check = 1;
            console.log("Tap - check alignment");
          }
        }

        // Clear everything
        clearAllTouchState();
      } else if (e.touches.length === 1) {
        // Went from 2 fingers to 1 - switch to rotation mode
        const touch = e.touches[0];
        touchState.twoFingerTouch.active = false;
        touchState.singleTouch.active = true;
        touchState.singleTouch.identifier = touch.identifier;
        touchState.singleTouch.startX = touch.clientX;
        touchState.singleTouch.startY = touch.clientY;
        touchState.singleTouch.currentX = touch.clientX;
        touchState.singleTouch.currentY = touch.clientY;
        touchState.singleTouch.startTime = Date.now();
        updateRotationFromTouch();
        updateZoomFromTouch();
      }
    },
    { passive: false },
  );

  window.addEventListener("touchcancel", (e) => {
    clearAllTouchState();
  });

  // =========================================================================
  // KEYBOARD INPUT HANDLERS
  // =========================================================================

  window.addEventListener("keydown", (e) => {
    // Global Q exit
    if (e.code === "KeyQ" && appState === "GAME") {
      stopGame();
      return;
    }

    if (appState !== "GAME") return;

    // Only process inputs when in PLAYING win state
    if (winState !== WinState.PLAYING) return;

    let handled = false;
    switch (e.code) {
      case "ArrowLeft":
        inputs.left = 1;
        setKeyUI("left", true);
        handled = true;
        break;
      case "ArrowRight":
        inputs.right = 1;
        setKeyUI("right", true);
        handled = true;
        break;
      case "ArrowUp":
        inputs.up = 1;
        setKeyUI("up", true);
        handled = true;
        break;
      case "ArrowDown":
        inputs.down = 1;
        setKeyUI("down", true);
        handled = true;
        break;
      case "Space":
        inputs.check = 1;
        handled = true;
        break;
      case "KeyR":
        writeGameStructure(getCurrentTrial());
        inputs.reset = 1;
        winState = WinState.PLAYING;
        handled = true;
        break;
      case "KeyB":
        // Toggle blank screen
        inputs.blankScreen = 1;
        handled = true;
        break;
      case "KeyP":
        // Toggle pause/resume rendering
        inputs.stopRendering = 1;
        handled = true;
        break;
      case "KeyO":
        // Resume rendering
        inputs.resumeRendering = 1;
        handled = true;
        break;
    }

    if (handled) e.preventDefault();
  });

  window.addEventListener("keyup", (e) => {
    switch (e.code) {
      case "ArrowLeft":
        inputs.left = 0;
        setKeyUI("left", false);
        break;
      case "ArrowRight":
        inputs.right = 0;
        setKeyUI("right", false);
        break;
      case "ArrowUp":
        inputs.up = 0;
        setKeyUI("up", false);
        break;
      case "ArrowDown":
        inputs.down = 0;
        setKeyUI("down", false);
        break;
    }
  });
}

function gameLoop() {
  if (!memory) return;

  // Read game state
  if (appState === "GAME" && running) {
    const hasWon = readGameHasWon();
    const isAnimating = readGameIsAnimating();
    const currentFrame = readGameFrameNumber();

    // Win state machine (frame-based timing)
    switch (winState) {
      case WinState.PLAYING:
        if (hasWon) {
          console.log(
            `Trial ${currentTrialIndex + 1} won! Waiting for animation to complete...`
          );
          winState = WinState.WAITING_FOR_ANIMATION_END;
        }
        break;

      case WinState.WAITING_FOR_ANIMATION_END:
        if (!isAnimating) {
          console.log(
            `Animation complete. Activating blank screen for ${WIN_BLANK_DURATION_FRAMES} frames`
          );

          // Prepare next trial config
          const nextTrialIndex = (currentTrialIndex + 1) % trials.length;
          writeGameStructure(trials[nextTrialIndex]);

          // Send commands: reset + blank_screen + stop_rendering
          inputs.reset = 1;
          inputs.blankScreen = 1;
          inputs.stopRendering = 1;

          blankStartFrame = currentFrame;
          winState = WinState.BLANK_SCREEN_ACTIVE;
        }
        break;

      case WinState.BLANK_SCREEN_ACTIVE:
        const framesElapsed = currentFrame - blankStartFrame;

        if (framesElapsed >= WIN_BLANK_DURATION_FRAMES) {
          console.log(`Blank screen complete (${framesElapsed} frames). Resuming.`);

          // Send commands: blank_screen (toggle off) + resume_rendering
          inputs.blankScreen = 1;
          inputs.resumeRendering = 1;

          // Advance trial index
          currentTrialIndex = (currentTrialIndex + 1) % trials.length;
          console.log(
            `Advancing to trial ${currentTrialIndex + 1}/${trials.length}`
          );

          // Update status bar
          document.getElementById("status-bar").innerText =
            `Trial ${currentTrialIndex + 1}/${trials.length}`;

          winState = WinState.PLAYING;
        }
        break;
    }
  }

  // Update touch states (virtual button system - instant like keyboard)
  updateRotationFromTouch();
  updateZoomFromTouch();

  // Combine keyboard and touch inputs (only when in PLAYING state)
  let combinedLeft = 0;
  let combinedRight = 0;
  let combinedUp = 0;
  let combinedDown = 0;

  if (winState === WinState.PLAYING) {
    combinedLeft = inputs.left || (touchState.rotateLeft ? 1 : 0);
    combinedRight = inputs.right || (touchState.rotateRight ? 1 : 0);
    combinedUp = inputs.up || (touchState.zoomIn ? 1 : 0);
    combinedDown = inputs.down || (touchState.zoomOut ? 1 : 0);
  }

  // Write Commands
  if (appState !== "GAME" || !running) {
    writeCommands(0, 0, 0, 0, 0, 0, 0, 0, 0);
  } else {
    writeCommands(
      combinedLeft,
      combinedRight,
      combinedUp,
      combinedDown,
      inputs.check,
      inputs.reset,
      inputs.blankScreen,
      inputs.stopRendering,
      inputs.resumeRendering,
    );
  }

  // Clear triggers
  inputs.reset = 0;
  inputs.check = 0;
  inputs.blankScreen = 0;
  inputs.stopRendering = 0;
  inputs.resumeRendering = 0;
}

function readGameHasWon() {
  // SharedGameStructure layout - we need to read has_won field
  // has_won is at offset 121
  const view = new DataView(memory.buffer, pointers.gameStructure);
  return view.getUint8(121) !== 0;
}

function readGameIsAnimating() {
  // is_animating is at offset 120
  const view = new DataView(memory.buffer, pointers.gameStructure);
  return view.getUint8(120) !== 0;
}

function readGameFrameNumber() {
  // frame_number is at offset 80 (u64, but we only need lower 32 bits for comparison)
  const view = new DataView(memory.buffer, pointers.gameStructure);
  // Read as u64 little-endian (low 32 bits at offset 80, high at 84)
  const low = view.getUint32(80, true);
  const high = view.getUint32(84, true);
  // For practical purposes, just use as Number (safe up to 2^53)
  return high * 0x100000000 + low;
}

function writeCommands(
  left,
  right,
  zoomIn,
  zoomOut,
  check,
  reset,
  blankScreen,
  stopRendering,
  resumeRendering,
) {
  // SharedCommands struct layout (9 bytes):
  // rotate_left(1), rotate_right(1), zoom_in(1), zoom_out(1),
  // check_alignment(1), reset(1), blank_screen(1), stop_rendering(1), resume_rendering(1)
  const view = new Uint8Array(memory.buffer, pointers.cmd, 9);
  view[0] = left ? 1 : 0;
  view[1] = right ? 1 : 0;
  view[2] = zoomIn ? 1 : 0;
  view[3] = zoomOut ? 1 : 0;
  view[4] = check ? 1 : 0;
  view[5] = reset ? 1 : 0;
  view[6] = blankScreen ? 1 : 0;
  view[7] = stopRendering ? 1 : 0;
  view[8] = resumeRendering ? 1 : 0;
}

// Helper to convert float to its u32 bit representation (like Rust's f32::to_bits)
function floatToU32Bits(f) {
  const buf = new ArrayBuffer(4);
  new Float32Array(buf)[0] = f;
  return new Uint32Array(buf)[0];
}

function writeGameStructure(config) {
  // SharedGameStructure config fields layout (all little-endian):
  // seed: u64 (8 bytes) - offset 0
  // pyramid_type: u32 (4 bytes) - offset 8
  // base_radius: u32 (4 bytes) - offset 12 (f32 bits)
  // height: u32 (4 bytes) - offset 16 (f32 bits)
  // start_orient: u32 (4 bytes) - offset 20 (f32 bits)
  // target_door: u32 (4 bytes) - offset 24
  // colors: [u32; 12] (48 bytes) - offset 28 (f32 bits)

  const view = new DataView(memory.buffer, pointers.gameStructure);
  let offset = 0;

  // seed (u64 - use two u32 writes, little-endian)
  view.setUint32(offset, config.seed & 0xffffffff, true);
  view.setUint32(offset + 4, 0, true); // High 32 bits (seed is small)
  offset += 8;

  // pyramid_type (u32)
  view.setUint32(offset, config.pyramidType, true);
  offset += 4;

  // base_radius (f32 as u32 bits)
  view.setUint32(offset, floatToU32Bits(config.baseRadius), true);
  offset += 4;

  // height (f32 as u32 bits)
  view.setUint32(offset, floatToU32Bits(config.height), true);
  offset += 4;

  // start_orient (f32 as u32 bits)
  view.setUint32(offset, floatToU32Bits(config.startOrient), true);
  offset += 4;

  // target_door (u32)
  view.setUint32(offset, config.targetDoor, true);

  offset += 4;

  // colors: 3 faces * 4 channels = 12 floats as u32 bits
  for (const faceColors of config.colors) {
    for (const channel of faceColors) {
      view.setUint32(offset, floatToU32Bits(channel), true);
      offset += 4;
    }
  }
}

// Start
start()
  .then(() => {
    startGame();
  })
  .catch(console.error);
