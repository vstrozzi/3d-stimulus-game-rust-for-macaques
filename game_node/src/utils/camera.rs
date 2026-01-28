//! Implementation of a 3D first-person orbit camera plugin for monkey_3d_game.

use crate::command_handler::{PendingRotation, PendingZoom};
use crate::utils::constants::camera_3d_constants::{
    CAMERA_3D_INITIAL_Y, CAMERA_3D_MAX_RADIUS, CAMERA_3D_MIN_RADIUS,
};
use crate::utils::objects::{GameState, RotableComponent};
use bevy::prelude::*;



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

