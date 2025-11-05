
/// Camera 3D
pub mod camera_3d_constants {
    pub const CAMERA_3D_INITIAL_X: f32 = 0.0;
    pub const CAMERA_3D_INITIAL_Y: f32 = 0.5;
    pub const CAMERA_3D_INITIAL_Z: f32 = 8.0;

    pub const CAMERA_3D_SPEED_X: f32 = 2.0;
    pub const CAMERA_3D_SPEED_Z: f32 = 4.0;

    pub const MIN_RADIUS: f32 = 5.0;
    pub const MAX_RADIUS: f32 = 20.0;
}

/// Object constants
pub mod object_constants {
    pub const GROUND_Y: f32 = 0.0;
}

// Pyramid constants
pub mod pyramid_constants {
    use bevy::prelude::Color;

    pub const PYRAMID_BASE_RADIUS: f32 = 1.0;
    pub const PYRAMID_HEIGHT: f32 = 2.0;
    pub const PYRAMID_ANGLE_OFFSET_RAD: f32 = 75.0 * (std::f32::consts::PI / 180.0);
    pub static PYRAMID_ANGLE_INCREMENT_RAD: f32 = 120.0 * (std::f32::consts::PI / 180.0);

    pub const PYRAMID_COLORS: [Color; 3] = [
        Color::srgb(1.0, 0.2, 0.2), 
        Color::srgb(0.2, 0.5, 1.0), 
        Color::srgb(0.2, 1.0, 0.3),
    ];

    pub const PYRAMID_TARGET_FACE_INDEX: usize = 0;
}

/// Game constants
pub mod game_constants {
    pub const REFRESH_RATE_HZ: f64 = 60.0; // Hz
}