use bevy::{input::mouse::MouseMotion, prelude::*};

use crate::utils::constants::camera_3d_constants;

pub struct Camera3dFpovPlugin;

impl Plugin for Camera3dFpovPlugin{
    fn build(&self, app: &mut App){
        app
        .add_systems(
            Update,
            (
                camera_3d_fpov_keyboard,
                camera_3d_fpov_mouse,
            ),
        );
    }
}
/// 3D Camera System
/// A simple first-person "look" camera system.
pub fn camera_3d_fpov_keyboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
) {
    // Get Camera3D Transform
    let Ok(mut transform) = camera_query.single_mut() else {
        // No camera, or more than one camera
        return;
    };

    let speed: f32 = camera_3d_constants::CAMERA_3D_SPEED * time.delta_secs();

    // Get the camera's local axes
    let strafe_vector = transform.local_x();
    let full_forward_vector = transform.local_z();

    // 1. Create a "flat" forward vector by ignoring the Y component
    let mut flat_forward = Vec3::new(full_forward_vector.x, 0.0, full_forward_vector.z);

    // 2. Normalize the flat vector to prevent slower movement when looking down.
    //    normalize_or_zero() handles the case where you look straight up or down (forward_flat would be zero).
    flat_forward = flat_forward.normalize_or_zero();

    // Strafe (A/D)
    if keyboard.pressed(KeyCode::ArrowLeft) || keyboard.pressed(KeyCode::KeyA) {
        // Use the unchanged local_x for strafing
        transform.translation -= strafe_vector * speed;
    }
    if keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD) {
        transform.translation += strafe_vector * speed;
    }

    // Forward/Backward (W/S)
    if keyboard.pressed(KeyCode::ArrowUp) || keyboard.pressed(KeyCode::KeyW) {
        // Use the *new flat_forward* vector
        transform.translation -= flat_forward * speed;
    }
    if keyboard.pressed(KeyCode::ArrowDown) || keyboard.pressed(KeyCode::KeyS) {
        transform.translation += flat_forward * speed;
    }

    // --- Y-Axis Movement (Flying) ---

    // Optional: Add vertical movement (like creative mode)
    if keyboard.pressed(KeyCode::Space) {
        transform.translation.y += speed; // Move straight up
    }
    if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ControlLeft) {
        transform.translation.y -= speed; // Move straight down
    }
}

pub fn camera_3d_fpov_mouse(
    mut mouse_motion_events: MessageReader<MouseMotion>,
    mut camera_query: Query<&mut Transform, With<Camera>>,
    time: Res<Time>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        // No camera, or more than one camera
        return;
    };
    let mut yaw = 0.0;
    let mut pitch = 0.0;

    let sensitivity = camera_3d_constants::CAMERA_3D_SENSITIVITY * time.delta_secs();

    for event in mouse_motion_events.read() {
        yaw -= event.delta.x * sensitivity;
        pitch -= event.delta.y * sensitivity;
    }

    transform.rotate(Quat::from_rotation_y(yaw));
    transform.rotate(Quat::from_rotation_x(pitch));
}
