//! This module collects game state and writes it to atomic shared memory

use bevy::{prelude::*};
use crate::shared_memory::shared_memory_reader::{SharedMemResource};
use crate::utils::objects::{BaseDoor, RoundStartTimestamp, GameStateLocal, GameConditions, BlankScreen};

// Pause-aware game-logic tick counter. Ticked once per rendered frame in
// `PostUpdate::increment_timing`, but skipped while `stop_rendering` is true.
// Kept as a separate counter from `RenderFrameCounterResource` so the SHM
// `frame_number` field carries the "active game time" tick index that the
// controller relies on for trial timeout / FSM bookkeeping.
#[derive(Resource, Default)]
pub struct FrameCounterResource(pub u64);

// Render-frame counter. Ticked unconditionally in `Update::stage_render_sample`
// once per vsync; never pauses on `stop_rendering`. Drives the
// `render_frame_number` SHM field used for photodiode / timing analysis.
#[derive(Resource, Default)]
pub struct RenderFrameCounterResource(pub u64);

/// Holds the per-render-frame data sampled in `Update` but not yet committed
/// to SHM. Committed in the *next* frame's `First` schedule, so the
/// `present_elapsed_secs` stamp captured at that point reflects the moment
/// this frame's photons actually appeared on screen (vsync of the prior
/// `present()` call, back-pressured by `PresentMode::Fifo`).
#[derive(Resource, Default)]
pub struct StagedRenderSample {
    pub pending: Option<StagedFrame>,
}

#[derive(Clone, Copy)]
pub struct StagedFrame {
    pub render_frame_number: u64,
    pub render_elapsed_secs_bits: u32,
    pub photodiode_white: bool,
}

// Update the shared memory game state after every game loop update
pub struct StateEmitterPlugin;

impl Plugin for StateEmitterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameCounterResource>()
           .init_resource::<RenderFrameCounterResource>()
           .init_resource::<StagedRenderSample>();
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

/// Sample render-frame data in `Update` and stash it for commit at the next
/// `First` tick. We do not write to SHM here: the matching
/// `present_elapsed_secs` (real wall-clock time of the actual flip) is only
/// available after `present()` returns and the next frame begins.
pub fn stage_render_sample(
    mut counter: ResMut<RenderFrameCounterResource>,
    mut staged: ResMut<StagedRenderSample>,
    round_start: Res<RoundStartTimestamp>,
    photodiode_query: Query<(&Visibility, &BackgroundColor), With<crate::utils::objects::PhotodiodeMarker>>,
) {
    counter.0 += 1;
    let render_secs_bits = round_start.0
        .map(|t| t.as_secs_f32().to_bits())
        .unwrap_or(0.0_f32.to_bits());
    let is_white = photodiode_query.iter().any(|(vis, bg)| {
        *vis != Visibility::Hidden && bg.0 == Color::WHITE
    });
    staged.pending = Some(StagedFrame {
        render_frame_number: counter.0,
        render_elapsed_secs_bits: render_secs_bits,
        photodiode_white: is_white,
    });
}

/// Commit the previously staged render-frame sample to SHM, stamping it with
/// the wall-clock time of *now* (Bevy's `First` schedule). Under
/// `PresentMode::Fifo` the main loop is back-pressured by `present()`, so this
/// stamp closely tracks the vsync at which the previous frame's pixels were
/// latched by the compositor — i.e. the photon-onset time for that frame.
///
/// Constant compositor + scanout offset remains; the photodiode absorbs it.
pub fn commit_render_sample(
    mut staged: ResMut<StagedRenderSample>,
    shm_res: Option<Res<SharedMemResource>>,
    time_real: Res<Time<bevy::time::Real>>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let Some(frame) = staged.pending.take() else { return };
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();
    let gs = &shm.game_structure_game;

    // Wall-clock seconds since app start. Monotonic; the controller computes
    // per-frame deltas against the previous row.
    let present_secs = time_real.elapsed_secs();

    gs.render_frame_number.store(frame.render_frame_number, Relaxed);
    gs.render_elapsed_secs.store(frame.render_elapsed_secs_bits, Relaxed);
    gs.present_elapsed_secs.store(present_secs.to_bits(), Relaxed);
    gs.photodiode_white.store(frame.photodiode_white, Relaxed);
}

/// Write shared memory from the local game state to shared memory to be read by controller
pub fn write_shared_memory_game_state(
    shm_res: Option<Res<SharedMemResource>>,
    mut game_state_local: ResMut<GameStateLocal>,
) {

    use std::sync::atomic::Ordering::Relaxed;

    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();
    let gs_game = &shm.game_structure_game;

    // Preserve the render-sample fields previously committed by
    // `commit_render_sample` (runs in `First`). Without this, the
    // subsequent whole-state write would clobber the fields (they can
    // be zero in the local `GameStateLocal`), which explains why
    // `render_frame_number` and `present_elapsed_secs` in logs were
    // observed as 0.
    let cur_rfn = gs_game.render_frame_number.load(Relaxed);
    let cur_rend_elapsed = gs_game.render_elapsed_secs.load(Relaxed);
    let cur_present = gs_game.present_elapsed_secs.load(Relaxed);
    let cur_photo = gs_game.photodiode_white.load(Relaxed);

    game_state_local.0.render_frame_number = cur_rfn;
    game_state_local.0.render_elapsed_secs = cur_rend_elapsed;
    game_state_local.0.present_elapsed_secs = cur_present;
    game_state_local.0.photodiode_white = cur_photo;

    // Update based on current values (now preserving staged fields)
    gs_game.write_from_local(&game_state_local.0);

    // Also push into the ring buffer so the controller can catch skipped frames
    shm.frame_ring_buffer.push(&game_state_local.0);
}
