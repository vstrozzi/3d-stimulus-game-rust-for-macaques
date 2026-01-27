// Web Controller + Glue Code
// Adapted for Monkey 3D Game Root Entry

import init, {
  create_shared_memory_wasm,
  WebSharedMemory,
  wasm_main,
} from "./game_node/pkg/game_node.js";

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

// Touch state tracking
let touchState = {
  // Single touch for swipe/tap
  startX: 0,
  startY: 0,
  startTime: 0,
  isActive: false,
  // Multi-touch for pinch
  initialPinchDistance: 0,
  isPinching: false,
  // Thresholds
  swipeThreshold: 30, // pixels to trigger swipe
  tapThreshold: 10, // max movement for tap
  tapTimeThreshold: 300, // max ms for tap
  // Continuous input state (for swipe-based rotation)
  swipeRotateLeft: false,
  swipeRotateRight: false,
  pinchZoomIn: false,
  pinchZoomOut: false,
};
let sharedMem = null;
let pointers = { cmd: 0, gameStructure: 0 };
let memory = null;

// Trials System
let trials = [];
let currentTrialIndex = 0;
let previousHasWon = false;

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
  previousHasWon = false;

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

function setupInput() {
  // =========================================================================
  // TOUCH INPUT HANDLERS
  // =========================================================================

  function getTouchDistance(touch1, touch2) {
    const dx = touch2.clientX - touch1.clientX;
    const dy = touch2.clientY - touch1.clientY;
    return Math.sqrt(dx * dx + dy * dy);
  }

  window.addEventListener(
    "touchstart",
    (e) => {
      if (appState !== "GAME") return;

      if (e.touches.length === 2) {
        // Start pinch gesture
        touchState.isPinching = true;
        touchState.isActive = false;
        touchState.initialPinchDistance = getTouchDistance(
          e.touches[0],
          e.touches[1],
        );
        e.preventDefault();
      } else if (e.touches.length === 1) {
        // Start single touch (swipe or tap)
        touchState.isActive = true;
        touchState.isPinching = false;
        touchState.startX = e.touches[0].clientX;
        touchState.startY = e.touches[0].clientY;
        touchState.startTime = Date.now();
        e.preventDefault();
      }
    },
    { passive: false },
  );

  window.addEventListener(
    "touchmove",
    (e) => {
      if (appState !== "GAME") return;

      if (touchState.isPinching && e.touches.length === 2) {
        // Handle pinch zoom
        const currentDistance = getTouchDistance(e.touches[0], e.touches[1]);
        const delta = currentDistance - touchState.initialPinchDistance;

        // Reset zoom states
        touchState.pinchZoomIn = false;
        touchState.pinchZoomOut = false;

        if (Math.abs(delta) > 20) {
          if (delta > 0) {
            // Pinch out = zoom in (get closer)
            touchState.pinchZoomIn = true;
            setKeyUI("up", true);
            setKeyUI("down", false);
          } else {
            // Pinch in = zoom out (get further)
            touchState.pinchZoomOut = true;
            setKeyUI("down", true);
            setKeyUI("up", false);
          }
          // Update initial distance for continuous zooming
          touchState.initialPinchDistance = currentDistance;
        }
        e.preventDefault();
      } else if (touchState.isActive && e.touches.length === 1) {
        // Handle swipe for rotation
        const currentX = e.touches[0].clientX;
        const deltaX = currentX - touchState.startX;

        // Reset rotation states
        touchState.swipeRotateLeft = false;
        touchState.swipeRotateRight = false;

        if (Math.abs(deltaX) > touchState.swipeThreshold) {
          if (deltaX > 0) {
            // Swipe right = rotate right
            touchState.swipeRotateRight = true;
            setKeyUI("right", true);
            setKeyUI("left", false);
          } else {
            // Swipe left = rotate left
            touchState.swipeRotateLeft = true;
            setKeyUI("left", true);
            setKeyUI("right", false);
          }
          // Update start position for continuous rotation
          touchState.startX = currentX;
        }
        e.preventDefault();
      }
    },
    { passive: false },
  );

  window.addEventListener(
    "touchend",
    (e) => {
      if (appState !== "GAME") return;

      if (touchState.isPinching) {
        // End pinch gesture
        touchState.isPinching = false;
        touchState.pinchZoomIn = false;
        touchState.pinchZoomOut = false;
        setKeyUI("up", false);
        setKeyUI("down", false);

        // If one finger remains, switch to swipe mode
        if (e.touches.length === 1) {
          touchState.isActive = true;
          touchState.startX = e.touches[0].clientX;
          touchState.startY = e.touches[0].clientY;
          touchState.startTime = Date.now();
        }
      } else if (touchState.isActive && e.touches.length === 0) {
        // End single touch - check for tap
        const endTime = Date.now();
        const elapsed = endTime - touchState.startTime;

        // Check if this was a tap (short time, minimal movement)
        if (elapsed < touchState.tapTimeThreshold) {
          const changedTouch = e.changedTouches[0];
          const deltaX = Math.abs(changedTouch.clientX - touchState.startX);
          const deltaY = Math.abs(changedTouch.clientY - touchState.startY);

          if (
            deltaX < touchState.tapThreshold &&
            deltaY < touchState.tapThreshold
          ) {
            // This is a tap - trigger check alignment
            inputs.check = 1;
            console.log("Touch tap - check alignment triggered");
          }
        }

        // Clear swipe rotation states
        touchState.isActive = false;
        touchState.swipeRotateLeft = false;
        touchState.swipeRotateRight = false;
        setKeyUI("left", false);
        setKeyUI("right", false);
      }

      e.preventDefault();
    },
    { passive: false },
  );

  window.addEventListener("touchcancel", (e) => {
    // Reset all touch state on cancel
    touchState.isActive = false;
    touchState.isPinching = false;
    touchState.swipeRotateLeft = false;
    touchState.swipeRotateRight = false;
    touchState.pinchZoomIn = false;
    touchState.pinchZoomOut = false;
    setKeyUI("left", false);
    setKeyUI("right", false);
    setKeyUI("up", false);
    setKeyUI("down", false);
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

  // Read game state to detect win
  if (appState === "GAME" && running) {
    const hasWon = readGameHasWon();

    // Detect rising edge of win (transition from not-won to won)
    if (hasWon && !previousHasWon) {
      console.log(`Trial ${currentTrialIndex + 1} won!`);

      // Advance to next trial after a short delay
      setTimeout(() => {
        advanceToNextTrial();
        const nextTrial = getCurrentTrial();
        writeGameStructure(nextTrial);
        inputs.reset = 1;
        document.getElementById("status-bar").innerText =
          `Trial ${currentTrialIndex + 1}/${trials.length}`;
      }, 2000);
    }
    previousHasWon = hasWon;
  }

  // Combine keyboard and touch inputs
  let combinedLeft = inputs.left || (touchState.swipeRotateLeft ? 1 : 0);
  let combinedRight = inputs.right || (touchState.swipeRotateRight ? 1 : 0);
  let combinedUp = inputs.up || (touchState.pinchZoomIn ? 1 : 0);
  let combinedDown = inputs.down || (touchState.pinchZoomOut ? 1 : 0);

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
  // Config fields:
  //   seed: u64 (8 bytes) - offset 0
  //   pyramid_type: u32 (4 bytes) - offset 8
  //   base_radius: u32 (4 bytes) - offset 12
  //   height: u32 (4 bytes) - offset 16
  //   start_orient: u32 (4 bytes) - offset 20
  //   target_door: u32 (4 bytes) - offset 24
  //   colors: [u32; 12] (48 bytes) - offset 28
  // State fields:
  //   phase: u32 (4 bytes) - offset 76
  //   frame_number: u64 (8 bytes) - offset 80
  //   elapsed_secs: u32 (4 bytes) - offset 88
  //   camera_radius: u32 (4 bytes) - offset 92
  //   camera_x: u32 (4 bytes) - offset 96
  //   camera_y: u32 (4 bytes) - offset 100
  //   camera_z: u32 (4 bytes) - offset 104
  //   pyramid_yaw: u32 (4 bytes) - offset 108
  //   attempts: u32 (4 bytes) - offset 112
  //   alignment: u32 (4 bytes) - offset 116
  //   is_animating: bool (1 byte) - offset 120
  //   has_won: bool (1 byte) - offset 121
  //   _padding: [u8; 2] (2 bytes) - offset 122
  //   win_time: u32 (4 bytes) - offset 124
  const view = new DataView(memory.buffer, pointers.gameStructure);
  // has_won is at offset 121
  return view.getUint8(121) !== 0;
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
