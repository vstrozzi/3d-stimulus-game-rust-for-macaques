//! Implementation of a 3D first-person orbit camera plugin for monkey_3d_game.

use crate::utils::constants::camera_3d_constants::{
    CAMERA_3D_INITIAL_Y, CAMERA_3D_MAX_RADIUS, CAMERA_3D_MIN_RADIUS, CAMERA_3D_SPEED_X,
    CAMERA_3D_SPEED_Z,
};
use crate::utils::objects::RotableComponent;
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
