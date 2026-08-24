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
use strum_macros::{Display, EnumIter, FromRepr};

pub mod constants;

/// Number of slots in the frame ring buffer.
pub const RING_BUFFER_SIZE: usize = 8;

/// Commands sent from Controller to Game.
#[repr(C)]
#[derive(Debug)]
pub struct SharedCommands {
    // Continuous
    pub rotate_left: AtomicBool,
    pub rotate_right: AtomicBool,
    pub zoom_in: AtomicBool,
    pub zoom_out: AtomicBool,
    pub check_alignment: AtomicBool,
    pub reset: AtomicBool,
    pub toggle_blank: AtomicBool,
    pub toggle_stop_rendering: AtomicBool,
    pub animation_door: AtomicBool,
    pub animation_all_door: AtomicBool,
    pub animation_colored: AtomicBool,
    pub shake: AtomicBool,
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
            toggle_blank: AtomicBool::new(false),
            toggle_stop_rendering: AtomicBool::new(false),
            animation_door: AtomicBool::new(false),
            animation_all_door: AtomicBool::new(false),
            animation_colored: AtomicBool::new(false),
            shake: AtomicBool::new(false),
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

/// Shapes for decorations on the pyramid faces.
/// Indices are persisted in trials.jsonl and read directly from SHM —
/// append-only. Never renumber existing variants.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display, EnumIter)]
pub enum DecorationShape {
    Circle     = 0,
    Square     = 1,
    Star       = 2,
    Triangle   = 3,
    Rectangle  = 4,
    Oval       = 5,
    Pentagon   = 6,
    Kite       = 7,
    Rhombus    = 8,
    Trapezoid  = 9,
    Semicircle = 10,
}

// Textures for the pyramid faces and decorations.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, FromRepr, Display, EnumIter)]
#[repr(u32)]
pub enum Texture {
    Bark001_1K                  = 0,
    ChristmasTreeOrnament021_1K = 1,
    Fabric079_1K                = 2,
    Grass004_1K                 = 3,
    Marble016_1K                = 4,
    Metal061B_1K                = 5,
    PavingStones146_1K          = 6,
    Rock024_1K                  = 7,
    Rock035_1K                  = 8,
    Rope001_1K                  = 9,
    Tiles017_1K                 = 10,
    WoodFloor057_1K             = 11,
    Wood035_1K                  = 12,
    PavingStones143_1K          = 13,
    PavingStones016_1K          = 14,
    PavingStones142_1K          = 15,
    Tiles128B_1K                = 16,
    Tiles035_1K                 = 17,
    PavingStones070_1K          = 18,
    Tiles070_1K                 = 19,
    PavingStones027_1K          = 20,
    PavingStones055_1K          = 21,
    Tiles053_1K                 = 22,
    Tiles104_1K                 = 23,
    Tiles019_1K                 = 24,
    Rocks023_1K                 = 25,
    Tiles120_1K                 = 26,
    Tiles118_1K                 = 27,
    Tiles099_1K                 = 28,
    Rocks004_1K                 = 29,
    PavingStones148_1K          = 30,
    Rocks022_1K                 = 31,
    Rocks005_1K                 = 32,
    Tiles101_1K                 = 33,
    Tiles103_1K                 = 34,
    Rocks008_1K                 = 35,
    PavingStones072_1K          = 36,
    Tiles124_1K                 = 37,
    PavingStones049_1K          = 38,
    PavingStones107_1K          = 39,
    Tiles102_1K                 = 40,
}

impl Texture {
    pub fn from_u32(val: u32) -> Self {
        Self::from_repr(val).unwrap_or(Texture::WoodFloor057_1K)
    }

    /// Asset folder path relative to assets/. Derived from the variant name.
    pub fn asset_folder(&self) -> String {
        format!("textures/{}-JPG/bevy_ready", self)
    }
}

/// Shared atomic game structure for game state communication (1 for each Controller and Game, 2 in total, read-write respectively).
/// It contains all the information relating to the current game state (i.e. the game is a deterministic state).
/// It is updated every Game tick by the game and whenever needed by the Controller.
macro_rules! shared_game_state {
    ($(#[$extra_meta:meta])* $name:ident, $U32:ty, $U64:ty, $B:ty) => {
        #[repr(C)]
        #[derive(Debug)]
        $(#[$extra_meta])*
        pub struct $name {
            // Fixed trials fields
            pub base_radius: $U32,
            pub height: $U32,
            pub start_orient: $U32,
            pub target_door: $U32,

            /// Colors: 3 faces * 4 channels (RGBA) = 12 floats as u32 bits
            pub colors: [$U32; 12],
            pub textures: [$U32; 3], 
            pub decorations_count: [$U32; 3],
            pub decorations_size: [$U32; 3],
            pub decorations_color: [$U32; 12],
            pub decorations_seeds: [$U64; 3],
            pub decorations_shape: [$U32; 3],
            pub decorations_texture: [$U32; 3],
            pub decorations_thickness: [$U32; 3],
            pub decorations_rotation: [$U32; 3],

            pub cosine_alignment_threshold: $U32,

            // Animation Durations
            pub door_anim_fade_out: $U32,
            pub door_anim_stay_open: $U32,
            pub door_anim_fade_in: $U32,

            // Lighting
            pub main_spotlight_intensity: $U32,
            pub ambient_brightness: $U32,
            pub max_spotlight_intensity: $U32,

            // Level bar
            pub progress_bar_size: $U32,
            pub progress_bar_cur_size: $U32,

            // Left-side score bar (session-wide, controller-owned).
            pub score_bar_value: $U32,
            pub score_bar_max: $U32,

            /// Fraction of the session still left (1.0 = full, 0.0 = over),
            pub session_time_left: $U32,

            /// Consecutive correct answers in the current level (0 at level
            /// start). Drives the ambient mote density; controller-owned.
            pub correct_streak: $U32,

            // Camera shake (per-level config: strength + duration in seconds).
            pub shake_amplitude: $U32,
            pub shake_duration: $U32,

            // Audio + atmosphere (per-level config).
            pub sound_effects_volume: $U32,
            pub background_music_volume: $U32,
            pub fog_enabled: $B,
            pub fog_thickness_base: $U32,
            pub firefly_count: $U32,
            pub firefly_size: $U32,
            pub firefly_expand_secs: $U32,

            // Backdrop surfaces (per-level config): the ground plane and the
            // curved wall behind it. Texture is a `Texture` enum index; the
            // mask is `[r, g, b, a]` with `a` as strength (0 = bare texture).
            pub platform_texture: $U32,
            pub platform_color_mask: [$U32; 4],
            pub background_texture: $U32,
            pub background_color_mask: [$U32; 4],

            // Dynamic trials fields
            pub frame_number: $U64,
            pub render_frame_number: $U64,
            pub elapsed_secs: $U32,
            pub render_elapsed_secs: $U32,
            /// Monotonic software marker captured after wgpu `present()` for this frame.
            /// A compositor/scanout delay remains; a photodiode measures physical onset.
            pub present_elapsed_secs: $U32,
            pub photodiode_white: $B,
            pub camera_radius: $U32,
            pub camera_x: $U32,
            pub camera_y: $U32,
            pub camera_z: $U32,
            pub attempts: $U32,
            pub current_alignment: $U32,
            pub current_angle: $U32,
            pub is_animating: $B,
            pub is_blank: $B,
            pub is_rendering_stopped: $B,
            pub is_scene_ready: $B,
            pub win_time: $U32,
            pub camera_speed_rotate: $U32,
            /// Multiplier applied to the camera rotation. `+1` is the default.
            pub camera_rotation_sense: $U32,
        }
    };
}

// Atomic version (shared memory)
shared_game_state!(SharedGameState, AtomicU32, AtomicU64, AtomicBool);
// Not atomic version (local use in game logic systems)
shared_game_state!(#[derive(Clone, Copy)] SharedGameStateLocal, u32, u64, bool);

impl SharedGameState {
    pub const fn new() -> Self {
        // Constant initialization from constants.rs module file.
        use constants::{
            game_constants::{
                DECORATIONS_SEEDS,
                COSINE_ALIGNMENT_TO_WIN},
            pyramid_constants::*,
            
            lighting_constants::{
                SPOTLIGHT_LIGHT_INTENSITY,
                GLOBAL_AMBIENT_LIGHT_INTENSITY,
            },
            camera_3d_constants::{
                CAMERA_3D_INITIAL_X,
                CAMERA_3D_INITIAL_Y,
                CAMERA_3D_INITIAL_Z,
                CAMERA_3D_INITIAL_RADIUS,
                CAMERA_3D_SPEED_ROTATE,
                CAMERA_ROTATION_SENSE_DEFAULT,
            }
        };
            
        Self {
            // Fixed trials fields
            base_radius: AtomicU32::new(PYRAMID_BASE_RADIUS.to_bits()),
            height: AtomicU32::new(PYRAMID_HEIGHT.to_bits()),
            start_orient: AtomicU32::new(PYRAMID_START_ANGLE_OFFSET_RAD.to_bits()),
            target_door: AtomicU32::new(PYRAMID_TARGET_DOOR_INDEX as u32),
            
            colors: [
                AtomicU32::new(PYRAMID_COLORS[0][0].to_bits()), AtomicU32::new(PYRAMID_COLORS[0][1].to_bits()), AtomicU32::new(PYRAMID_COLORS[0][2].to_bits()), AtomicU32::new(PYRAMID_COLORS[0][3].to_bits()),
                AtomicU32::new(PYRAMID_COLORS[1][0].to_bits()), AtomicU32::new(PYRAMID_COLORS[1][1].to_bits()), AtomicU32::new(PYRAMID_COLORS[1][2].to_bits()), AtomicU32::new(PYRAMID_COLORS[1][3].to_bits()),
                AtomicU32::new(PYRAMID_COLORS[2][0].to_bits()), AtomicU32::new(PYRAMID_COLORS[2][1].to_bits()), AtomicU32::new(PYRAMID_COLORS[2][2].to_bits()), AtomicU32::new(PYRAMID_COLORS[2][3].to_bits()),
            ],
            
            textures: [
                AtomicU32::new(PYRAMID_TEXTURES[0] as u32),
                AtomicU32::new(PYRAMID_TEXTURES[1] as u32),
                AtomicU32::new(PYRAMID_TEXTURES[2] as u32),
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

            decorations_color: [
                AtomicU32::new(PYRAMID_DECORATIONS_COLOR[0][0].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[0][1].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[0][2].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[0][3].to_bits()),
                AtomicU32::new(PYRAMID_DECORATIONS_COLOR[1][0].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[1][1].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[1][2].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[1][3].to_bits()),
                AtomicU32::new(PYRAMID_DECORATIONS_COLOR[2][0].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[2][1].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[2][2].to_bits()), AtomicU32::new(PYRAMID_DECORATIONS_COLOR[2][3].to_bits()),
            ],

            decorations_seeds: [
                AtomicU64::new(DECORATIONS_SEEDS[0]),
                AtomicU64::new(DECORATIONS_SEEDS[1]),
                AtomicU64::new(DECORATIONS_SEEDS[2]),
            ],

            decorations_shape: [
                AtomicU32::new(PYRAMID_DECORATIONS_SHAPE[0] as u32),
                AtomicU32::new(PYRAMID_DECORATIONS_SHAPE[1] as u32),
                AtomicU32::new(PYRAMID_DECORATIONS_SHAPE[2] as u32),
            ],
            
            decorations_texture: [
                AtomicU32::new(PYRAMID_DECORATIONS_TEXTURE[0] as u32),
                AtomicU32::new(PYRAMID_DECORATIONS_TEXTURE[1] as u32),
                AtomicU32::new(PYRAMID_DECORATIONS_TEXTURE[2] as u32),
            ],

            decorations_thickness: [
                AtomicU32::new(PYRAMID_DECORATIONS_THICKNESS[0].to_bits()),
                AtomicU32::new(PYRAMID_DECORATIONS_THICKNESS[1].to_bits()),
                AtomicU32::new(PYRAMID_DECORATIONS_THICKNESS[2].to_bits()),
            ],

            decorations_rotation: [
                AtomicU32::new(PYRAMID_DECORATIONS_ROTATION[0] as u32),
                AtomicU32::new(PYRAMID_DECORATIONS_ROTATION[1] as u32),
                AtomicU32::new(PYRAMID_DECORATIONS_ROTATION[2] as u32),
            ],

            cosine_alignment_threshold: AtomicU32::new(COSINE_ALIGNMENT_TO_WIN.to_bits()),
            
            door_anim_fade_out: AtomicU32::new(DOOR_ANIM_FADE_OUT.to_bits()),
            door_anim_stay_open: AtomicU32::new(DOOR_ANIM_STAY_OPEN.to_bits()),
            door_anim_fade_in: AtomicU32::new(DOOR_ANIM_FADE_IN.to_bits()),
            
            main_spotlight_intensity: AtomicU32::new(SPOTLIGHT_LIGHT_INTENSITY.to_bits()),
            ambient_brightness: AtomicU32::new(GLOBAL_AMBIENT_LIGHT_INTENSITY.to_bits()),
            max_spotlight_intensity: AtomicU32::new(constants::lighting_constants::MAX_SPOTLIGHT_INTENSITY.to_bits()),

            progress_bar_size: AtomicU32::new(0),
            progress_bar_cur_size: AtomicU32::new(0),

            score_bar_value: AtomicU32::new(constants::game_constants::SCORE_BAR_DEFAULT_MAX / 2),
            score_bar_max: AtomicU32::new(constants::game_constants::SCORE_BAR_DEFAULT_MAX),
            session_time_left: AtomicU32::new(1.0f32.to_bits()),
            correct_streak: AtomicU32::new(0),

            shake_amplitude: AtomicU32::new(constants::game_constants::SHAKE_AMPLITUDE_DEFAULT.to_bits()),
            shake_duration: AtomicU32::new(constants::game_constants::SHAKE_DURATION_DEFAULT.to_bits()),

            sound_effects_volume: AtomicU32::new(constants::sound_constants::SOUND_EFFECTS_VOLUME.to_bits()),
            background_music_volume: AtomicU32::new(constants::sound_constants::BACKGROUND_MUSIC_VOLUME.to_bits()),
            fog_enabled: AtomicBool::new(constants::fog_constants::FOG_ENABLED),
            fog_thickness_base: AtomicU32::new(constants::fog_constants::FOG_THICKNESS_BASE.to_bits()),
            firefly_count: AtomicU32::new(constants::fog_constants::FIREFLY_COUNT),
            firefly_size: AtomicU32::new(constants::fog_constants::FIREFLY_SIZE.to_bits()),
            firefly_expand_secs: AtomicU32::new(constants::fog_constants::FIREFLY_EXPAND_SECS.to_bits()),

            platform_texture: AtomicU32::new(constants::backdrop_constants::PLATFORM_TEXTURE as u32),
            platform_color_mask: [
                AtomicU32::new(constants::backdrop_constants::COLOR_MASK_NONE[0].to_bits()),
                AtomicU32::new(constants::backdrop_constants::COLOR_MASK_NONE[1].to_bits()),
                AtomicU32::new(constants::backdrop_constants::COLOR_MASK_NONE[2].to_bits()),
                AtomicU32::new(constants::backdrop_constants::COLOR_MASK_NONE[3].to_bits()),
            ],
            background_texture: AtomicU32::new(constants::backdrop_constants::BACKGROUND_TEXTURE as u32),
            background_color_mask: [
                AtomicU32::new(constants::backdrop_constants::COLOR_MASK_NONE[0].to_bits()),
                AtomicU32::new(constants::backdrop_constants::COLOR_MASK_NONE[1].to_bits()),
                AtomicU32::new(constants::backdrop_constants::COLOR_MASK_NONE[2].to_bits()),
                AtomicU32::new(constants::backdrop_constants::COLOR_MASK_NONE[3].to_bits()),
            ],

            // Dynamic trials fields
            frame_number: AtomicU64::new(0),
            render_frame_number: AtomicU64::new(0),
            elapsed_secs: AtomicU32::new(0),
            render_elapsed_secs: AtomicU32::new(0),
            present_elapsed_secs: AtomicU32::new(0),
            photodiode_white: AtomicBool::new(false),
            camera_radius: AtomicU32::new(CAMERA_3D_INITIAL_RADIUS.to_bits()),
            camera_x: AtomicU32::new(CAMERA_3D_INITIAL_X.to_bits()),
            camera_y: AtomicU32::new(CAMERA_3D_INITIAL_Y.to_bits()),
            camera_z: AtomicU32::new(CAMERA_3D_INITIAL_Z.to_bits()),
            attempts: AtomicU32::new(0),
            current_alignment: AtomicU32::new(f32::to_bits(0.0)),
            current_angle: AtomicU32::new(0),
            is_animating: AtomicBool::new(false),
            is_blank: AtomicBool::new(false),
            is_rendering_stopped: AtomicBool::new(false),
            is_scene_ready: AtomicBool::new(false),

            win_time: AtomicU32::new(0),
            camera_speed_rotate: AtomicU32::new(CAMERA_3D_SPEED_ROTATE.to_bits()),
            camera_rotation_sense: AtomicU32::new(CAMERA_ROTATION_SENSE_DEFAULT as u32),
        }
    }

    pub fn reset_all_fields(&self, other: &SharedGameState) {
        self.base_radius.store(other.base_radius.load(Ordering::Relaxed), Ordering::Relaxed);
        self.height.store(other.height.load(Ordering::Relaxed), Ordering::Relaxed);
        self.start_orient.store(other.start_orient.load(Ordering::Relaxed), Ordering::Relaxed);
        self.target_door.store(other.target_door.load(Ordering::Relaxed), Ordering::Relaxed);
        for i in 0..12 {
            self.colors[i].store(other.colors[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        for i in 0..3 {
            self.textures[i].store(other.textures[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.decorations_count[i].store(other.decorations_count[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.decorations_size[i].store(other.decorations_size[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.decorations_seeds[i].store(other.decorations_seeds[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.decorations_shape[i].store(other.decorations_shape[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.decorations_texture[i].store(other.decorations_texture[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.decorations_thickness[i].store(other.decorations_thickness[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.decorations_rotation[i].store(other.decorations_rotation[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        for i in 0..12 {
            self.decorations_color[i].store(other.decorations_color[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        self.cosine_alignment_threshold.store(other.cosine_alignment_threshold.load(Ordering::Relaxed), Ordering::Relaxed);
        self.door_anim_fade_out.store(other.door_anim_fade_out.load(Ordering::Relaxed), Ordering::Relaxed);
        self.door_anim_stay_open.store(other.door_anim_stay_open.load(Ordering::Relaxed), Ordering::Relaxed);
        self.door_anim_fade_in.store(other.door_anim_fade_in.load(Ordering::Relaxed), Ordering::Relaxed);
        
        self.main_spotlight_intensity.store(other.main_spotlight_intensity.load(Ordering::Relaxed), Ordering::Relaxed);
        self.ambient_brightness.store(other.ambient_brightness.load(Ordering::Relaxed), Ordering::Relaxed);
        self.max_spotlight_intensity.store(other.max_spotlight_intensity.load(Ordering::Relaxed), Ordering::Relaxed);

        self.progress_bar_cur_size.store(other.progress_bar_cur_size.load(Ordering::Relaxed), Ordering::Relaxed);
        self.progress_bar_size.store(other.progress_bar_size.load(Ordering::Relaxed), Ordering::Relaxed);
        self.score_bar_value.store(other.score_bar_value.load(Ordering::Relaxed), Ordering::Relaxed);
        self.score_bar_max.store(other.score_bar_max.load(Ordering::Relaxed), Ordering::Relaxed);
        self.session_time_left.store(other.session_time_left.load(Ordering::Relaxed), Ordering::Relaxed);
        self.correct_streak.store(other.correct_streak.load(Ordering::Relaxed), Ordering::Relaxed);
        self.shake_amplitude.store(other.shake_amplitude.load(Ordering::Relaxed), Ordering::Relaxed);
        self.shake_duration.store(other.shake_duration.load(Ordering::Relaxed), Ordering::Relaxed);
        self.sound_effects_volume.store(other.sound_effects_volume.load(Ordering::Relaxed), Ordering::Relaxed);
        self.background_music_volume.store(other.background_music_volume.load(Ordering::Relaxed), Ordering::Relaxed);
        self.fog_enabled.store(other.fog_enabled.load(Ordering::Relaxed), Ordering::Relaxed);
        self.fog_thickness_base.store(other.fog_thickness_base.load(Ordering::Relaxed), Ordering::Relaxed);
        self.firefly_count.store(other.firefly_count.load(Ordering::Relaxed), Ordering::Relaxed);
        self.firefly_size.store(other.firefly_size.load(Ordering::Relaxed), Ordering::Relaxed);
        self.firefly_expand_secs.store(other.firefly_expand_secs.load(Ordering::Relaxed), Ordering::Relaxed);
        self.platform_texture.store(other.platform_texture.load(Ordering::Relaxed), Ordering::Relaxed);
        self.background_texture.store(other.background_texture.load(Ordering::Relaxed), Ordering::Relaxed);
        for i in 0..4 {
            self.platform_color_mask[i].store(other.platform_color_mask[i].load(Ordering::Relaxed), Ordering::Relaxed);
            self.background_color_mask[i].store(other.background_color_mask[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }

        self.frame_number.store(other.frame_number.load(Ordering::Relaxed), Ordering::Relaxed);
        self.render_frame_number.store(other.render_frame_number.load(Ordering::Relaxed), Ordering::Relaxed);
        self.elapsed_secs.store(other.elapsed_secs.load(Ordering::Relaxed), Ordering::Relaxed);
        self.render_elapsed_secs.store(other.render_elapsed_secs.load(Ordering::Relaxed), Ordering::Relaxed);
        self.present_elapsed_secs.store(other.present_elapsed_secs.load(Ordering::Relaxed), Ordering::Relaxed);
        self.photodiode_white.store(other.photodiode_white.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_radius.store(other.camera_radius.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_x.store(other.camera_x.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_y.store(other.camera_y.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_z.store(other.camera_z.load(Ordering::Relaxed), Ordering::Relaxed);
        self.attempts.store(other.attempts.load(Ordering::Relaxed), Ordering::Relaxed);
        self.current_alignment.store(other.current_alignment.load(Ordering::Relaxed), Ordering::Relaxed);
        self.current_angle.store(other.current_angle.load(Ordering::Relaxed), Ordering::Relaxed);
        self.is_animating.store(other.is_animating.load(Ordering::Relaxed), Ordering::Relaxed);
        self.is_blank.store(other.is_blank.load(Ordering::Relaxed), Ordering::Relaxed);
        self.is_rendering_stopped.store(other.is_rendering_stopped.load(Ordering::Relaxed), Ordering::Relaxed);
        self.is_scene_ready.store(other.is_scene_ready.load(Ordering::Relaxed), Ordering::Relaxed);
        self.win_time.store(other.win_time.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_speed_rotate.store(other.camera_speed_rotate.load(Ordering::Relaxed), Ordering::Relaxed);
        self.camera_rotation_sense.store(other.camera_rotation_sense.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    pub fn reset_all_fields_to_default(&self) {
        self.reset_all_fields(&SharedGameState::new());
    }

    pub fn to_not_atomic(&self) -> SharedGameStateLocal {
        SharedGameStateLocal {
            base_radius: self.base_radius.load(Ordering::Relaxed),
            height: self.height.load(Ordering::Relaxed),
            start_orient: self.start_orient.load(Ordering::Relaxed),
            target_door: self.target_door.load(Ordering::Relaxed),
            colors: {
                let mut cols = [0u32; 12];
                for (i, c) in cols.iter_mut().enumerate() {
                    *c = self.colors[i].load(Ordering::Relaxed);
                }
                cols
            },
            textures: [
                self.textures[0].load(Ordering::Relaxed),
                self.textures[1].load(Ordering::Relaxed),
                self.textures[2].load(Ordering::Relaxed),
            ],
            decorations_count: [
                self.decorations_count[0].load(Ordering::Relaxed),
                self.decorations_count[1].load(Ordering::Relaxed),
                self.decorations_count[2].load(Ordering::Relaxed),
            ],
            decorations_size: [
                self.decorations_size[0].load(Ordering::Relaxed),
                self.decorations_size[1].load(Ordering::Relaxed),
                self.decorations_size[2].load(Ordering::Relaxed),
            ],
            decorations_color: {
                let mut cols = [0u32; 12];
                for (i, c) in cols.iter_mut().enumerate() {
                    *c = self.decorations_color[i].load(Ordering::Relaxed);
                }
                cols
            },
            decorations_seeds: {
                let mut seeds = [0u64; 3];
                for (i, s) in seeds.iter_mut().enumerate() {
                    *s = self.decorations_seeds[i].load(Ordering::Relaxed);
                }
                seeds
            },
            decorations_shape: {
                let mut shapes = [0u32; 3];
                for (i, s) in shapes.iter_mut().enumerate() {
                    *s = self.decorations_shape[i].load(Ordering::Relaxed);
                }
                shapes
            },
            decorations_texture: [
                self.decorations_texture[0].load(Ordering::Relaxed),
                self.decorations_texture[1].load(Ordering::Relaxed),
                self.decorations_texture[2].load(Ordering::Relaxed),
            ],
            decorations_thickness: [
                self.decorations_thickness[0].load(Ordering::Relaxed),
                self.decorations_thickness[1].load(Ordering::Relaxed),
                self.decorations_thickness[2].load(Ordering::Relaxed),
            ],
            decorations_rotation: [
                self.decorations_rotation[0].load(Ordering::Relaxed),
                self.decorations_rotation[1].load(Ordering::Relaxed),
                self.decorations_rotation[2].load(Ordering::Relaxed),
            ],
            cosine_alignment_threshold: self.cosine_alignment_threshold.load(Ordering::Relaxed),
            door_anim_fade_out: self.door_anim_fade_out.load(Ordering::Relaxed),
            door_anim_stay_open: self.door_anim_stay_open.load(Ordering::Relaxed),
            door_anim_fade_in: self.door_anim_fade_in.load(Ordering::Relaxed),
            main_spotlight_intensity: self.main_spotlight_intensity.load(Ordering::Relaxed),
            ambient_brightness: self.ambient_brightness.load(Ordering::Relaxed),
            max_spotlight_intensity: self.max_spotlight_intensity.load(Ordering::Relaxed),
            progress_bar_size: self.progress_bar_size.load(Ordering::Relaxed),
            progress_bar_cur_size: self.progress_bar_cur_size.load(Ordering::Relaxed),
            score_bar_value: self.score_bar_value.load(Ordering::Relaxed),
            score_bar_max: self.score_bar_max.load(Ordering::Relaxed),
            session_time_left: self.session_time_left.load(Ordering::Relaxed),
            correct_streak: self.correct_streak.load(Ordering::Relaxed),
            shake_amplitude: self.shake_amplitude.load(Ordering::Relaxed),
            shake_duration: self.shake_duration.load(Ordering::Relaxed),
            sound_effects_volume: self.sound_effects_volume.load(Ordering::Relaxed),
            background_music_volume: self.background_music_volume.load(Ordering::Relaxed),
            fog_enabled: self.fog_enabled.load(Ordering::Relaxed),
            fog_thickness_base: self.fog_thickness_base.load(Ordering::Relaxed),
            firefly_count: self.firefly_count.load(Ordering::Relaxed),
            firefly_size: self.firefly_size.load(Ordering::Relaxed),
            firefly_expand_secs: self.firefly_expand_secs.load(Ordering::Relaxed),
            platform_texture: self.platform_texture.load(Ordering::Relaxed),
            platform_color_mask: {
                let mut m = [0u32; 4];
                for (i, c) in m.iter_mut().enumerate() {
                    *c = self.platform_color_mask[i].load(Ordering::Relaxed);
                }
                m
            },
            background_texture: self.background_texture.load(Ordering::Relaxed),
            background_color_mask: {
                let mut m = [0u32; 4];
                for (i, c) in m.iter_mut().enumerate() {
                    *c = self.background_color_mask[i].load(Ordering::Relaxed);
                }
                m
            },
            frame_number: self.frame_number.load(Ordering::Relaxed),
            render_frame_number: self.render_frame_number.load(Ordering::Relaxed),
            elapsed_secs: self.elapsed_secs.load(Ordering::Relaxed),
            render_elapsed_secs: self.render_elapsed_secs.load(Ordering::Relaxed),
            present_elapsed_secs: self.present_elapsed_secs.load(Ordering::Relaxed),
            photodiode_white: self.photodiode_white.load(Ordering::Relaxed),
            camera_radius: self.camera_radius.load(Ordering::Relaxed),
            camera_x: self.camera_x.load(Ordering::Relaxed),
            camera_y: self.camera_y.load(Ordering::Relaxed),
            camera_z: self.camera_z.load(Ordering::Relaxed),
            attempts: self.attempts.load(Ordering::Relaxed),
            current_alignment: self.current_alignment.load(Ordering::Relaxed),
            current_angle: self.current_angle.load(Ordering::Relaxed),
            is_animating: self.is_animating.load(Ordering::Relaxed),
            is_blank: self.is_blank.load(Ordering::Relaxed),
            is_rendering_stopped: self.is_rendering_stopped.load(Ordering::Relaxed),
            is_scene_ready: self.is_scene_ready.load(Ordering::Relaxed),
            win_time: self.win_time.load(Ordering::Relaxed),
            camera_speed_rotate: self.camera_speed_rotate.load(Ordering::Relaxed),
            camera_rotation_sense: self.camera_rotation_sense.load(Ordering::Relaxed),
        }
    }

    pub fn write_from_local(&self, state: &SharedGameStateLocal){
        self.base_radius.store(state.base_radius, Ordering::Relaxed);
        self.height.store(state.height, Ordering::Relaxed);
        self.start_orient.store(state.start_orient, Ordering::Relaxed);
        self.target_door.store(state.target_door, Ordering::Relaxed);
        for i in 0..12 {
            self.colors[i].store(state.colors[i], Ordering::Relaxed);
            self.decorations_color[i].store(state.decorations_color[i], Ordering::Relaxed);
        }
        for i in 0..3 {
            self.textures[i].store(state.textures[i], Ordering::Relaxed);
            self.decorations_count[i].store(state.decorations_count[i], Ordering::Relaxed);
            self.decorations_size[i].store(state.decorations_size[i], Ordering::Relaxed);
            self.decorations_seeds[i].store(state.decorations_seeds[i], Ordering::Relaxed);
            self.decorations_shape[i].store(state.decorations_shape[i], Ordering::Relaxed);
            self.decorations_texture[i].store(state.decorations_texture[i], Ordering::Relaxed);
            self.decorations_thickness[i].store(state.decorations_thickness[i], Ordering::Relaxed);
            self.decorations_rotation[i].store(state.decorations_rotation[i], Ordering::Relaxed);
        }
        self.cosine_alignment_threshold.store(state.cosine_alignment_threshold, Ordering::Relaxed);
        self.door_anim_fade_out.store(state.door_anim_fade_out, Ordering::Relaxed);
        self.door_anim_stay_open.store(state.door_anim_stay_open, Ordering::Relaxed);
        self.door_anim_fade_in.store(state.door_anim_fade_in, Ordering::Relaxed);
        self.main_spotlight_intensity.store(state.main_spotlight_intensity, Ordering::Relaxed);
        self.ambient_brightness.store(state.ambient_brightness, Ordering::Relaxed);
        self.max_spotlight_intensity.store(state.max_spotlight_intensity, Ordering::Relaxed);
        self.progress_bar_size.store(state.progress_bar_size, Ordering::Relaxed);
        self.progress_bar_cur_size.store(state.progress_bar_cur_size, Ordering::Relaxed);
        self.score_bar_value.store(state.score_bar_value, Ordering::Relaxed);
        self.score_bar_max.store(state.score_bar_max, Ordering::Relaxed);
        self.session_time_left.store(state.session_time_left, Ordering::Relaxed);
        self.correct_streak.store(state.correct_streak, Ordering::Relaxed);
        self.shake_amplitude.store(state.shake_amplitude, Ordering::Relaxed);
        self.shake_duration.store(state.shake_duration, Ordering::Relaxed);
        self.sound_effects_volume.store(state.sound_effects_volume, Ordering::Relaxed);
        self.background_music_volume.store(state.background_music_volume, Ordering::Relaxed);
        self.fog_enabled.store(state.fog_enabled, Ordering::Relaxed);
        self.fog_thickness_base.store(state.fog_thickness_base, Ordering::Relaxed);
        self.firefly_count.store(state.firefly_count, Ordering::Relaxed);
        self.firefly_size.store(state.firefly_size, Ordering::Relaxed);
        self.firefly_expand_secs.store(state.firefly_expand_secs, Ordering::Relaxed);
        self.platform_texture.store(state.platform_texture, Ordering::Relaxed);
        self.background_texture.store(state.background_texture, Ordering::Relaxed);
        for i in 0..4 {
            self.platform_color_mask[i].store(state.platform_color_mask[i], Ordering::Relaxed);
            self.background_color_mask[i].store(state.background_color_mask[i], Ordering::Relaxed);
        }
        self.frame_number.store(state.frame_number, Ordering::Relaxed);
        self.render_frame_number.store(state.render_frame_number, Ordering::Relaxed);
        self.elapsed_secs.store(state.elapsed_secs, Ordering::Relaxed);
        self.render_elapsed_secs.store(state.render_elapsed_secs, Ordering::Relaxed);
        self.present_elapsed_secs.store(state.present_elapsed_secs, Ordering::Relaxed);
        self.photodiode_white.store(state.photodiode_white, Ordering::Relaxed);
        self.camera_radius.store(state.camera_radius, Ordering::Relaxed);
        self.camera_x.store(state.camera_x, Ordering::Relaxed);
        self.camera_y.store(state.camera_y, Ordering::Relaxed);
        self.camera_z.store(state.camera_z, Ordering::Relaxed);
        self.attempts.store(state.attempts, Ordering::Relaxed);
        self.current_alignment.store(state.current_alignment, Ordering::Relaxed);
        self.current_angle.store(state.current_angle, Ordering::Relaxed);
        self.is_animating.store(state.is_animating, Ordering::Relaxed);
        self.is_blank.store(state.is_blank, Ordering::Relaxed);
        self.is_rendering_stopped.store(state.is_rendering_stopped, Ordering::Relaxed);
        self.is_scene_ready.store(state.is_scene_ready, Ordering::Relaxed);
        self.win_time.store(state.win_time, Ordering::Relaxed);
        self.camera_speed_rotate.store(state.camera_speed_rotate, Ordering::Relaxed);
        self.camera_rotation_sense.store(state.camera_rotation_sense, Ordering::Relaxed);
    }

}

impl Default for SharedGameState {
    fn default() -> Self { Self::new() }
}

/// Ring buffer of recent frame states.  Each slot is a full `SharedGameState`
/// so that all existing read/write helpers work unchanged.  The game writes one
/// entry per matched render completion via `push`, advancing `write_head`
/// monotonically.
/// The controller drains unseen entries to backfill any frames missed during
/// polling gaps.
#[repr(C)]
#[derive(Debug)]
pub struct FrameRingBuffer {
    /// Monotonically increasing write counter.
    /// Slot index = `write_head % RING_BUFFER_SIZE`.
    /// Written with Release after filling the slot; read with Acquire by consumers.
    pub write_head: AtomicU64,
    pub entries: [SharedGameState; RING_BUFFER_SIZE],
}

impl FrameRingBuffer {
    pub fn new() -> Self {
        Self {
            write_head: AtomicU64::new(0),
            entries: core::array::from_fn(|_| SharedGameState::new()),
        }
    }

    /// Write a frame into the next ring slot and advance the head.
    pub fn push(&self, state: &SharedGameStateLocal) {
        let head = self.write_head.load(Ordering::Relaxed);
        let idx = (head as usize) % RING_BUFFER_SIZE;
        self.entries[idx].write_from_local(state);
        // Release ensures all entry stores are visible before the consumer sees the new head.
        self.write_head.store(head + 1, Ordering::Release);
    }
}

impl Default for FrameRingBuffer {
    fn default() -> Self { Self::new() }
}

/// Combined shared memory region between Controller and Game.
#[repr(C)]
#[derive(Debug)]
pub struct SharedMemory {
    pub command_seq: AtomicU64,
    pub command_ack: AtomicU64,
    pub commands: SharedCommands,
    pub game_structure_game: SharedGameState,
    pub game_structure_control: SharedGameState,
    pub frame_ring_buffer: FrameRingBuffer,
}

impl SharedMemory {
    pub fn new() -> Self {
        Self {
            command_seq: AtomicU64::new(0),
            command_ack: AtomicU64::new(0),
            commands: SharedCommands::new(),
            game_structure_game: SharedGameState::new(),
            game_structure_control: SharedGameState::new(),
            frame_ring_buffer: FrameRingBuffer::new(),
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
