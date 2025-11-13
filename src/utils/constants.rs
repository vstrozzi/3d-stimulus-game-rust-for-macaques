// This file contains all the constants used in the game, organized into modules.

/// Constants for the 3D camera.
pub mod camera_3d_constants {
    // The initial X position of the camera.
    pub const CAMERA_3D_INITIAL_X: f32 = 0.0;
    // The initial Y position of the camera.
    pub const CAMERA_3D_INITIAL_Y: f32 = 0.5;
    // The initial Z position of the camera.
    pub const CAMERA_3D_INITIAL_Z: f32 = 8.0;

    // The speed at which the camera rotates around the X-axis.
    pub const CAMERA_3D_SPEED_X: f32 = 2.0;
    // The speed at which the camera zooms along the Z-axis.
    pub const CAMERA_3D_SPEED_Z: f32 = 4.0;

    // The minimum radius for the camera's orbit.
    pub const CAMERA_3D_MIN_RADIUS: f32 = 5.0;
    // The maximum radius for the camera's orbit.
    pub const CAMERA_3D_MAX_RADIUS: f32 = 50.0;
}

/// Constants for game objects.
pub mod object_constants {
    // The Y position of the ground plane.
    pub const GROUND_Y: f32 = 0.0;
}

/// Constants for the pyramid object.
pub mod pyramid_constants {
    use bevy::prelude::Color;

    // The minimum radius of the pyramid's base.
    pub const PYRAMID_BASE_RADIUS_MIN: f32 = 1.0;
    // The maximum radius of the pyramid's base.
    pub const PYRAMID_BASE_RADIUS_MAX: f32 = 5.0;

    // The minimum height of the pyramid.
    pub const PYRAMID_HEIGHT_MIN: f32 = 2.0;
    // The maximum height of the pyramid.
    pub const PYRAMID_HEIGHT_MAX: f32 = 7.0;

    // The minimum angle offset for the pyramid's base in radians.
    pub static PYRAMID_ANGLE_OFFSET_RAD_MIN: f32 = 0.0 * (std::f32::consts::PI / 180.0);
    // The maximum angle offset for the pyramid's base in radians.
    pub static PYRAMID_ANGLE_OFFSET_RAD_MAX: f32 = 360.0 * (std::f32::consts::PI / 180.0);

    // The angle increment for each side of the pyramid's base in radians.
    pub const PYRAMID_ANGLE_INCREMENT_RAD: f32 = 120.0 * (std::f32::consts::PI / 180.0);

    // The colors for each face of the pyramid.
    pub const PYRAMID_COLORS: [Color; 3] = [
        Color::srgb(1.0, 0.2, 0.2),
        Color::srgb(0.2, 0.5, 1.0),
        Color::srgb(0.2, 1.0, 0.3),
    ];

    // The index of the target face of the pyramid.
    pub const PYRAMID_TARGET_FACE_INDEX: usize = 0;

    // The minimum number of decorations on a pyramid face.
    pub const DECORATION_COUNT_MIN: usize = 10;
    // The maximum number of decorations on a pyramid face.
    pub const DECORATION_COUNT_MAX: usize = 100;
    // The minimum size of a decoration on a pyramid face.
    pub const DECORATION_SIZE_MIN: f32 = 0.05;
    // The maximum size of a decoration on a pyramid face.
    pub const DECORATION_SIZE_MAX: f32 = 0.15;
}

/// Generic game constants.
pub mod game_constants {
    // The refresh rate of the game in hertz.
    pub const REFRESH_RATE_HZ: f64 = 60.0; // Hz

    // The seed for the random number generator.
    pub const SEED: u64 = 69;

    // The path to the font file.
    pub const FONT_PATH: &str = "fonts/Roboto/Roboto-VariableFont_wdth,wght.ttf";

    // The cosine of the angle between the camera and the face normal, used to determine if the camera is looking at the correct face.
    pub const COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD: f32 = -0.9; // Allowed misalignment camera and correct face normal
}
