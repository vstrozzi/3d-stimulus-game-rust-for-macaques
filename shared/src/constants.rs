// Constants used in the game_node and shared across libraries.
/// Generic game constants
pub mod game_constants {
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

    /// Default capacity of the left-side score bar. Controller-owned and
    /// session-persistent; starts at `max / 2` on session start.
    pub const SCORE_BAR_DEFAULT_MAX: u32 = 0;

    /// Amplitude of the camera shake effect on failed attempts.
    pub const SHAKE_AMPLITUDE_DEFAULT: f32 = 0.5;
    pub const SHAKE_DURATION_DEFAULT: f32 = 1.0;

    /// Duration of the black pre-start countdown.
    pub const LOADING_COUNTDOWN_SECS: f32 = 3.0;

    /// Radius of the round session clock in the top-right corner.
    pub const SESSION_CLOCK_RADIUS_PX: f32 = 40.0;

    /// Blink rate and peak overshoot of the level circle that is being filled
    /// in (or cleared) while the door animation plays.
    pub const PROGRESS_PULSE_HZ: f32 = 3.0;
    pub const PROGRESS_PULSE_SCALE: f32 = 0.45;

    /// Blink rate of the step being gained or lost on the trial-progress bar.
    /// Plays at the same time as the level circle above, so keep it in step
    /// with `PROGRESS_PULSE_HZ`.
    pub const TRIAL_BAR_PULSE_HZ: f32 = 2.0;
}

/// Audio toggles and levels. Both effects and the background loop are on by
/// default. Source files should be peak-equalized first (see
/// `game_node/src/scripts/equalize_audio.py`) so these volumes balance the
/// tracks against each other rather than fighting per-file peak differences.
pub mod sound_constants {
    /// Master switch for one-shot sound effects (win / hint). When `false`,
    /// no effect is spawned during door animations.
    pub const ENABLE_SOUND_EFFECTS: bool = true;

    /// Master switch for the looping background music. When `false`, the
    /// background track is never spawned.
    pub const ENABLE_BACKGROUND_MUSIC: bool = true;

    /// Linear playback volume for one-shot sound effects (1.0 = source level).
    pub const SOUND_EFFECTS_VOLUME: f32 = 1.0;

    /// Linear playback volume for the looping background music. Kept below the
    /// effects so the ambience doesn't mask the win/hint cues.
    pub const BACKGROUND_MUSIC_VOLUME: f32 = 0.20;
}

/// Offscreen render scaling.
pub mod render_constants {
    /// When `true`, the 3D scene is rendered to a fixed-resolution offscreen
    /// target (display aspect ratio, height capped at [`FIXED_RENDER_HEIGHT`])
    /// and upscaled to fill the native window. The UI (score bars, photodiode)
    /// is drawn separately at native resolution, so it stays crisp. Trades
    /// sharpness of the 3D scene for a stable, lower GPU cost.
    pub const RENDER_AT_FIXED_RESOLUTION: bool = true;

    /// Internal render target bounding box, in pixels. The 3D scene is scaled
    /// to fit inside `FIXED_RENDER_WIDTH` × `FIXED_RENDER_HEIGHT` while keeping
    /// the display aspect ratio (no distortion, no letterboxing). Bounding
    /// *both* dimensions caps the pixel count on wide displays — capping height
    /// alone let the width (and GPU cost) balloon on ultrawide / high-res
    /// monitors. Scale is clamped to <= 1 so we never render above native.
    pub const FIXED_RENDER_WIDTH: u32 = 1920;
    pub const FIXED_RENDER_HEIGHT: u32 = 1080;
}

/// 3D camera
pub mod camera_3d_constants {
    pub const CAMERA_3D_INITIAL_X: f32 = 0.0;
    pub const CAMERA_3D_INITIAL_Y: f32 = 1.;
    pub const CAMERA_3D_INITIAL_Z: f32 = 15.0;

    pub const CAMERA_3D_INITIAL_RADIUS: f32 = 15.0; 

    pub const CAMERA_3D_SPEED_ROTATE: f32 = 0.05;
    pub const CAMERA_3D_SPEED_ZOOM: f32 = 0.0;  // Set on 0 means no zoom

    // Radius range for the camera's orbit.
    pub const CAMERA_3D_MIN_RADIUS: f32 = 5.0;
    pub const CAMERA_3D_MAX_RADIUS: f32 = 50.0;

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

    // Lights color
    pub const LIGHT_RED:   bevy_color::Color = bevy_color::Color::srgba(0x8B as f32 / 255.0, 0x00 as f32 / 255.0, 0x00 as f32 / 255.0, 1.0);
    pub const LIGHT_GREEN: bevy_color::Color = bevy_color::Color::srgba(0xCC as f32 / 255.0, 0xFF as f32 / 255.0, 0x00 as f32 / 255.0, 1.0);
    pub const LIGHT_BLUE:  bevy_color::Color = bevy_color::Color::srgba(0x77 as f32 / 255.0, 0xB1 as f32 / 255.0, 0xD4 as f32 / 255.0, 1.0);

}
/// Mystical distance fog around the pyramid + gold firefly particles shown on
/// a winning animation. Fog is centered on the pyramid (origin): the scene is
/// within `FOG_START_RADIUS` of the center and dissolves into haze
/// beyond it. Fireflies spawn at the start of a correct (green) animation and
/// drift until the black screen appears.
pub mod fog_constants {
    use bevy_color::Color;

    /// Master switch for the distance fog.
    pub const FOG_ENABLED: bool = true;

    /// Clear radius (world units) around the pyramid center. Inside this sphere
    /// the scene stays sharp; beyond it the surroundings fade into fog. Keep it
    /// `>=` the pyramid radius (~2.5) so the pyramid edges don't get fogged.
    pub const FOG_START_RADIUS: f32 = 2.5;

    /// Fog density. Larger = the fog saturates over a shorter distance past
    /// `FOG_START_RADIUS` (thicker, hides the surroundings sooner). Full-fog
    /// distance from the camera is `start + FOG_THICKNESS_BASE / FOG_DENSITY`.
    pub const FOG_DENSITY: f32 = 0.5;

    /// Base transition thickness (divided by `FOG_DENSITY`), in world units.
    pub const FOG_THICKNESS_BASE: f32 = 25.0;

    /// Fog color — desaturated cold blue-grey for a mystical haze.
    pub const FOG_COLOR: Color = Color::srgb(0.55, 0.60, 0.70);

    /// Number of fireflies spawned on a win (the particle "density"). `0`
    /// disables them.
    pub const FIREFLY_COUNT: u32 = 200;

    /// Mean radius (world units) from the pyramid center where the fireflies
    /// drift. Sit them near the misty edge for the best glow-in-fog look.
    /// Keep `FIREFLY_RADIUS + FIREFLY_SPREAD` below the surrounding wall radius
    /// (9.0, see `setup_environment`) — nothing renders behind the wall.
    pub const FIREFLY_RADIUS: f32 = 6.0;

    /// Half-width of the radial band the fireflies are scattered within.
    pub const FIREFLY_SPREAD: f32 = 2.5;

    /// Wander speed multiplier (scales how fast each firefly drifts/twinkles).
    pub const FIREFLY_SPEED: f32 = 2.0;

    /// Seconds the fireflies take to burst out from the winning hole and
    /// reach their resting drift positions when they spawn.
    pub const FIREFLY_EXPAND_SECS: f32 = 1.5;

    /// Two-phase burst: in the first `FIREFLY_BURST_PHASE1` fraction of the
    /// expansion the fireflies shoot `FIREFLY_BURST_TOWARD_CAMERA` world units
    /// from the hole toward the player camera; the rest of the time they fan
    /// out to their resting positions.
    pub const FIREFLY_BURST_PHASE1: f32 = 1.0 / 5.0;
    pub const FIREFLY_BURST_TOWARD_CAMERA: f32 = 2.0;

    /// Particle size (world units).
    pub const FIREFLY_SIZE: f32 = 0.010;

    /// Emissive boost applied to `FIREFLY_COLOR` so the motes glow.
    pub const FIREFLY_GLOW: f32 = 500.0;

    /// Gold emissive color of the fireflies.
    pub const FIREFLY_COLOR: Color = crate::constants::pyramid_constants::LIGHT_GREEN;
}

/// Ambient "magic" motes drifting in a ring in front of the curved back wall.
/// Their number tracks the player's consecutive-correct streak, which the
/// controller pushes as `correct_streak`: no correct answer in a row means no
/// motes at all, and the ring then thickens step by step up to
/// `AMBIENT_COUNT_MAX`. The streak — and so the ring — restarts at 0 on every
/// new level.
pub mod ambient_particle_constants {
    use bevy_color::Color;

    /// Granularity: a streak at or above this shows the densest ring. With `5`
    /// the states are none / very few / few / okay / some / a good amount.
    pub const AMBIENT_STEPS: u32 = 5;

    /// Number of motes at the first step (streak 1) and at the last one. A
    /// streak of 0 always means 0 motes, whatever `AMBIENT_COUNT_MIN` says.
    /// `AMBIENT_COUNT_MAX` is also the size of the pool spawned at startup, so
    /// it is the only knob that costs anything: keep it small.
    pub const AMBIENT_COUNT_MIN: u32 = 3;
    pub const AMBIENT_COUNT_MAX: u32 = 48;

    /// Radial band the motes are scattered in, in world units from the pyramid
    /// center. The outer edge sits `AMBIENT_WALL_GAP` in front of the curved
    /// wall (radius 9.0, see `setup_environment`); the inner edge is about
    /// halfway between the platform rim (`BASE_RADIUS`, 5.0) and the wall.
    /// Setting both to the same value gives a thin ring.
    pub const AMBIENT_WALL_GAP: f32 = 1.0;
    pub const AMBIENT_INNER_RADIUS: f32 = 7.0;

    /// Angular width of the ring, in degrees, centered on the middle of the
    /// curved wall (the -Z side the camera faces). Motes outside it would sit
    /// behind or beside the camera, costing frames for nothing. `360.0` is the
    /// full circle again.
    pub const AMBIENT_ARC_DEG: f32 = 170.0;

    /// Height band above the ground the motes drift in.
    pub const AMBIENT_Y_MIN: f32 = 0.5;
    pub const AMBIENT_Y_MAX: f32 = 4.5;

    /// Mote size (world units), wander speed multiplier and emissive boost —
    /// same meaning as their `FIREFLY_*` counterparts.
    pub const AMBIENT_SIZE: f32 = 0.010;
    pub const AMBIENT_SPEED: f32 = 2.0;
    pub const AMBIENT_GLOW: f32 = 500.0;

    /// Mote color — kept distinct from the gold-green win burst.
    pub const AMBIENT_COLOR: Color = crate::constants::pyramid_constants::LIGHT_BLUE;
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

    pub const FAINT_ALIGNED_INTENSITY_FACTOR: f32 = 1.0 / 8.0;
    pub const FAINT_ALIGNED_SPOTLIGHT_FACTOR: f32 = 1.0 / 64.0;
    pub const FAINT_ALIGNED_SPOTLIGHT_RANGE: f32 = 4.0;
    pub const HOLE_SPOTLIGHT_RANGE: f32 = 25.0;
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

    /// Cosine threshold above which the "check" animation shows
    /// a colored hint instead of plain white.
    pub const COLOR_SUGGESTION_COS_SIM: f32 = 0.95;

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

    pub const MAX_SESSION_DURATION_MIN: u32 = 20;

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
        "start_object",
        "camera_y",
        "show_all",
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

