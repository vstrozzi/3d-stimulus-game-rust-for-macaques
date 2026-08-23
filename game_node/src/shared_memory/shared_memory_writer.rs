//! Collect game state and pair each snapshot with the matching wgpu present marker.

use std::{
    collections::VecDeque,
    sync::{mpsc, Mutex},
};

use bevy::{
    platform::time::Instant,
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        renderer::render_system,
        Render, RenderApp, RenderSystems,
    },
};

use crate::shared_memory::shared_memory_reader::{PendingCommands, SharedMemResource};
use crate::utils::objects::{
    BaseDoor, BlankScreen, GameConditions, GameStateLocal, PhotodiodeMarker,
    RoundStartTimestamp,
};

// Main-loop tick counter. It is monotonic for the lifetime of the process,
// including while visual rendering is stopped; controllers rebase it per trial.
#[derive(Resource, Default)]
pub struct FrameCounterResource(pub u64);

// ID assigned to the final main-world state extracted for a render submission.
#[derive(Resource, Default)]
pub struct RenderFrameCounterResource(pub u64);

/// The render ID extracted from the main world alongside the frame it names.
#[derive(Resource, Clone, Copy, Default, ExtractResource)]
pub struct CurrentRenderFrameId(pub u64);

/// Final state snapshots waiting for their matching render-world marker.
#[derive(Resource, Default)]
pub struct StagedRenderSamples {
    pub pending: VecDeque<StagedFrame>,
}

#[derive(Clone, Copy)]
pub struct StagedFrame {
    pub render_frame_number: u64,
    pub state: shared::SharedGameStateLocal,
}

/// A reset starts a new trial. Discard older snapshots so a delayed render
/// completion cannot be written into the new trial's ring sequence.
pub fn discard_staged_samples_on_reset(
    pending_commands: Res<PendingCommands>,
    mut staged: ResMut<StagedRenderSamples>,
) {
    if pending_commands.reset {
        staged.pending.clear();
    }
}

#[derive(Clone, Copy)]
struct PresentedFrame {
    render_frame_number: u64,
    marked_at: Instant,
}

#[derive(Resource)]
struct PresentationSender(mpsc::Sender<PresentedFrame>);

#[derive(Resource)]
pub(crate) struct PresentationReceiver(Mutex<mpsc::Receiver<PresentedFrame>>);

/// Installs the same state/presentation pairing path on native and WASM.
pub struct StateEmitterPlugin;

impl Plugin for StateEmitterPlugin {
    fn build(&self, app: &mut App) {
        let (sender, receiver) = mpsc::channel();

        app.init_resource::<FrameCounterResource>()
            .init_resource::<RenderFrameCounterResource>()
            .init_resource::<CurrentRenderFrameId>()
            .init_resource::<StagedRenderSamples>()
            .insert_resource(PresentationReceiver(Mutex::new(receiver)))
            .add_plugins(ExtractResourcePlugin::<CurrentRenderFrameId>::default());

        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(PresentationSender(sender))
                .add_systems(
                    Render,
                    mark_frame_presented
                        .after(render_system)
                        .in_set(RenderSystems::Render),
                );
        } else {
            warn!("RenderApp unavailable; presentation timing will not be recorded");
        }
    }
}

/// Increment the global timing variables
pub fn increment_timing(
    mut counter: ResMut<FrameCounterResource>,
    time: Res<Time>,
    mut round_start: ResMut<RoundStartTimestamp>,
    game_conditions: Res<GameConditions>,
) {
    // This is a main-loop counter, not a proof that the display refreshed.
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

/// Capture the finished main-world state in `Last`. The ID resource is then
/// extracted with the render submission. Sampling here also guarantees that
/// the photodiode value reflects the completed `Update` schedule.
pub fn stage_render_sample(
    mut counter: ResMut<RenderFrameCounterResource>,
    mut current_render_id: ResMut<CurrentRenderFrameId>,
    mut staged: ResMut<StagedRenderSamples>,
    game_state_local: Res<GameStateLocal>,
    round_start: Res<RoundStartTimestamp>,
    photodiode_query: Query<(&Visibility, &BackgroundColor), With<PhotodiodeMarker>>,
) {
    counter.0 += 1;
    current_render_id.0 = counter.0;

    let mut state = game_state_local.0;
    state.render_frame_number = counter.0;
    state.render_elapsed_secs = round_start.0
        .map(|t| t.as_secs_f32().to_bits())
        .unwrap_or(0.0_f32.to_bits());
    state.photodiode_white = photodiode_query.iter().any(|(vis, bg)| {
        *vis != Visibility::Hidden && bg.0 == Color::WHITE
    });
    // Filled only when the render world reports the matching submission.
    state.present_elapsed_secs = 0.0_f32.to_bits();

    staged.pending.push_back(StagedFrame {
        render_frame_number: counter.0,
        state,
    });
    // Ordinarily native pipelining is only one frame deep and WASM is
    // single-threaded. Bound this defensively if a surface stops completing.
    const MAX_PENDING_SNAPSHOTS: usize = shared::RING_BUFFER_SIZE * 8;
    while staged.pending.len() > MAX_PENDING_SNAPSHOTS {
        staged.pending.pop_front();
    }
}

/// Runs in the render world directly after Bevy calls wgpu `present()`.
/// `present()` is the closest portable software marker available through
/// wgpu; the compositor/display may make photons visible later.
fn mark_frame_presented(
    render_id: Option<Res<CurrentRenderFrameId>>,
    sender: Res<PresentationSender>,
) {
    let Some(render_id) = render_id else { return };
    if render_id.0 == 0 {
        return;
    }
    let _ = sender.0.send(PresentedFrame {
        render_frame_number: render_id.0,
        marked_at: Instant::now(),
    });
}

/// Drain completed render submissions in `First` and commit each exact state
/// snapshot to the ring. No state is mixed with a timestamp from another ID.
pub(crate) fn commit_render_sample(
    mut staged: ResMut<StagedRenderSamples>,
    receiver: Res<PresentationReceiver>,
    shm_res: Option<Res<SharedMemResource>>,
    time_real: Res<Time<bevy::time::Real>>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();
    let gs = &shm.game_structure_game;
    let Ok(receiver) = receiver.0.lock() else { return };

    while let Ok(presented) = receiver.try_recv() {
        while staged
            .pending
            .front()
            .is_some_and(|frame| frame.render_frame_number < presented.render_frame_number)
        {
            staged.pending.pop_front();
        }
        let Some(mut frame) = staged.pending.pop_front() else { continue };
        if frame.render_frame_number != presented.render_frame_number {
            // A completion from before a trial reset has no matching snapshot.
            staged.pending.push_front(frame);
            continue;
        }

        let present_secs = presented
            .marked_at
            .saturating_duration_since(time_real.startup())
            .as_secs_f32();
        frame.state.present_elapsed_secs = present_secs.to_bits();

        // Keep the live atomics useful without briefly replacing current game
        // state with an older pipelined snapshot.
        gs.render_frame_number
            .store(frame.state.render_frame_number, Relaxed);
        gs.render_elapsed_secs
            .store(frame.state.render_elapsed_secs, Relaxed);
        gs.present_elapsed_secs
            .store(frame.state.present_elapsed_secs, Relaxed);
        gs.photodiode_white
            .store(frame.state.photodiode_white, Relaxed);

        shm.frame_ring_buffer.push(&frame.state);
    }
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

}
