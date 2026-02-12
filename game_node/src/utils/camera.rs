//! Implementation of a 3D first-person orbit camera plugin for monkey_3d_game.

use crate::command_handler::{PendingRotation, PendingZoom};

use crate::utils::objects::RotableComponent;
use std::sync::atomic::Ordering;
use bevy::prelude::*;
use crate::command_handler::SharedMemResource;
use shared::constants::camera_3d_constants::{
    CAMERA_3D_INITIAL_Y, CAMERA_3D_MAX_RADIUS, CAMERA_3D_MIN_RADIUS,
};

/// Apply rotation to all rotable entities by the given delta (in radians).
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
pub fn apply_zoom(delta: f32, camera_query: &mut Query<&mut Transform, With<Camera3d>>) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };
    let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
    let mut radius = transform.translation.xz().length();

    radius += delta;
    radius = radius.clamp(CAMERA_3D_MIN_RADIUS, CAMERA_3D_MAX_RADIUS);

    transform.translation = Vec3::new(radius * yaw.sin(), CAMERA_3D_INITIAL_Y, radius * yaw.cos());
    transform.look_at(Vec3::ZERO, Vec3::Y);
}

/// System that applies pending rotation from commands.
pub fn apply_pending_rotation(
    pending: Res<PendingRotation>,
    mut rot_entities: Query<&mut Transform, (With<RotableComponent>, Without<Camera3d>)>,
    shm_res: Option<Res<SharedMemResource>>,

) {
    // Read shared memory
    let Some(shm_res) = shm_res else {
        error!("Shared Memory not initialized in setup_round");
        return;
    };

    let shm = shm_res.0.get();

    let is_animating = shm.game_structure_game.is_animating.load(Ordering::Relaxed);


    if is_animating || pending.0.abs() < 0.0001 {
        return;
    }
    apply_rotation(pending.0, &mut rot_entities);
}

/// System that applies pending zoom from commands.
pub fn apply_pending_zoom(
    pending: Res<PendingZoom>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
    shm_res: Option<Res<SharedMemResource>>,
) {

    // Read shared memory
    let Some(shm_res) = shm_res else {
        error!("Shared Memory not initialized in setup_round");
        return;
    };

    let shm = shm_res.0.get();

    let is_animating = shm.game_structure_game.is_animating.load(Ordering::Relaxed);


    if is_animating || pending.0.abs() < 0.0001 {
        return;
    }
    apply_zoom(pending.0, &mut camera_query);
}
