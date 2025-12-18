// Constants used in the game, structured into modules.

/// 3D camera
pub mod camera_3d_constants {
    pub const CAMERA_3D_INITIAL_X: f32 = 0.0;
    pub const CAMERA_3D_INITIAL_Y: f32 = 0.5;
    pub const CAMERA_3D_INITIAL_Z: f32 = 8.0;

    pub const CAMERA_3D_SPEED_X: f32 = 2.0;
    pub const CAMERA_3D_SPEED_Z: f32 = 4.0;

    // Radius range for the camera's orbit.
    pub const CAMERA_3D_MIN_RADIUS: f32 = 5.0;
    pub const CAMERA_3D_MAX_RADIUS: f32 = 50.0;
}

/// Game objects
pub mod object_constants {
    // Y position from the ground plane.
    pub const GROUND_Y: f32 = 0.0;
}

/// Pyramid object
pub mod pyramid_constants {
    use bevy::prelude::Color;

    pub const PYRAMID_BASE_RADIUS_MIN: f32 = 1.0;
    pub const PYRAMID_BASE_RADIUS_MAX: f32 = 5.0;

    pub const PYRAMID_HEIGHT_MIN: f32 = 2.0;
    pub const PYRAMID_HEIGHT_MAX: f32 = 7.0;

    // Angle's offset for the pyramid's base in radians from the camera
    pub static PYRAMID_ANGLE_OFFSET_RAD_MIN: f32 = 0.0 * (std::f32::consts::PI / 180.0);
    pub static PYRAMID_ANGLE_OFFSET_RAD_MAX: f32 = 360.0 * (std::f32::consts::PI / 180.0);

    // Angle increment of each side of the pyramid's base in radians
    pub const PYRAMID_ANGLE_INCREMENT_RAD: f32 = 120.0 * (std::f32::consts::PI / 180.0);

    // Colors for each face of the pyramid
    pub const PYRAMID_COLORS: [Color; 3] = [
        Color::srgb(1.0, 0.2, 0.2),
        Color::srgb(0.2, 0.5, 1.0),
        Color::srgb(0.2, 1.0, 0.3),
    ];

    // Index of the target face of the pyramid
    pub const PYRAMID_TARGET_FACE_INDEX: usize = 0;

    // Number and size range of decorations on a pyramid face
    pub const DECORATION_COUNT_MIN: usize = 10;
    pub const DECORATION_COUNT_MAX: usize = 100;
    pub const DECORATION_SIZE_MIN: f32 = 0.05;
    pub const DECORATION_SIZE_MAX: f32 = 0.15;
}

/// Generic game constants
pub mod game_constants {
    pub const REFRESH_RATE_HZ: f64 = 60.0; // Hz

    // Seed for the random number generator.
    pub const SEED: u64 = 69;

    // Allowed misalignment camera and correct face normal (cosine of normal vectore camera and face angle)
    pub const COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD: f32 = -0.9; 
}
