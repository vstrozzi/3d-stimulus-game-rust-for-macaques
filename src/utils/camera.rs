use bevy::{prelude::*};
use crate::utils::constants::camera_3d_constants::{self, MAX_RADIUS, MIN_RADIUS};

pub struct Camera3dFpovPlugin;

impl Plugin for Camera3dFpovPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, camera_3d_fpov_inputs);
    }
}


/// Orbiting 3D Camera System
/// Rotates around the origin with A/D and zooms in/out with W/S
pub fn camera_3d_fpov_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    timer: Res<Time<Fixed>>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };

    // Orbit parameters
    let speed = camera_3d_constants::CAMERA_3D_SPEED_X * timer.delta_secs();
    let zoom_speed = camera_3d_constants::CAMERA_3D_SPEED_Z * timer.delta_secs();

    let (mut yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
    let mut radius = transform.translation.xz().length();

    // Handle Inputs
    let left  = keyboard.pressed(KeyCode::ArrowLeft)  || keyboard.pressed(KeyCode::KeyA);
    let right = keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD);
    let up    = keyboard.pressed(KeyCode::ArrowUp)    || keyboard.pressed(KeyCode::KeyW);
    let down  = keyboard.pressed(KeyCode::ArrowDown)  || keyboard.pressed(KeyCode::KeyS);

    // Check if *any* key is pressed
    let changed = left || right || up || down;

    // Update angles and radius
    if left  { yaw += speed; }
    if right { yaw -= speed; }

    if up    { radius -= zoom_speed; }
    if down  { radius += zoom_speed; }

    // Clamp zoom range
    radius = radius.clamp(MIN_RADIUS, MAX_RADIUS);


    // Compute new position relative to the origin
    if changed {
        transform.translation = Vec3::new(
        radius * yaw.sin(),
        0.0,  // keep same height
        radius * yaw.cos(),
        );
    }

    // Make the camera look at the origin
    transform.look_at(Vec3::ZERO, Vec3::Y);
}
