// Constants used in the game_node and shared across libraries.
/// Generic game constants
pub mod game_constants {
    pub const REFRESH_RATE_HZ: f64 = 60.0; // Hz

    // Cosine alignment with door to win
    pub const COSINE_ALIGNMENT_TO_WIN: f32 = 0.95; // approx ~8 degrees

    // Seeds for the random number generator, one per face.
    // If two faces share the same seed (and same count/size), they get identical decorations.
    pub const DECORATIONS_SEEDS: [u64; 3] = [69, 70, 71];

    // UI responsive design reference
    pub const UI_REFERENCE_HEIGHT: f32 = 1080.0; // 1080p as reference

    // Score bar UI constants (scaled values)
    pub const SCORE_BAR_WIDTH_PERCENT: f32 = 40.0; // 40% of screen width
    pub const SCORE_BAR_HEIGHT: f32 = 20.0; // pixels (scaled by UiScale)
    pub const SCORE_BAR_TOP_OFFSET: f32 = 50.0; // pixels from top (scaled by UiScale)

    // Progress bar dots constants
    pub const PROGRESS_BAR_WRAP_AROUND_SIZE: u32 = 20;
    pub const PROGRESS_BAR_DOTS_SIZE: f32 = 20.0;
    /// Maximum number of dots ever displayed in the score bar. The pool is
    /// spawned once at startup and entities beyond `progress_bar_size` are
    /// hidden via `Node.display`. Controllers must keep `progress_bar_size`
    /// at or below this; excess dots silently won't show.
    pub const PROGRESS_BAR_MAX_SIZE: u32 = 100;
}

/// 3D camera
pub mod camera_3d_constants {
    pub const CAMERA_3D_INITIAL_X: f32 = 0.0;
    pub const CAMERA_3D_INITIAL_Y: f32 = 1.;
    pub const CAMERA_3D_INITIAL_Z: f32 = 15.0;

    pub const CAMERA_3D_INITIAL_RADIUS: f32 = 15.0; 

    pub const CAMERA_3D_SPEED_ROTATE: f32 = 0.05;
    pub const CAMERA_3D_SPEED_ZOOM: f32 = 0.10;

    // Radius range for the camera's orbit.
    pub const CAMERA_3D_MIN_RADIUS: f32 = 5.0;
    pub const CAMERA_3D_MAX_RADIUS: f32 = 150.0;

    /// Default rotation sense (`+1` or `-1`). Multiplies the rotation
    /// applied per tick, so `-1` swaps left/right at the level scope.
    pub const CAMERA_ROTATION_SENSE_DEFAULT: i32 = 1;
}

/// Game objects
pub mod object_constants {
    // Y position from the ground plane.
    pub const GROUND_Y: f32 = 0.0;
}

/// Pyramid object
pub mod pyramid_constants {
    use crate::{DecorationShape, Texture};

    pub const PYRAMID_BASE_RADIUS: f32 = 2.5;
    pub const PYRAMID_HEIGHT: f32 = 4.0;
    pub const PYRAMID_START_ANGLE_OFFSET_RAD: f32 = 0.0;

    // Angle's offset for the pyramid's base in radians from the camera
    pub const PYRAMID_ANGLE_OFFSET_RAD_MIN: f32 = 0.0 * (std::f32::consts::PI / 180.0);
    pub const PYRAMID_ANGLE_OFFSET_RAD_MAX: f32 = 360.0 * (std::f32::consts::PI / 180.0);

    // Angle increment of each side of the pyramid's base in radians
    pub const PYRAMID_ANGLE_INCREMENT_RAD: f32 = 120.0 * (std::f32::consts::PI / 180.0);

    pub const PYRAMID_COLORS: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 1.0], // red, green, blue, alpha
    [0.0, 1.0, 0.0, 1.0], // green
    [0.0, 0.0, 1.0, 1.0], // blue
    ];

    // Default Textures
    pub const PYRAMID_TEXTURES: [Texture; 3] = [
        Texture::Bark001_1K,
        Texture::ChristmasTreeOrnament021_1K,
        Texture::Fabric079_1K,
    ];

    // Number of decorations on each pyramid side
    pub const PYRAMID_DECORATIONS_COUNT: [u32; 3] = [
        50,
        20,
        10,
    ];
    // Size of decorations per face
    pub const PYRAMID_DECORATIONS_SIZE: [f32; 3] = [
        0.1,
        0.2,
        0.3,
    ];

    // Decorations Shape
    pub const PYRAMID_DECORATIONS_SHAPE: [DecorationShape; 3] = [
        DecorationShape::Circle,
        DecorationShape::Square,
        DecorationShape::Star,
    ];

    pub const PYRAMID_DECORATIONS_COLOR: [[f32; 4]; 3] = [
        [1.0, 1.0, 0.0, 1.0], // yellow
        [1.0, 0.0, 1.0, 1.0], // magenta
        [0.0, 1.0, 1.0, 1.0], // cyan
    ];

    pub const PYRAMID_DECORATIONS_TEXTURE: [Texture; 3] = [
        Texture::Bark001_1K,
        Texture::Bark001_1K,
        Texture::Bark001_1K,
    ];

    pub const PYRAMID_DECORATIONS_THICKNESS: [f32; 3] = [
        0.02,
        0.04,
        0.06,
    ];

    /// Per-face decoration rotation in degrees.
    /// `>= 0` is a fixed rotation; `-1` means each decoration on the face
    /// gets an independent random rotation.
    pub const PYRAMID_DECORATIONS_ROTATION: [i32; 3] = [0, 0, 0];

    // Index of the target door of the pyramid
    pub const PYRAMID_TARGET_DOOR_INDEX: usize = 0;

    // Wooden base
    pub const BASE_HEIGHT: f32 = 0.3;
    pub const BASE_RADIUS: f32 = PYRAMID_BASE_RADIUS * 2.0;
    pub const BASE_COLOR: [f32; 4] = [0.59, 0.29, 0.00, 1.0]; // brown
    pub const BASE_NR_SIDES: usize = 6; // multiple of 3
    pub const BASE_HOLES_LIGHT_Y_OFFSET: f32 = 0.0; // Y offset of the light holes from the Y of the holes itself
    pub const BASE_HOLES_LIGHT_OFFSET_CENTER: f32 = -0.4; // Offset of the light holes from the normal of center of the hole


    // Door animation timing
    pub const DOOR_ANIM_FADE_OUT: f32 = 0.5; // seconds
    pub const DOOR_ANIM_STAY_OPEN: f32 = 0.5; // seconds
    pub const DOOR_ANIM_FADE_IN: f32 = 0.5; // seconds
}

/// Lighting constants
pub mod lighting_constants {
    // Shadow settings
    #[cfg(target_arch = "wasm32")]
    pub const SHADOWS_ENABLED: bool = false;    // Need to disable shadowslight on WASM for weird artifacts
    #[cfg(not(target_arch = "wasm32"))]
    pub const SHADOWS_ENABLED: bool = true;

    pub const SPOTLIGHT_LIGHT_INTENSITY: f32 = 50_000_000.0;
    pub const GLOBAL_AMBIENT_LIGHT_INTENSITY: f32 = 100.0;
    pub const MAX_SPOTLIGHT_INTENSITY: f32 = 50_000_000.0;
}


/// Constants shared between the Python and JS controllers. Single source of
/// truth — both controllers import these via PyO3 / wasm-bindgen.
pub mod controller_constants {
    /// SHM segment name used by the native game and the Python controller.
    pub const SHM_NAME: &str = "monkey_game";

    /// Controller→game poll period.
    pub const POLLING_RATE_S: f32 = 0.001;

    /// If the game stops pushing frames for this long, the controller resyncs.
    pub const GAME_UNRESPONSIVENESS_THRESHOLD_S: f32 = 3.0;

    /// Cosine threshold (≈ cos(π/6)) above which the "check" animation shows
    /// a colored hint instead of plain white.
    pub const COLOR_SUGGESTION_COS_SIM: f32 = 0.8660254;

    /// Default camera Y when a level doesn't override it.
    pub const DEFAULT_CAMERA_Y: f32 = 1.0;

    /// Number of pyramid faces (and chains per level).
    pub const N_FACES: usize = 3;

    /// Color channels per face (RGBA).
    pub const N_COLOR_CHANNELS: usize = 4;

    /// Flat color array length = N_FACES * N_COLOR_CHANNELS.
    pub const N_COLOR_FLOATS: usize = N_FACES * N_COLOR_CHANNELS;

    /// Number of evenly-spaced start orientations sampled per trial
    /// (one per door of the hexagonal base = 2 × N_FACES).
    pub const N_START_ORIENTS: usize = N_FACES * 2;

    /// Upper bound on per-trial frame log length (20 min × 60 Hz).
    /// Both controllers preallocate fixed-size frame-log buffers sized to
    /// this so per-frame logging never triggers heap growth or GC during a
    /// trial. A trial that would exceed this is clamped and skips logging
    /// the overflow frames (with a warning).
    pub const MAX_TRIAL_FRAMES: usize = 72_000;

    /// SHM-direct state fields written into each frame entry of a trial log.
    /// Both the Python and JS controllers consume this list; the verifier
    /// reads the same keys. Add/rename a field here, both sides pick it up.
    pub const LOGGED_STATE_FIELDS: &[&str] = &[
        "frame_number",
        "render_frame_number",
        "present_elapsed_secs",
        "photodiode_white",
        "camera_radius",
        "camera_x",
        "camera_y",
        "camera_z",
        "attempts",
        "current_alignment",
        "current_angle",
        "is_animating",
        "is_blank",
        "is_rendering_stopped",
    ];

    /// Trial-config keys consumed only by the controller (never written to SHM).
    pub const CONTROLLER_META_FIELDS: &[&str] = &[
        "nr_attempts_to_win",
        "nr_attempts_suggestion",
        "nr_attempts_to_retroceed",
        "elapsed_time_to_win",
        "elapsed_time_to_retroceed",
        "start_trial",
        "camera_y",
    ];

    /// FSM state names. Used as string labels in logs and for cross-language
    /// consistency assertions.
    pub const FSM_STATES: &[&str] = &[
        "INIT",
        "WAITING_FOR_START",
        "PLAYING",
        "WAITING_ANIMATION_START",
        "WAITING_ANIMATION_END",
        "TRIAL_COMPLETE",
    ];

    /// Trial-outcome labels.
    pub const PROCEEDING_VALUES: &[&str] = &["ADVANCE", "STAY", "RETROCEED"];
}

/// Shared timing constants for stimulus experiments.
pub mod timing {
    use super::game_constants::REFRESH_RATE_HZ;

    /// Duration to show black screen after win (in frames)
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
