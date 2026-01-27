//! Implementation of a 3D first-person orbit camera plugin for monkey_3d_game.

use crate::command_handler::{PendingRotation, PendingZoom};
use crate::utils::constants::camera_3d_constants::{
    CAMERA_3D_INITIAL_Y, CAMERA_3D_MAX_RADIUS, CAMERA_3D_MIN_RADIUS, CAMERA_3D_SPEED_X,
    CAMERA_3D_SPEED_Z,
};
use crate::utils::objects::{GameState, RotableComponent};
use bevy::prelude::*;

/// Controls the 3D camera, rotating the main pyramid (A/D) and its platform and zooms in/out with W/S.
pub fn camera_3d_fpov_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    timer: Res<Time>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
    mut rot_entities: Query<&mut Transform, (With<RotableComponent>, Without<Camera3d>)>,
    gamestate: Res<crate::utils::objects::GameState>,
) {
    if gamestate.is_animating {
        return; // Do not allow camera inputs while animating
    }
    // Set the camera's movement and zoom speed
    let speed = CAMERA_3D_SPEED_X * timer.delta_secs();
    let zoom_speed = CAMERA_3D_SPEED_Z * timer.delta_secs();

    // Check for keyboard inputs for camera movement
    let left = keyboard.pressed(KeyCode::ArrowLeft) || keyboard.pressed(KeyCode::KeyA);
    let right = keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD);
    let up = keyboard.pressed(KeyCode::ArrowUp) || keyboard.pressed(KeyCode::KeyW);
    let down = keyboard.pressed(KeyCode::ArrowDown) || keyboard.pressed(KeyCode::KeyS);

    // Update Camera zoom by updating camera (up/down)
    if up || down {
        let Ok(mut transform) = camera_query.single_mut() else {
            return;
        };
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        let mut radius = transform.translation.xz().length();
        if up {
            radius -= zoom_speed;
        }
        if down {
            radius += zoom_speed;
        }
        // Clamp the camera's zoom level to a specific range.
        radius = radius.clamp(CAMERA_3D_MIN_RADIUS, CAMERA_3D_MAX_RADIUS);

        transform.translation = Vec3::new(
            radius * yaw.sin(),
            CAMERA_3D_INITIAL_Y, // Keep the camera at the same height.
            radius * yaw.cos(),
        );
        // Make the camera always look at the origin.
        transform.look_at(Vec3::ZERO, Vec3::Y);
    }
    // Rotate all the rotable entities around the origin based on camera input
    else if left || right {
        for mut rot_entity_transform in &mut rot_entities {
            // Get the entity's current rotation and radius from the origin.
            let (mut yaw, _, _) = rot_entity_transform.rotation.to_euler(EulerRot::YXZ);

            yaw += if left {
                -speed
            } else if right {
                speed
            } else {
                0.
            };

            rot_entity_transform.rotation = Quat::from_rotation_y(yaw);
        }
    }
}

// ============================================================================
// COMMAND-DRIVEN FUNCTIONS (used by Twin-Engine architecture)
// ============================================================================

/// Apply rotation to all rotable entities by the given delta (in radians).
/// Positive delta rotates right, negative rotates left.
pub fn apply_rotation(
    delta: f32,
    rot_entities: &mut Query<&mut Transform, (With<RotableComponent>, Without<Camera3d>)>,
) {
    for mut rot_entity_transform in rot_entities.iter_mut() {
        let (mut yaw, _, _) = rot_entity_transform.rotation.to_euler(EulerRot::YXZ);
        yaw += delta;
        rot_entity_transform.rotation = Quat::from_rotation_y(yaw);
    }
}

/// Apply zoom to the camera by the given delta.
/// Positive delta zooms out, negative zooms in.
pub fn apply_zoom(
    delta: f32,
    camera_query: &mut Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };
    let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
    let mut radius = transform.translation.xz().length();

    radius += delta;
    radius = radius.clamp(CAMERA_3D_MIN_RADIUS, CAMERA_3D_MAX_RADIUS);

    transform.translation = Vec3::new(
        radius * yaw.sin(),
        CAMERA_3D_INITIAL_Y,
        radius * yaw.cos(),
    );
    transform.look_at(Vec3::ZERO, Vec3::Y);
}

// ============================================================================
// SYSTEMS FOR PENDING ACTIONS
// ============================================================================

/// System that applies pending rotation from commands.
pub fn apply_pending_rotation(
    pending: Res<PendingRotation>,
    gamestate: Res<GameState>,
    mut rot_entities: Query<&mut Transform, (With<RotableComponent>, Without<Camera3d>)>,
) {
    if gamestate.is_animating || pending.0.abs() < 0.0001 {
        return;
    }
    apply_rotation(pending.0, &mut rot_entities);
}

/// System that applies pending zoom from commands.
pub fn apply_pending_zoom(
    pending: Res<PendingZoom>,
    gamestate: Res<GameState>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
) {
    if gamestate.is_animating || pending.0.abs() < 0.0001 {
        return;
    }
    apply_zoom(pending.0, &mut camera_query);
}

