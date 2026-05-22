//! Functions to handle commands received by the Controller
use bevy::prelude::*;
use crate::shared_memory::shared_memory_reader::{PendingCommands, SharedMemResource};
use crate::utils::objects::{
    DoorWinEntities,BaseDoor,  RotableComponent, RoundStartTimestamp, PersistentCamera, GameConditions,
    UIEntity, BlankScreen, GameEntity, GameStateLocal};
use crate::utils::helpers::{spawn_blank_screen, despawn_all_game_and_ui};
use crate::utils::setup::{setup_round};
use shared::constants::camera_3d_constants::{CAMERA_3D_MAX_RADIUS, CAMERA_3D_MIN_RADIUS,
};

// Door/light palette. sRGB hex spelled out as (byte / 255) so the source
// values remain obvious. Bevy's `Color::srgba` is sRGB-encoded.
//   #8B0000 dark red               (139, 0,   0)
//   #CCFF00 yellow-green/chartreuse (204, 255, 0)
pub const LIGHT_RED:   Color = Color::srgba(0x8B as f32 / 255.0, 0x00 as f32 / 255.0, 0x00 as f32 / 255.0, 1.0);
pub const LIGHT_GREEN: Color = Color::srgba(0xCC as f32 / 255.0, 0xFF as f32 / 255.0, 0x00 as f32 / 255.0, 1.0);

/// Reset state
pub fn handle_reset_command(
    pending: ResMut<PendingCommands>,
    mut commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    preloaded: Res<crate::utils::objects::PreloadedTextures>,
    camera_query: Query<&mut Transform, With<PersistentCamera>>,
    game_entities: Query<Entity, With<GameEntity>>,
    ambient_light: Option<ResMut<GlobalAmbientLight>>,
    ui_entities: Query<Entity, With<UIEntity>>,
    shm: Option<Res<SharedMemResource>>,
    mut local_game_struct: ResMut<GameStateLocal>,
    spotlight_query: Query<&mut SpotLight, (Without<crate::utils::objects::HoleLight>, Without<GameEntity>)>,
    round_start: ResMut<RoundStartTimestamp>,
    mut door_win_entities: ResMut<DoorWinEntities>,
    time: Res<Time>,
) {

    if !pending.reset {
        return;
    }

    // Reset commands received
    local_game_struct.0.render_frame_number = 0;
    local_game_struct.0.frame_number = 0;

    // Clear animation state to avoid stale entity references after despawn
    door_win_entities.winning_light = None;
    door_win_entities.winning_emissive = None;

    // Clear is_animating flag in SHM
    local_game_struct.0.is_animating = false;

    despawn_all_game_and_ui(commands.reborrow(), game_entities, ui_entities);

    // Reset shared memory game structure to default values for new round.
    // The score bar is a persistent entity pool (see `spawn_score_bar_pool`)
    // and is intentionally not despawned here; `update_score_bar` reflects
    // the new `progress_bar_size` automatically.
    setup_round(
    commands.reborrow(),
    meshes,
    materials,
    preloaded,
    camera_query,
    spotlight_query,
    ambient_light,
    shm,
    round_start,
    local_game_struct,
    door_win_entities,
    time,
    );

}


/// Handle animation door command
pub fn handle_animation_door_command(
    pending: ResMut<PendingCommands>,
    mut door_win_entities: ResMut<DoorWinEntities>,
    mut local_game_struct: ResMut<GameStateLocal>,
    time: Res<Time>,
) {
    // React only on animation door or when not animating
    if !pending.animation_door{
        return;
    }

    // Start animation
    door_win_entities.animation_start_time = Some(time.elapsed());
    door_win_entities.animate_all = pending.animation_all_door;

    // Request of color
    door_win_entities.color =
    if pending.animation_colored{
        // Single door with requested animation of color, then green
        if !pending.animation_all_door {
            LIGHT_GREEN
        }
        else {
            // All door with requested animation of color, then red
            LIGHT_RED
        }
    }
    // Single animation not colored
    else if !pending.animation_colored && !pending.animation_all_door {
        // No animation colored, then red TODO: change name
        LIGHT_RED
    }
    else {
        // No animation requested → invisible (alpha=0) but keep the green hue
        // so any leftover fade-out interpolates from the right palette.
        LIGHT_GREEN.with_alpha(0.0)
    };

    // Set animation flag
    local_game_struct.0.is_animating = true;
    
}

/// Applies pending check alignment and change colors of doors
pub fn handle_check_alignment(
    pending: Res<PendingCommands>,
    mut local_game_struct: ResMut<GameStateLocal>,
    camera_query: Query<&Transform, With<Camera3d>>,
    door_query: Query<(Entity, &BaseDoor, &Transform)>,
    time: Res<Time>,
    mut door_win_entities: ResMut<DoorWinEntities>,
) {
    // Only proceed check alignment was requested or when not animating
    if !pending.check_alignment{
        return;
    }

    let gs_game= &mut local_game_struct.0;

    // Increment attempt counter
    gs_game.attempts += 1;

    let Ok(camera_transform) = camera_query.single() else {
        return;
    };

    // Get local camera direction
    let camera_forward = camera_transform.forward();

    // Project camera forward to XZ plane
    let camera_forward_xz = Vec3::new(camera_forward.x, 0.0, camera_forward.z).normalize();

    let mut best_alignment = -1.0;
    let mut _best_door_index = 0;
    let mut winning_door_alignment = -1.0;

    // Determine target door from SHM
    let target_door_idx = gs_game.target_door;

    for (_, door, door_transform) in &door_query {
        // Get door normal in world space
        let door_normal_world = door_transform.rotation * door.normal;

        // Project to XZ plane
        let door_normal_xz = Vec3::new(door_normal_world.x, 0.0, door_normal_world.z).normalize();

        // Calculate alignment (dot product)
        let alignment = door_normal_xz.dot(camera_forward_xz);

        // Most positive = door facing toward camera (from outside)
        if alignment > best_alignment {
            best_alignment = alignment;
            _best_door_index = door.door_index;
        }

        // Save the alignment for the target door
        if door.door_index as u32 == target_door_idx {
            winning_door_alignment = alignment;
        }
    }

    // Store alignment for score bar animation and shm
    gs_game.current_alignment = winning_door_alignment.to_bits();

    // Player wins
    if winning_door_alignment > f32::from_bits(gs_game.cosine_alignment_threshold) {
        // Player wins! Set win time in SHM to trigger win state
        gs_game.win_time = time.elapsed().as_secs_f32().to_bits();
        door_win_entities.animate_all = false; // Animate only the winning door
    }
}


/// Apply blank screen command
pub fn handle_blank_screen(
    mut commands: Commands,
    pending: Res<PendingCommands>,
    overlay_query: Query<Entity, With<BlankScreen>>,

) {
    if !pending.toggle_blank {
        return;
    }

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
    }
}

/// Apply rotation
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

/// Apply zoom
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

    transform.translation = Vec3::new(radius * yaw.sin(), transform.translation.y, radius * yaw.cos());
    transform.look_at(Vec3::ZERO, Vec3::Y);
}


/// Apply stop rendering
pub fn handle_stop_rendering(
    pending: Res<PendingCommands>,
    mut game_conditions: ResMut<GameConditions>,
) {
    if !pending.toggle_stop_rendering {
        return;
    }
    // Modulo through blank screen state
    game_conditions.stop_rendering = !game_conditions.stop_rendering;
    
}