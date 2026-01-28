//! Cross-platform shared memory interface for Monkey 3D Game.
//!
//! This library provides fixed-size atomic data structures for lock-free communication
//! between the game (renderer) and controller (state machine).
//!
//! ## Memory Layout
//!
//! SharedMemory {
//!     commands: SharedCommands,           // Controller -> Game (one-way)
//!     game_structure: SharedGameStructure // Bidirectional (both read/write)
//! }
//!
//! From controller perspective:
//! - Write: commands + game_structure (to send commands and set/restore state)
//! - Read: game_structure (to observe current state)

/// Shared timing constants for stimulus experiments.
/// These constants ensure consistent timing across all controllers.
pub mod timing {
    /// Target refresh rate in Hz (game runs at 60fps)
    pub const REFRESH_RATE_HZ: u64 = 60;
    
    /// Duration to show black screen after win (in frames)
    /// At 60fps, 60 frames = 1 second
    pub const WIN_BLANK_DURATION_FRAMES: u64 = 60;
    
    /// Convert frames to approximate seconds
    pub const fn frames_to_seconds(frames: u64) -> f32 {
        frames as f32 / REFRESH_RATE_HZ as f32
    }
    
    /// Convert seconds to frames
    pub const fn seconds_to_frames(seconds: f32) -> u64 {
        (seconds * REFRESH_RATE_HZ as f32) as u64
    }
}

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};

/// Commands sent from Controller to Game.
///
/// ## Byte Layout (9 bytes total)
/// Offset 0: rotate_left (1 byte)
/// Offset 1: rotate_right (1 byte)
/// Offset 2: zoom_in (1 byte)
/// Offset 3: zoom_out (1 byte)
/// Offset 4: check_alignment (1 byte)
/// Offset 5: reset (1 byte)
/// Offset 6: blank_screen (1 byte)
/// Offset 7: stop_rendering (1 byte)
/// Offset 8: resume_rendering (1 byte)
#[repr(C)]
#[derive(Debug)]
pub struct SharedCommands {
    /// Rotate pyramid left (continuous)
    pub rotate_left: AtomicBool,
    /// Rotate pyramid right (continuous)
    pub rotate_right: AtomicBool,
    /// Zoom camera in (continuous)
    pub zoom_in: AtomicBool,
    /// Zoom camera out (continuous)
    pub zoom_out: AtomicBool,
    /// Trigger: Check alignment
    pub check_alignment: AtomicBool,
    /// Trigger: Reset game (Game reads config from game_structure when this is true)
    pub reset: AtomicBool,
    /// Trigger: Blank the screen (show black overlay)
    pub blank_screen: AtomicBool,
    /// Trigger: Stop/pause rendering
    pub stop_rendering: AtomicBool,
    /// Trigger: Resume rendering
    pub resume_rendering: AtomicBool,
}

impl SharedCommands {
    pub const fn new() -> Self {
        Self {
            rotate_left: AtomicBool::new(false),
            rotate_right: AtomicBool::new(false),
            zoom_in: AtomicBool::new(false),
            zoom_out: AtomicBool::new(false),
            check_alignment: AtomicBool::new(false),
            reset: AtomicBool::new(false),
            blank_screen: AtomicBool::new(false),
            stop_rendering: AtomicBool::new(false),
            resume_rendering: AtomicBool::new(false),
        }
    }

    pub fn reset_triggers(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.rotate_left.store(false, Relaxed);
        self.rotate_right.store(false, Relaxed);
        self.zoom_in.store(false, Relaxed);
        self.zoom_out.store(false, Relaxed);
        self.check_alignment.store(false, Relaxed);
    }
}

impl Default for SharedCommands {
    fn default() -> Self { Self::new() }
}

/// Pyramid types.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PyramidType {
    Type1 = 0,
    Type2 = 1,
}

/// Game phases.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Playing = 0,
    Won = 1,
}

/// Unified game structure for bidirectional communication.
///
/// This structure contains both configuration (set by controller, read by game on reset)
/// and state (written by game every frame, read by controller).
///
/// From controller perspective:
/// - Write: Set config fields before triggering reset, or set state fields to restore state
/// - Read: Observe current game state
///
/// ## Byte Layout
/// Offset 0:   seed (8 bytes, u64)
/// Offset 8:   pyramid_type (4 bytes, u32)
/// Offset 12:  base_radius (4 bytes, f32 as u32 bits)
/// Offset 16:  height (4 bytes, f32 as u32 bits)
/// Offset 20:  start_orient (4 bytes, f32 as u32 bits)
/// Offset 24:  target_door (4 bytes, u32)
/// Offset 28:  colors[0..12] (48 bytes, 12 x f32 as u32 bits)
/// Offset 76:  phase (4 bytes, u32: 0=Playing, 1=Won)
/// Offset 80:  frame_number (8 bytes, u64)
/// Offset 88:  elapsed_secs (4 bytes, f32 as u32 bits)
/// Offset 92:  camera_radius (4 bytes, f32 as u32 bits)
/// Offset 96:  camera_x (4 bytes, f32 as u32 bits)
/// Offset 100: camera_y (4 bytes, f32 as u32 bits)
/// Offset 104: camera_z (4 bytes, f32 as u32 bits)
/// Offset 108: pyramid_yaw (4 bytes, f32 as u32 bits)
/// Offset 112: attempts (4 bytes, u32)
/// Offset 116: alignment (4 bytes, f32 as u32 bits, 2.0 = sentinel for None)
/// Offset 120: is_animating (1 byte, bool)
/// Offset 121: has_won (1 byte, bool)
/// Offset 122: padding (2 bytes for alignment)
/// Offset 124: win_time (4 bytes, f32 as u32 bits)
/// Total: 128 bytes
#[repr(C)]
#[derive(Debug)]
pub struct SharedGameStructure {
    // === Config fields (set by controller, read by game on reset) ===

    /// Random seed for procedural generation
    pub seed: AtomicU64,
    /// Pyramid type: 0=Type1, 1=Type2
    pub pyramid_type: AtomicU32,
    /// Base radius of pyramid (f32 bits)
    pub base_radius: AtomicU32,
    /// Height of pyramid (f32 bits)
    pub height: AtomicU32,
    /// Starting orientation in radians (f32 bits)
    pub start_orient: AtomicU32,
    /// Target door index
    pub target_door: AtomicU32,
    /// Colors: 3 faces * 4 channels (RGBA) = 12 floats as u32 bits
    pub colors: [AtomicU32; 12],

    // === State fields (written by game every frame, read by controller) ===

    /// Current game phase: 0=Playing, 1=Won
    pub phase: AtomicU32,
    /// Current frame number
    pub frame_number: AtomicU64,
    /// Elapsed seconds since game start (f32 bits)
    pub elapsed_secs: AtomicU32,
    /// Camera orbit radius (f32 bits)
    pub camera_radius: AtomicU32,
    /// Camera X position (f32 bits)
    pub camera_x: AtomicU32,
    /// Camera Y position (f32 bits)
    pub camera_y: AtomicU32,
    /// Camera Z position (f32 bits)
    pub camera_z: AtomicU32,
    /// Pyramid yaw in radians (f32 bits)
    pub pyramid_yaw: AtomicU32,
    /// Number of alignment check attempts
    pub attempts: AtomicU32,
    /// Cosine alignment: 1.0=aligned, -1.0=opposite, 2.0=sentinel for None (f32 bits)
    pub alignment: AtomicU32,
    /// Whether door animation is currently playing
    pub is_animating: AtomicBool,
    /// Whether the player has won
    pub has_won: AtomicBool,
    /// Padding for alignment
    _padding: [u8; 2],
    /// Time when player won (f32 bits), 0.0 if not won yet
    pub win_time: AtomicU32,
}

impl SharedGameStructure {
    pub const fn new() -> Self {
        Self {
            // Config defaults
            seed: AtomicU64::new(0),
            pyramid_type: AtomicU32::new(PyramidType::Type1 as u32),
            base_radius: AtomicU32::new(0),
            height: AtomicU32::new(0),
            start_orient: AtomicU32::new(0),
            target_door: AtomicU32::new(0),
            colors: [
                AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
                AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
                AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
            ],
            // State defaults
            phase: AtomicU32::new(Phase::Playing as u32),
            frame_number: AtomicU64::new(0),
            elapsed_secs: AtomicU32::new(0),
            camera_radius: AtomicU32::new(0),
            camera_x: AtomicU32::new(0),
            camera_y: AtomicU32::new(0),
            camera_z: AtomicU32::new(0),
            pyramid_yaw: AtomicU32::new(0),
            attempts: AtomicU32::new(0),
            alignment: AtomicU32::new(0),
            is_animating: AtomicBool::new(false),
            has_won: AtomicBool::new(false),
            _padding: [0; 2],
            win_time: AtomicU32::new(0),
        }
    }
}

impl Default for SharedGameStructure {
    fn default() -> Self { Self::new() }
}

/// Combined shared memory region between Controller and Game.
///
/// ## Byte Layout
/// Offset 0:  commands (9 bytes + padding)
/// Offset 16: game_structure (128 bytes) - aligned to 8 bytes due to AtomicU64
///
/// Note: Actual offsets may vary due to alignment requirements.
/// Use the pointer getter methods for accurate offsets.
#[repr(C)]
#[derive(Debug)]
pub struct SharedMemory {
    pub commands: SharedCommands,
    pub game_structure: SharedGameStructure,
}

impl SharedMemory {
    pub const fn new() -> Self {
        Self {
            commands: SharedCommands::new(),
            game_structure: SharedGameStructure::new(),
        }
    }
}

impl Default for SharedMemory {
    fn default() -> Self { Self::new() }
}

// Ensure Send/Sync for FFI/Thread usage
unsafe impl Send for SharedMemory {}
unsafe impl Sync for SharedMemory {}

// Platform modules
cfg_if::cfg_if! {
    if #[cfg(not(target_arch = "wasm32"))] {
        mod native;
        pub use native::*;

        #[cfg(feature = "python")]
        pub mod python;
    } else {
        mod web;
        pub use web::*;
    }
}
