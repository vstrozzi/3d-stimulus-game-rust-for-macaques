//! This module collects game state and writes it to atomic shared memory

use bevy::{prelude::*};
use crate::shared_memory::shared_memory_reader::{SharedMemResource};
use crate::utils::objects::{BaseDoor, RoundStartTimestamp, GameStateLocal, GameConditions, BlankScreen};

// Count FixedUpdate ticks since beginning of game
#[derive(Resource, Default)]
pub struct FrameCounterResource(pub u64);

// Count render frames (Update ticks) since beginning of game
#[derive(Resource, Default)]
pub struct RenderFrameCounterResource(pub u64);

// Update the shared memory game state after every game loop update
pub struct StateEmitterPlugin;

impl Plugin for StateEmitterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameCounterResource>()
           .init_resource::<RenderFrameCounterResource>();
    }
}

/// Increment the global timing variables
pub fn increment_timing(
    mut counter: ResMut<FrameCounterResource>,
    time: Res<Time>,
    mut round_start: ResMut<RoundStartTimestamp>,
    game_conditions: Res<GameConditions>,
) {
    // Increment the frames regardless
    counter.0 += 1;
    
    if game_conditions.stop_rendering {
        return;
    }

    // Add the delta 
    if let Some(ref mut total) = round_start.0 {
        *total += time.delta();
    }
}

/// Update local memory
pub fn update_shared_memory_local(
    mut game_state_local: ResMut<GameStateLocal>,
    frame_counter: Res<FrameCounterResource>,
    round_start: Res<RoundStartTimestamp>,
    camera_query: Query<&Transform, With<Camera3d>>,
    door_query: Query<(&BaseDoor, &Transform)>,
    game_conditions: ResMut<GameConditions>,
    black_screen_query: Query<Entity, With<BlankScreen>>,
) {
    game_state_local.0.is_blank = !black_screen_query.is_empty();
    game_state_local.0.is_rendering_stopped = game_conditions.stop_rendering;
    game_state_local.0.is_scene_ready = game_conditions.is_scene_ready;
    game_state_local.0.frame_number = frame_counter.0;
    game_state_local.0.elapsed_secs = round_start.0
        .map(|t| t.as_secs_f32().to_bits())
        .unwrap_or(0.0_f32.to_bits());

    let Ok(camera_transform) = camera_query.single() else { return };

    let pos = camera_transform.translation;
    game_state_local.0.camera_radius = pos.xz().length().to_bits();
    game_state_local.0.camera_x = pos.x.to_bits();
    game_state_local.0.camera_y = pos.y.to_bits();
    game_state_local.0.camera_z = pos.z.to_bits();

    let target_door_idx = game_state_local.0.target_door as usize;
    let camera_forward = camera_transform.forward();
    let camera_forward_xz = Vec3::new(camera_forward.x, 0.0, camera_forward.z).normalize_or_zero();

    for (door, door_transform) in &door_query {
        if door.door_index == target_door_idx {
            let door_normal_world = door_transform.rotation * door.normal;
            let door_normal_xz = Vec3::new(door_normal_world.x, 0.0, door_normal_world.z).normalize_or_zero();
            let alignment = door_normal_xz.dot(camera_forward_xz);
            game_state_local.0.current_alignment = alignment.to_bits();
            game_state_local.0.current_angle = alignment.clamp(-1.0, 1.0).acos().to_bits();
            break;
        }
    }
}

/// Increment render frame counter and write render-scoped fields directly to SHM.
/// Runs in Update (once per render frame), not FixedUpdate.
/// Writes: render_frame_number, render_elapsed_secs, photodiode_white.
pub fn increment_render_frame_counter(
    mut counter: ResMut<RenderFrameCounterResource>,
    shm_res: Option<Res<SharedMemResource>>,
    round_start: Res<RoundStartTimestamp>,
    photodiode_query: Query<(&Visibility, &BackgroundColor), With<crate::utils::debug_functions::PhotodiodeMarker>>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    counter.0 += 1;
    if let Some(shm_res) = shm_res {
        let shm = shm_res.0.get();
        let gs = &shm.game_structure_game;
        gs.render_frame_number.store(counter.0, Relaxed);

        // Render-frame timestamp (same clock as elapsed_secs but sampled in Update)
        let render_secs = round_start.0
            .map(|t| t.as_secs_f32().to_bits())
            .unwrap_or(0.0_f32.to_bits());
        gs.render_elapsed_secs.store(render_secs, Relaxed);

        // Photodiode state: true when visible AND white
        let is_white = photodiode_query.iter().any(|(vis, bg)| {
            *vis != Visibility::Hidden && bg.0 == Color::WHITE
        });
        gs.photodiode_white.store(is_white, Relaxed);
    }
}

/// Write shared memory from the local game state to shared memory to be read by controller
pub fn write_shared_memory_game_state(
    shm_res: Option<Res<SharedMemResource>>,
    game_state_local: Res<GameStateLocal>,
) {

    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();
    let gs_game = &shm.game_structure_game;

    // Update based on current values
    gs_game.write_from_local(&game_state_local.0);

    // Also push into the ring buffer so the controller can catch skipped frames
    shm.frame_ring_buffer.push(&game_state_local.0);
}
