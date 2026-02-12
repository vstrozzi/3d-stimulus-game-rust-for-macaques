//! Cross-platform shared memory interface and constants for the game.
//!
//! This library provides fixed-size atomic data structures for atomic communication
//! between the game (renderer) and controller (state machine). 
//!
//! ## Memory Layout
//!
//! SharedMemory {
//!     commands: SharedCommands,                 // Controller -> Game (one-way)
//!     game_structure_contr: SharedGameStructure // Controller -> Game (one-way)
//!     game_structure_game: SharedGameStructure  // Game ->  Controller (one-way)
//!
//! }
//! 
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::atomic::Ordering;
pub mod constants;


/// Commands sent from Controller to Game.
#[repr(C)]
#[derive(Debug)]
pub struct SharedCommands {
    // Continous
    pub rotate_left: AtomicBool,
    pub rotate_right: AtomicBool,
    pub zoom_in: AtomicBool,
    pub zoom_out: AtomicBool,
    /// Trigger once
    pub check_alignment: AtomicBool,
    pub reset: AtomicBool,
    pub blank_screen: AtomicBool,
    pub stop_rendering: AtomicBool,
    pub resume_rendering: AtomicBool,
    pub animation_door: AtomicBool,
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
            animation_door: AtomicBool::new(false),
        }
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

/// Shared atomic game structure for game state communication (1 for each Controller and Game, 2 in total, read-write respectively).
/// It contains all the information realting the current game state (i.e. the game is a deterministic state).
/// It is updated every Game tick by the game and whenever needed by the Controller.
#[repr(C)]
#[derive(Debug)]
pub struct SharedGameStructure {

    // Fixed trials fields
    /// Per-face decoration seeds: 3 faces, one u64 seed each
    pub decoration_seeds: [AtomicU64; 3],
    pub base_radius: AtomicU32,
    pub height: AtomicU32,
    pub start_orient: AtomicU32,
    pub target_door: AtomicU32,
    /// Colors: 3 faces * 4 channels (RGBA) = 12 floats as u32 bits
    pub colors: [AtomicU32; 12],

    pub decorations_count: [AtomicU32; 3], // per face
    pub decorations_size: [AtomicU32; 3], // per face

    // Logic
    pub cosine_alignment_threshold: AtomicU32,

    // Animation Durations
    pub door_anim_fade_out: AtomicU32,   
    pub door_anim_stay_open: AtomicU32,  
    pub door_anim_fade_in: AtomicU32,    

    // Lighting
    pub main_spotlight_intensity: AtomicU32, 
    pub ambient_brightness: AtomicU32,      
    pub max_spotlight_intensity: AtomicU32, 

    // Dynamic trials fields
    pub frame_number: AtomicU64,
    pub elapsed_secs: AtomicU32,
    pub camera_radius: AtomicU32,
    pub camera_x: AtomicU32,
    pub camera_y: AtomicU32,
    pub camera_z: AtomicU32,
    pub attempts: AtomicU32,
    pub current_alignment: AtomicU32,
    pub current_angle: AtomicU32,
    pub is_animating: AtomicBool,
    pub win_time: AtomicU32,
}

impl SharedGameStructure {
    pub const fn new() -> Self {
        // Constant initialization from constants.rs module file.
        use constants::{
            game_constants::{
                DECORATION_SEEDS,
                COSINE_ALIGNMENT_TO_WIN},
            pyramid_constants::{
                PYRAMID_BASE_RADIUS,
                PYRAMID_HEIGHT,
                PYRAMID_START_ANGLE_OFFSET_RAD,
                PYRAMID_TARGET_DOOR_INDEX,
                PYRAMID_COLORS,
                PYRAMID_DECORATIONS_COUNT,
                PYRAMID_DECORATIONS_SIZE,
                DOOR_ANIM_FADE_IN,
                DOOR_ANIM_FADE_OUT,
                DOOR_ANIM_STAY_OPEN
            },
            lighting_constants::{
                SPOTLIGHT_LIGHT_INTENSITY,
                GLOBAL_AMBIENT_LIGHT_INTENSITY,
            },
            camera_3d_constants::{
                CAMERA_3D_INITIAL_X,
                CAMERA_3D_INITIAL_Y,
                CAMERA_3D_INITIAL_Z,
                CAMERA_3D_INITIAL_RADIUS,
            }

        };
            
        Self {
            // Fixed trials vars
            decoration_seeds: [
                AtomicU64::new(DECORATION_SEEDS[0]),
                AtomicU64::new(DECORATION_SEEDS[1]),
                AtomicU64::new(DECORATION_SEEDS[2]),
            ],
            base_radius: AtomicU32::new(PYRAMID_BASE_RADIUS.to_bits()),
            height: AtomicU32::new(PYRAMID_HEIGHT.to_bits()),
            start_orient: AtomicU32::new(PYRAMID_START_ANGLE_OFFSET_RAD.to_bits()),
            target_door: AtomicU32::new(PYRAMID_TARGET_DOOR_INDEX as u32),
            colors: [
                AtomicU32::new(PYRAMID_COLORS[0][0].to_bits()), AtomicU32::new(PYRAMID_COLORS[0][1].to_bits()), AtomicU32::new(PYRAMID_COLORS[0][2].to_bits()), AtomicU32::new(PYRAMID_COLORS[0][3].to_bits()),
                AtomicU32::new(PYRAMID_COLORS[1][0].to_bits()), AtomicU32::new(PYRAMID_COLORS[1][1].to_bits()), AtomicU32::new(PYRAMID_COLORS[1][2].to_bits()), AtomicU32::new(PYRAMID_COLORS[1][3].to_bits()),
                AtomicU32::new(PYRAMID_COLORS[2][0].to_bits()), AtomicU32::new(PYRAMID_COLORS[2][1].to_bits()), AtomicU32::new(PYRAMID_COLORS[2][2].to_bits()), AtomicU32::new(PYRAMID_COLORS[2][3].to_bits()),
            ],

            decorations_count: [
                AtomicU32::new(PYRAMID_DECORATIONS_COUNT[0]),
                AtomicU32::new(PYRAMID_DECORATIONS_COUNT[1]),
                AtomicU32::new(PYRAMID_DECORATIONS_COUNT[2]),
            ],
            
            decorations_size: [
                AtomicU32::new(PYRAMID_DECORATIONS_SIZE[0].to_bits()),
                AtomicU32::new(PYRAMID_DECORATIONS_SIZE[1].to_bits()),
                AtomicU32::new(PYRAMID_DECORATIONS_SIZE[2].to_bits()),
            ],

            cosine_alignment_threshold: AtomicU32::new(COSINE_ALIGNMENT_TO_WIN.to_bits()), // 0.9 approx
            
            door_anim_fade_out: AtomicU32::new(DOOR_ANIM_FADE_OUT.to_bits()),
            door_anim_stay_open: AtomicU32::new(DOOR_ANIM_STAY_OPEN.to_bits()),
            door_anim_fade_in: AtomicU32::new(DOOR_ANIM_FADE_IN.to_bits()),
            
            main_spotlight_intensity: AtomicU32::new(SPOTLIGHT_LIGHT_INTENSITY.to_bits()),
            ambient_brightness: AtomicU32::new(GLOBAL_AMBIENT_LIGHT_INTENSITY.to_bits()),
            max_spotlight_intensity: AtomicU32::new(constants::lighting_constants::MAX_SPOTLIGHT_INTENSITY.to_bits()),

            // Dynamic trials fields
            frame_number: AtomicU64::new(0),
            elapsed_secs: AtomicU32::new(0),
            camera_radius: AtomicU32::new(CAMERA_3D_INITIAL_RADIUS.to_bits()),
            camera_x: AtomicU32::new(CAMERA_3D_INITIAL_X.to_bits()),
            camera_y: AtomicU32::new(CAMERA_3D_INITIAL_Y.to_bits()),
            camera_z: AtomicU32::new(CAMERA_3D_INITIAL_Z.to_bits()),
            attempts: AtomicU32::new(0),
            current_alignment: AtomicU32::new(f32::to_bits(0.0)),
            current_angle: AtomicU32::new(0),
            is_animating: AtomicBool::new(false),
            win_time: AtomicU32::new(0),
        }
    }

    pub fn reset_all_fields(&self, other: &SharedGameStructure) {
        for i in 0..3 {
            self.decoration_seeds[i].store(other.decoration_seeds[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        self.base_radius.store(other.base_radius.load(Ordering::Relaxed), Ordering::Relaxed);
        self.height.store(other.height.load(Ordering::Relaxed), Ordering::Relaxed);
        self.start_orient.store(other.start_orient.load(Ordering::Relaxed), Ordering::Relaxed);
        self.target_door.store(other.target_door.load(Ordering::Relaxed), Ordering::Relaxed);
        for i in 0..12 {
            self.colors[i].store(other.colors[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        for i in 0..3 {
            self.decorations_count[i].store(other.decorations_count[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.decorations_size[i].store(other.decorations_size[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        self.cosine_alignment_threshold.store(other.cosine_alignment_threshold.load(Ordering::Relaxed), Ordering::Relaxed);
        self.door_anim_fade_out.store(other.door_anim_fade_out.load(Ordering::Relaxed), Ordering::Relaxed);
        self.door_anim_stay_open.store(other.door_anim_stay_open.load(Ordering::Relaxed), Ordering::Relaxed);
        self.door_anim_fade_in.store(other.door_anim_fade_in.load(Ordering::Relaxed), Ordering::Relaxed);
        
        self.main_spotlight_intensity.store(other.main_spotlight_intensity.load(Ordering::Relaxed), Ordering::Relaxed);
        self.ambient_brightness.store(other.ambient_brightness.load(Ordering::Relaxed), Ordering::Relaxed);
        self.max_spotlight_intensity.store(other.max_spotlight_intensity.load(Ordering::Relaxed), Ordering::Relaxed);

        self.frame_number.store(other.frame_number.load(Ordering::Relaxed), Ordering::Relaxed);
        self.elapsed_secs.store(other.elapsed_secs.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_radius.store(other.camera_radius.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_x.store(other.camera_x.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_y.store(other.camera_y.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_z.store(other.camera_z.load(Ordering::Relaxed), Ordering::Relaxed);
        self.attempts.store(other.attempts.load(Ordering::Relaxed), Ordering::Relaxed);
        self.current_alignment.store(other.current_alignment.load(Ordering::Relaxed), Ordering::Relaxed);
        self.current_angle.store(other.current_angle.load(Ordering::Relaxed), Ordering::Relaxed);
        self.is_animating.store(other.is_animating.load(Ordering::Relaxed), Ordering::Relaxed);
        self.win_time.store(other.win_time.load(Ordering::Relaxed), Ordering::Relaxed);
    }

}

impl Default for SharedGameStructure {
    fn default() -> Self { Self::new() }
}

/// Combined shared memory region between Controller and Game.
/// Using sequence number to track updates and synchronize between read and write operations.
#[repr(C)]
#[derive(Debug)]
pub struct SharedMemory {
    pub commands: SharedCommands,
    pub game_structure_game: SharedGameStructure,
    pub game_structure_control: SharedGameStructure,
}

impl SharedMemory {
    pub const fn new() -> Self {
        Self {
            commands: SharedCommands::new(),
            game_structure_game: SharedGameStructure::new(),
            game_structure_control: SharedGameStructure::new(),
        }
    }
}

impl Default for SharedMemory {
    fn default() -> Self { Self::new() }
}

// Ensure Send/Sync for thread usage
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
