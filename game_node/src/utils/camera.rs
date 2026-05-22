//! Camera handler
use bevy::prelude::*;
use crate::shared_memory::shared_memory_reader::PendingCommands;
use crate::utils::objects::{CameraShakeState, GameStateLocal, PersistentCamera};

/// Game camera, persistent across levels and trials
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

pub fn handle_camera_shake(
    pending: Res<PendingCommands>,
    local_game_struct: Res<GameStateLocal>,
    time: Res<Time>,
    mut shake: ResMut<CameraShakeState>,
    mut camera_query: Query<&mut Transform, With<PersistentCamera>>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };

    if pending.shake {
        let amplitude = f32::from_bits(local_game_struct.0.shake_amplitude);
        let duration = f32::from_bits(local_game_struct.0.shake_duration);
        if amplitude > 0.0 && duration > 0.0 {
            shake.start = Some(time.elapsed());
            shake.amplitude = amplitude;
            shake.duration = duration;
        }
    }

    let Some(start) = shake.start else { return };
    let elapsed = (time.elapsed() - start).as_secs_f32();
    if elapsed >= shake.duration {
        shake.start = None;
        return;
    }

    // Pitch + roll jitter only — yaw would change handle_zoom's extracted
    // euler-yaw next frame and drift the orbit position. look_at resets
    // rotation each frame, so no accumulation.
    let decay = (-4.0 * elapsed / shake.duration).exp();
    let t = time.elapsed_secs();
    let pitch = shake.amplitude * decay * (t * 37.0).sin() * 0.05;
    let roll  = shake.amplitude * decay * (t * 53.0 + 1.3).sin() * 0.05;
    transform.rotate_local_x(pitch);
    transform.rotate_local_z(roll);
}