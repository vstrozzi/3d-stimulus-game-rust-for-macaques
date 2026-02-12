//! This module collects game state and writes it to atomic shared memory

use bevy::prelude::*;
use crate::command_handler::{SharedMemResource, RenderingPaused};
use crate::utils::objects::{BaseDoor, RoundStartTimestamp};

use core::sync::atomic::Ordering;

// Count frames since beginning of game
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

fn increment_frame_counter(
    mut counter: ResMut<FrameCounterResource>,
    paused: Option<Res<RenderingPaused>>,
) {
    if let Some(paused) = paused {
        if paused.0 {
            return;
        }
    }
    counter.0 += 1;
}

// Write state of the game to shared memory to be read by controller
fn emit_state_to_shm(
    time: Res<Time>,
    frame_counter: Res<FrameCounterResource>,
    round_start: Res<RoundStartTimestamp>,
    camera_query: Query<&Transform, With<Camera3d>>,
    door_query: Query<(&BaseDoor, &Transform)>,
    shm_res: Option<Res<SharedMemResource>>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();
    let gs_game = &shm.game_structure_game;

    // Time & Frame
    gs_game.frame_number.store(frame_counter.0, Ordering::Relaxed);

    // Elapsed time
    let elapsed = if let Some(start) = round_start.0 {
        (time.elapsed() - start).as_secs_f32()
    } else {
        0.0
    };
    gs_game.elapsed_secs.store(elapsed.to_bits(), Ordering::Relaxed);

    // Camera
    if let Ok(camera_transform) = camera_query.single() {
        let pos = camera_transform.translation;
        let radius = pos.xz().length();
        gs_game.camera_radius.store(radius.to_bits(), Ordering::Relaxed);
        gs_game.camera_x.store(pos.x.to_bits(), Ordering::Relaxed);
        gs_game.camera_y.store(pos.y.to_bits(), Ordering::Relaxed);
        gs_game.camera_z.store(pos.z.to_bits(), Ordering::Relaxed);
    }

    // Continuous Alignment Calculation
    let current_alignment; 
    let current_angle;
    
    let target_door_idx = gs_game.target_door.load(Ordering::Relaxed) as usize;

    if let Ok(camera_transform) = camera_query.single() {
        let camera_forward = camera_transform.forward();
        let camera_forward_xz = Vec3::new(camera_forward.x, 0.0, camera_forward.z).normalize_or_zero();

        // Find target door
        for (door, door_transform) in &door_query {
            if door.door_index == target_door_idx {
                let door_normal_world = door_transform.rotation * door.normal;
                let door_normal_xz = Vec3::new(door_normal_world.x, 0.0, door_normal_world.z).normalize_or_zero();
                
                let alignment = door_normal_xz.dot(camera_forward_xz);
                current_alignment = alignment;
                // Angle in radians (0 to PI) using acos, clamping to safe range
                current_angle = alignment.clamp(-1.0, 1.0).acos();

                gs_game.current_alignment.store(current_alignment.to_bits(), Ordering::Relaxed);
                gs_game.current_angle.store(current_angle.to_bits(), Ordering::Relaxed);
                break;
            }
        }
    }

    // Update sequence number to indicate new data is available
    shm.game_structure_game_seq.fetch_add(1, Ordering::Relaxed);

}
