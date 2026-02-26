//! This module collects game state and writes it to atomic shared memory

use bevy::{prelude::*};
use crate::shared_memory::shared_memory_reader::{SharedMemResource};
use crate::utils::objects::{BaseDoor, RoundStartTimestamp, GameStateLocal, GameConditions};

// Count frames since beginning of game
#[derive(Resource, Default)]
pub struct FrameCounterResource(pub u64);

// Update the shared memory game state after every game loop update.
pub struct StateEmitterPlugin;

impl Plugin for StateEmitterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameCounterResource>();
    }
}

pub fn increment_timing(
    mut counter: ResMut<FrameCounterResource>,
    time: Res<Time>,
    mut round_start: ResMut<RoundStartTimestamp>,
    game_conditions: Res<GameConditions>,
) {
    // Increment the frames regardless
    counter.0 += 1;

    println!("Frame: {}, Elapsed: {:?}, Stop Rendering: {}", counter.0, round_start.0, game_conditions.stop_rendering);
    if game_conditions.stop_rendering {
        return;
    }


    // Add the delta 
    if let Some(ref mut total) = round_start.0 {
        *total += time.delta();
    }
}

// Update local memory
pub fn update_shared_memory_local(
    mut game_state_local: ResMut<GameStateLocal>,
    frame_counter: Res<FrameCounterResource>,
    round_start: Res<RoundStartTimestamp>,
    camera_query: Query<&Transform, With<Camera3d>>,
    door_query: Query<(&BaseDoor, &Transform)>,
) {
    game_state_local.0.frame_number = frame_counter.0;
    game_state_local.0.elapsed_secs = if let Some(start) = round_start.0 {
        start.as_secs_f32().to_bits()
    } else {
        0.0_f32.to_bits()
    };
    if let Ok(camera_transform) = camera_query.single() {
        let pos = camera_transform.translation;
        game_state_local.0.camera_radius = pos.xz().length().to_bits();
        game_state_local.0.camera_x = pos.x.to_bits();
        game_state_local.0.camera_y = pos.y.to_bits();
        game_state_local.0.camera_z = pos.z.to_bits();
    }

    let target_door_idx = game_state_local.0.target_door as usize;

    let current_alignment; 
    let current_angle;  

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
                current_angle = alignment.clamp(-1.0, 1.0).acos();

                game_state_local.0.current_alignment = current_alignment.to_bits();
                game_state_local.0.current_angle = current_angle.to_bits();
                break;
            }
        }
    }
}

// Write shared memory from the local game state to shared memory to be read by controller
pub fn write_shared_memory_game_state(
    shm_res: Option<Res<SharedMemResource>>,
    game_state_local: Res<GameStateLocal>,
) {

    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();
    let gs_game = &shm.game_structure_game;

    // Update based on current values
    gs_game.from_not_atomic(&game_state_local.0);
}
