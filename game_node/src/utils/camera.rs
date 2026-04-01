//! Camera handler.
use bevy::prelude::*;
use crate::utils::objects::GameStateLocal;

use crate::utils::objects::PersistentCamera;

/// Reference horizontal FOV (radians). Derived from 60° vertical at 16:9.
/// All devices will see exactly this horizontal angle regardless of aspect ratio.
const REF_HFOV: f32 = std::f32::consts::FRAC_PI_3; // ≈ 60° horizontal

/// This camera persists across resets to avoid artifacts.
pub fn spawn_persistent_camera(mut commands: Commands, local_game_struct: Res<GameStateLocal>) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: REF_HFOV, // will be corrected immediately by adapt_camera_fov
            ..default()
        }),
        Transform::from_xyz(
            f32::from_bits(local_game_struct.0.camera_x),
            f32::from_bits(local_game_struct.0.camera_y),
            f32::from_bits(local_game_struct.0.camera_z),
        )
        .looking_at(Vec3::ZERO, Vec3::Y),
        PersistentCamera,
    ));
}