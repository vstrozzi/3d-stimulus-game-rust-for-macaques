//! Camera handler.
use bevy::prelude::*;
use crate::utils::objects::GameStateLocal;

use crate::utils::objects::PersistentCamera;

/// This camera persists across resets to avoid artifacts.
pub fn spawn_persistent_camera(mut commands: Commands, local_game_struct: Res<GameStateLocal>) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(
            f32::from_bits(local_game_struct.0.camera_x),
            f32::from_bits(local_game_struct.0.camera_y),
            f32::from_bits(local_game_struct.0.camera_z),

        )
        .looking_at(Vec3::ZERO, Vec3::Y),
        PersistentCamera,
    ));
}