//! Command handler
//! This module reads from Shared Memory and updates the game resources (`PendingRotation`, etc.).

use crate::utils::objects::{GameStateLocal, PersistentCamera};
use bevy::prelude::*;
use bevy::time::Real;
use core::sync::atomic::Ordering;
use shared::constants::camera_3d_constants::{
    CAMERA_3D_SPEED_ZOOM, CAMERA_MOVEMENT_MAX_CATCHUP_FRAMES,
    CAMERA_MOVEMENT_REFERENCE_HZ,
};
#[cfg(not(target_arch = "wasm32"))]
use shared::create_shared_memory;
use shared::SharedMemoryHandle;

/// Wrapper to access shared memory as a bevy resource
#[derive(Resource)]
pub struct SharedMemResource(pub SharedMemoryHandle);

/// Tracks the last command sequence number processed by the game.
#[derive(Resource, Default)]
pub struct LastCommandSeq(pub u64);

/// Local copy of pending commands read from shared memory
#[derive(Resource, Default)]
pub struct PendingCommands {
    pub reset: bool,
    pub rotation: f32,
    pub zoom: f32,
    pub check_alignment: bool,
    pub toggle_blank: bool,
    pub toggle_stop_rendering: bool,
    pub animation_door: bool,
    pub animation_all_door: bool,
    pub animation_colored: bool,
    pub shake: bool,
}

/// Bevy plugin to read shared memory commands and update local game state
pub struct SharedMemoryReaderPlugin;

impl Plugin for SharedMemoryReaderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingCommands>();
        app.init_resource::<LastCommandSeq>();
    }
}

#[cfg_attr(target_arch = "wasm32", allow(unused_variables, unused_mut))]
pub fn init_shared_memory_system(mut commands: Commands) {
    let name = "monkey_game";

    #[cfg(not(target_arch = "wasm32"))]
    {
        match create_shared_memory(name) {
            Ok(handle) => {
                info!("Shared Memory initialized successfully.");
                commands.insert_resource(SharedMemResource(handle));
            }
            Err(e) => {
                error!("Failed to initialize shared memory: {}", e);
            }
        }
    }
}

pub fn clear_pending_commands(mut pending_commands: ResMut<PendingCommands>) {
    *pending_commands = PendingCommands::default();
}

pub fn read_shared_memory_commands(
    shm_res: Option<Res<SharedMemResource>>,
    mut pending_commands: ResMut<PendingCommands>,
    mut last_seq: ResMut<LastCommandSeq>,
    mut camera_query: Query<&mut Transform, With<PersistentCamera>>,
    time_real: Res<Time<Real>>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();

    // Read EVERY tick (no seq gate). The seq gate below skips
    let speed_rotate = f32::from_bits(
        shm.game_structure_control
            .camera_speed_rotate
            .load(Ordering::Relaxed),
    );
    let rotation_sense = shm
        .game_structure_control
        .camera_rotation_sense
        .load(Ordering::Relaxed) as i32 as f32;
    // Preserve the authored speed at 60 Hz while compensating for small late
    // frames. Cap catch-up so a long browser pause cannot cause a large jump.
    let movement_scale = (time_real.delta_secs() * CAMERA_MOVEMENT_REFERENCE_HZ)
        .clamp(0.0, CAMERA_MOVEMENT_MAX_CATCHUP_FRAMES);
    let signed_speed = speed_rotate * rotation_sense * movement_scale;

    if shm.commands.rotate_left.load(Ordering::Relaxed) {
        pending_commands.rotation -= signed_speed;
    }
    if shm.commands.rotate_right.load(Ordering::Relaxed) {
        pending_commands.rotation += signed_speed;
    }
    if shm.commands.zoom_in.load(Ordering::Relaxed) {
        pending_commands.zoom -= CAMERA_3D_SPEED_ZOOM * movement_scale;
    }
    if shm.commands.zoom_out.load(Ordering::Relaxed) {
        pending_commands.zoom += CAMERA_3D_SPEED_ZOOM * movement_scale;
    }

    if let Ok(mut cam_transform) = camera_query.single_mut() {
        let cam_y = f32::from_bits(
            shm.game_structure_control
                .camera_y
                .load(Ordering::Relaxed),
        );
        let cam_z = f32::from_bits(
            shm.game_structure_control
                .camera_z
                .load(Ordering::Relaxed),
        );

        cam_transform.translation.y = cam_y;
        cam_transform.translation.z = cam_z;
    }

    // ─── Edge-triggered: one-shot commands gated by seq ────────────────────
    let seq = shm.command_seq.load(Ordering::Acquire);
    if seq == last_seq.0 {
        return;
    }
    pending_commands.toggle_stop_rendering = shm.commands.toggle_stop_rendering.load(Ordering::Relaxed);
    pending_commands.animation_door = shm.commands.animation_door.load(Ordering::Relaxed);
    pending_commands.check_alignment = shm.commands.check_alignment.load(Ordering::Relaxed);
    pending_commands.toggle_blank = shm.commands.toggle_blank.load(Ordering::Relaxed);
    pending_commands.reset = shm.commands.reset.load(Ordering::Relaxed);
    pending_commands.animation_all_door = shm.commands.animation_all_door.load(Ordering::Relaxed);
    pending_commands.animation_colored = shm.commands.animation_colored.load(Ordering::Relaxed);
    pending_commands.shake = shm.commands.shake.load(Ordering::Relaxed);

    // Acknowledge: tell the controller we processed this batch.
    // Release ensures all reads above are complete before the controller sees the ack.
    shm.command_ack.store(seq, Ordering::Release);
    last_seq.0 = seq;
}

// Read shared memory to local structure (from game to local)
pub fn read_shared_memory_game_state_local(
    shm_res: Option<Res<SharedMemResource>>,
    mut local_game_struct: ResMut<GameStateLocal>,
    last_seq: ResMut<LastCommandSeq>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();

    // Check if there are new commands (seq != last processed)
    let seq = shm.command_seq.load(Ordering::Acquire);
    if seq == last_seq.0 {
        return; // Already processed this command batch
    }

    let gs_game = &shm.game_structure_control;

    // Update local to copy
    local_game_struct.0 = gs_game.to_not_atomic();
}

/// Refresh "live" controller-owned fields every frame, bypassing the seq
/// gate above. These are values the controller updates continuously
/// (score bar, shake config) which the game needs to react to without
/// waiting for a trial reset.
pub fn sync_live_state_from_shm(
    shm_res: Option<Res<SharedMemResource>>,
    mut local_game_struct: ResMut<GameStateLocal>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();
    let gs = &shm.game_structure_control;
    // Level chain: the controller pushes the new count when it issues the
    // terminal door animation, so this has to land within the same trial —
    // the seq-gated snapshot copy only refreshes at trial reset.
    local_game_struct.0.progress_bar_size = gs.progress_bar_size.load(Ordering::Relaxed);
    local_game_struct.0.progress_bar_cur_size = gs.progress_bar_cur_size.load(Ordering::Relaxed);
    local_game_struct.0.score_bar_value = gs.score_bar_value.load(Ordering::Relaxed);
    local_game_struct.0.score_bar_max = gs.score_bar_max.load(Ordering::Relaxed);
    local_game_struct.0.session_time_left = gs.session_time_left.load(Ordering::Relaxed);
    local_game_struct.0.correct_streak = gs.correct_streak.load(Ordering::Relaxed);
    local_game_struct.0.shake_amplitude = gs.shake_amplitude.load(Ordering::Relaxed);
    local_game_struct.0.shake_duration = gs.shake_duration.load(Ordering::Relaxed);
}
