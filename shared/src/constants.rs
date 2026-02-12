// Constants used in the game_node and shared across libraries.

/// Generic game constants
pub mod game_constants {
    pub const REFRESH_RATE_HZ: f64 = 60.0; // Hz

    pub const UNLOCK_SOL_NR: usize = 3; // Number of consecutive correct disalignments to unlock

    // Cosine alignment with door to win
    pub const COSINE_ALIGNMENT_TO_WIN: f32 = 0.95; // approx ~8 degrees

    // Seeds for the random number generator, one per face.
    // If two faces share the same seed (and same count/size), they get identical decorations.
    pub const DECORATION_SEEDS: [u64; 3] = [69, 70, 71];

    // UI responsive design reference
    pub const UI_REFERENCE_HEIGHT: f32 = 1080.0; // 1080p as reference

    // Score bar UI constants (scaled values)
    pub const SCORE_BAR_WIDTH_PERCENT: f32 = 40.0; // 40% of screen width
    pub const SCORE_BAR_HEIGHT: f32 = 20.0; // pixels (scaled by UiScale)
    pub const SCORE_BAR_TOP_OFFSET: f32 = 50.0; // pixels from top (scaled by UiScale)
    pub const SCORE_BAR_BORDER_THICKNESS: f32 = 2.0; // pixels (scaled by UiScale)

    // Loading screen duration in seconds (time for scene to render/stabilize)
    pub const LOADING_DURATION_SECS: f32 = 0.3;
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
    pub const CAMERA_3D_MIN_RADIUS: f32 = 12.0;
    pub const CAMERA_3D_MAX_RADIUS: f32 = 20.0;
}

/// Game objects
pub mod object_constants {
    // Y position from the ground plane.
    pub const GROUND_Y: f32 = 0.0;
}

/// Pyramid object
pub mod pyramid_constants {

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

    // Index of the target door of the pyramid
    pub const PYRAMID_TARGET_DOOR_INDEX: usize = 0;

    // Decorations
    pub const DECORATION_COUNT: u32 = 50;
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

    pub const SPOTLIGHT_LIGHT_INTENSITY: f32 = 5_000_000.0;
    pub const GLOBAL_AMBIENT_LIGHT_INTENSITY: f32 = 200.0;
    pub const MAX_SPOTLIGHT_INTENSITY: f32 = 1000000.0;
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
