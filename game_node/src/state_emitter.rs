//! This module collects game state and writes it to atomic shared memory.

use bevy::prelude::*;
use crate::command_handler::SharedMemResource;
use crate::utils::objects::{GamePhase as InternalGamePhase, GameState as InternalGameState, RotableComponent};

use core::sync::atomic::Ordering;

#[derive(Resource, Default)]
pub struct FrameCounterResource(pub u64);

// Update the shared memory game state after every game loop update.
pub struct StateEmitterPlugin;

impl Plugin for StateEmitterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameCounterResource>()
           .add_systems(PostUpdate, (increment_frame_counter, emit_state_to_shm).chain());
    }
}

fn increment_frame_counter(mut counter: ResMut<FrameCounterResource>) {
    counter.0 += 1;
}

// Emit the current game state to shared memory read by the controller.
fn emit_state_to_shm(
    time: Res<Time>,
    frame_counter: Res<FrameCounterResource>,
    internal_state: Res<InternalGameState>,
    current_phase: Res<State<InternalGamePhase>>,
    camera_query: Query<&Transform, With<Camera3d>>,
    rotable_query: Query<&Transform, With<RotableComponent>>,
    shm_res: Option<Res<SharedMemResource>>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();
    let gs = &shm.game_structure;

    // Phase: 0=Playing, 1=Won (Resetting is transient, treat as Playing)
    let phase_code = match *current_phase.get() {
        InternalGamePhase::Playing | InternalGamePhase::Resetting => 0,
        InternalGamePhase::Won => 1,
    };
    gs.phase.store(phase_code, Ordering::Relaxed);

    // Time & Frame
    gs.frame_number.store(frame_counter.0, Ordering::Relaxed);

    let elapsed = internal_state.start_time
        .map(|start| (time.elapsed() - start).as_secs_f32())
        .unwrap_or(0.0);
    gs.elapsed_secs.store(elapsed.to_bits(), Ordering::Relaxed);

    // Camera
    if let Ok(camera_transform) = camera_query.single() {
        let pos = camera_transform.translation;
        let radius = pos.xz().length();
        gs.camera_radius.store(radius.to_bits(), Ordering::Relaxed);
        gs.camera_x.store(pos.x.to_bits(), Ordering::Relaxed);
        gs.camera_y.store(pos.y.to_bits(), Ordering::Relaxed);
        gs.camera_z.store(pos.z.to_bits(), Ordering::Relaxed);
    }

    // Pyramid
    let yaw = rotable_query.iter().next()
        .map(|t| t.rotation.to_euler(EulerRot::YXZ).0)
        .unwrap_or(0.0);
    gs.pyramid_yaw.store(yaw.to_bits(), Ordering::Relaxed);

    // Logic
    gs.attempts.store(internal_state.nr_attempts, Ordering::Relaxed);

    if let Some(align) = internal_state.cosine_alignment {
        gs.alignment.store(align.to_bits(), Ordering::Relaxed);
    } else {
        gs.alignment.store((2.0f32).to_bits(), Ordering::Relaxed); // Sentinel for None
    }

    gs.is_animating.store(internal_state.is_animating, Ordering::Relaxed);
    gs.has_won.store(phase_code == 1, Ordering::Relaxed);

    // Win time
    if let (Some(start), Some(end)) = (internal_state.start_time, internal_state.end_time) {
        let dur = (end - start).as_secs_f32();
        gs.win_time.store(dur.to_bits(), Ordering::Relaxed);
    } else {
        gs.win_time.store(0, Ordering::Relaxed);
    }
}
