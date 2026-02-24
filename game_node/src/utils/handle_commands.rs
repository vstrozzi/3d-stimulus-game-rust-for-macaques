//! Functions to handle commands received by the Controlelr

use bevy::prelude::*;
use crate::shared_memory::shared_memory_reader::{PendingCommands, SharedMemResource};
use crate::shared_memory::shared_memory_writer::FrameCounterResource;
use crate::utils::objects::{DoorWinEntities, RotableComponent, PersistentCamera, RoundStartTimestamp, UIEntity, BlankScreen, GameEntity, GameStateLocal};
use crate::utils::ui::{spawn_score_bar};
use crate::utils::utils::{spawn_blank_screen, despawn_all_game_and_ui};
use crate::utils::setup::{setup_round};

// TODO: add these variables to shared
use shared::constants::camera_3d_constants::{
    CAMERA_3D_INITIAL_Y, CAMERA_3D_MAX_RADIUS, CAMERA_3D_MIN_RADIUS,
};

/// Reset state
pub fn handle_reset_command(
    pending: ResMut<PendingCommands>,
    mut commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut frame_counter: ResMut<FrameCounterResource>,
    camera_query: Query<&mut Transform, With<PersistentCamera>>,
    game_entities: Query<Entity, With<GameEntity>>,
    ambient_light: Option<ResMut<GlobalAmbientLight>>,
    ui_entities: Query<Entity, With<UIEntity>>,
    shm: Option<Res<SharedMemResource>>,
    mut local_game_struct: ResMut<GameStateLocal>,
    spotlight_query: Query<&mut SpotLight, (Without<crate::utils::objects::HoleLight>, Without<GameEntity>)>,
    round_start: ResMut<RoundStartTimestamp>,
    mut door_win_entities: ResMut<DoorWinEntities>,
) {    
    if !pending.reset {
        return;
    }

    // Reset commands received
    frame_counter.0 = 0;

    // Clear animation state to avoid stale entity references after despawn
    door_win_entities.animation_start_time = None;
    door_win_entities.winning_light = None;
    door_win_entities.winning_emissive = None;

    // Clear is_animating flag in SHM
    local_game_struct.0.is_animating = false;

    despawn_all_game_and_ui(commands.reborrow(), game_entities, ui_entities);

    // Reset shared memory game structure to default values for new round
    setup_round(
    commands.reborrow(),
    meshes,
    materials,
    camera_query,
    spotlight_query,
    ambient_light,
    shm,
    round_start,
    time,
    local_game_struct,
    door_win_entities,
    );

    spawn_score_bar(&mut commands);

}


/// System to handle animation door command
pub fn handle_animation_door_command(
    pending: ResMut<PendingCommands>,
    mut door_win_entities: ResMut<DoorWinEntities>,
    mut local_game_struct: ResMut<GameStateLocal>,
    time: Res<Time>,
) {
    if !pending.animation_door {
        return;
    }

    // Start animation
    door_win_entities.animation_start_time = Some(time.elapsed());
    
    // Set animation flag
    
    local_game_struct.0.is_animating = true;
}

/// System to apply blank screen command - spawns/despawns a black fullscreen overlay
pub fn handle_blank_screen(
    mut commands: Commands,
    pending: Res<PendingCommands>,
    overlay_query: Query<Entity, With<BlankScreen>>,
) {
    if pending.blank_screen {
        // Modulo through blank screen state
        if overlay_query.is_empty() {
            // Spawn black fullscreen overlay
            spawn_blank_screen(&mut commands);
            info!("Blank screen activated");
        } else {
            // Despawn the overlay
            for entity in overlay_query.iter() {
                commands.entity(entity).despawn();
            }
            info!("Blank screen deactivated");
        }
    }
}

/// Apply rotation.
pub fn handle_rotation(
    pending: Res<PendingCommands>,
    mut rot_entities: Query<&mut Transform, (With<RotableComponent>, Without<Camera3d>)>,
) {
    for mut rot_entity_transform in rot_entities.iter_mut() {
        let (mut yaw, _, _) = rot_entity_transform.rotation.to_euler(EulerRot::YXZ);
        yaw += pending.rotation;
        rot_entity_transform.rotation = Quat::from_rotation_y(yaw);
    }
}

/// Apply zoom.
pub fn handle_zoom(
    pending: Res<PendingCommands>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };
    let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
    let mut radius = transform.translation.xz().length();

    radius += pending.zoom;
    radius = radius.clamp(CAMERA_3D_MIN_RADIUS, CAMERA_3D_MAX_RADIUS);

    transform.translation = Vec3::new(radius * yaw.sin(), CAMERA_3D_INITIAL_Y, radius * yaw.cos());
    transform.look_at(Vec3::ZERO, Vec3::Y);
}