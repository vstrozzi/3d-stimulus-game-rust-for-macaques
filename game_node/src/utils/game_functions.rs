//! Core game and UI functions.
use bevy::prelude::*;

use crate::shared_memory::shared_memory_reader::PendingCommands;
use crate::utils::objects::{
    BaseDoor, DoorWinEntities, HoleEmissive, HoleLight, ScoreBarFill, UIEntity, GameStateLocal
};
use crate::utils::utils::{despawn_ui};
use crate::utils::ui::{spawn_score_bar};

/// Applies pending check alignment
pub fn apply_pending_check_alignment(
    pending: Res<PendingCommands>,
    mut local_game_struct: ResMut<GameStateLocal>,
    camera_query: Query<&Transform, With<Camera3d>>,
    door_query: Query<(Entity, &BaseDoor, &Transform)>,
    mut commands: Commands,
    time: Res<Time>,
    ui_query: Query<Entity, With<UIEntity>>,
    mut door_win_entities: ResMut<DoorWinEntities>,
) {
    // Only proceed check alignment was requested
    if !pending.check_alignment {
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
    }

    // Every alignment check triggers the door animation on the winning light/emissive
    gs_game.is_animating = true;
    door_win_entities.animation_start_time = Some(time.elapsed());

    // Clean old UI and spawn new (Score Bar)
    despawn_ui(&mut commands, &ui_query);
    spawn_score_bar(&mut commands);
}



/// Handles the light animation
pub fn handle_door_animation(
    mut door_win_entities: ResMut<DoorWinEntities>,
    mut local_game_struct: ResMut<GameStateLocal>,
    time: Res<Time>,
    mut light_query: Query<(&mut Visibility, &mut SpotLight), With<HoleLight>>,
    mut emissive_query: Query<
        (&mut Visibility, &MeshMaterial3d<StandardMaterial>),
        (With<HoleEmissive>, Without<HoleLight>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let gs_game = &mut local_game_struct.0;

    // Animation is started by handle_animation_door_command (sets is_animating + entities)
    if !gs_game.is_animating {
        return;
    }

    let Some(start_time) = door_win_entities.animation_start_time else {
        // No start time set — animation state is inconsistent, clear it
        gs_game.is_animating = false;
        return;
    };
    let elapsed = (time.elapsed() - start_time).as_secs_f32();

    // Config values from SHM
    let fade_out_end = f32::from_bits(gs_game.door_anim_fade_out);
    let stay_open_end =
        fade_out_end + f32::from_bits(gs_game.door_anim_stay_open);
    let fade_in_end =
        stay_open_end + f32::from_bits(gs_game.door_anim_fade_in);

    // Get light entity from door_win_entities (winning_light = SpotLight/HoleLight)
    let Some(light_entity) = door_win_entities.winning_light else {
        // Entity was despawned (e.g. by reset)
        door_win_entities.animation_start_time = None;
        gs_game.is_animating = false;
        return;
    };

    // Get light visibility and component
    let Ok((mut light_visibility, mut spotlight)) = light_query.get_mut(light_entity) else {
        // Entity no longer valid 
        door_win_entities.animation_start_time = None;
        gs_game.is_animating = false;
        return;
    };

    // Calculate animation intensity (0.0 to 1.0)
    let intensity_factor = if elapsed < fade_out_end {
        // Phase 1: Fade Out (Opening) - 0.0 to 1.0
        elapsed / fade_out_end
    } else if elapsed < stay_open_end {
        // Phase 2: Stay Open - 1.0
        1.0
    } else if elapsed < fade_in_end {
        // Phase 3: Fade In (Closing) - 1.0 to 0.0
        1.0 - ((elapsed - stay_open_end) / f32::from_bits(gs_game.door_anim_fade_in))
    } else {
        // Animation finished
        0.0
    };

    // Max intensity values 
    let max_spotlight_intensity = f32::from_bits(gs_game.max_spotlight_intensity);

    if intensity_factor > 0.0 {
        
        // Animation is in progress — update spotlight
        *light_visibility = Visibility::Visible;

        let light_intensity = max_spotlight_intensity * intensity_factor;
        spotlight.intensity = light_intensity;

        // Also update emissive material
        if let Some(emissive_entity) = door_win_entities.winning_emissive {
            if let Ok((mut emissive_visibility, material_handle)) =
                emissive_query.get_mut(emissive_entity)
            {
                *emissive_visibility = Visibility::Visible;

                if let Some(material) = materials.get_mut(&material_handle.0) {
                    let light_color = spotlight.color.to_linear();
                    material.emissive = LinearRgba::new(
                        light_color.red * light_intensity,
                        light_color.green * light_intensity,
                        light_color.blue * light_intensity,
                        1.0,
                    );
                }
            }
        }
    } else {
        // Animation finished — hide spotlight
        *light_visibility = Visibility::Hidden;
        spotlight.intensity = 0.0;

        // Hide emissive and clear state
        if let Some(emissive_entity) = door_win_entities.winning_emissive {
            if let Ok((mut emissive_visibility, material_handle)) =
                emissive_query.get_mut(emissive_entity)
            {
                *emissive_visibility = Visibility::Hidden;

                if let Some(material) = materials.get_mut(&material_handle.0) {
                    material.emissive = LinearRgba::new(0.0, 0.0, 0.0, 0.0);
                }
            }
        }

        // Clear animation timing state (winning entities persist for the round)
        door_win_entities.animation_start_time = None;
        gs_game.is_animating = false;
    }
}

/// Updates the score bar fill and color during the door animation
pub fn update_score_bar_animation(
    door_win_entities: Res<DoorWinEntities>,
    mut local_game_struct: ResMut<GameStateLocal>,
    time: Res<Time>,
    mut fill_query: Query<(&mut Node, &mut BackgroundColor), With<ScoreBarFill>>,
) {
    let Ok((mut node, mut bg_color)) = fill_query.single_mut() else {
        return;
    };
  
    let gs_game = &mut local_game_struct.0;

    // Get alignment score (normalized to 0.0 - 1.0 range from -1.0 - 1.0)
    let alignment = f32::from_bits(gs_game.current_alignment);
    let alignment_normalized = ((alignment + 1.0) / 2.0).clamp(0.0, 1.0);

    let is_animating = gs_game.is_animating;

    // Calculate the bar width — only show fill during animation, otherwise empty
    let current_width = if is_animating {
        // During animation: fill progressively based on animation progress
        let Some(start_time) = door_win_entities.animation_start_time else {
            return;
        };
        let elapsed = (time.elapsed() - start_time).as_secs_f32();

        let fade_out_end = f32::from_bits(gs_game.door_anim_fade_out);
        let stay_open_dur = f32::from_bits(gs_game.door_anim_stay_open);
        let fade_in_dur = f32::from_bits(gs_game.door_anim_fade_in);

        let total_duration = fade_out_end + stay_open_dur + fade_in_dur;
        let fill_progress = (elapsed / total_duration).clamp(0.0, 1.0);
        let target_width = alignment_normalized * 100.0;
        fill_progress * target_width
    } else {
        // Not animating: bar stays empty
        0.0
    };

    node.width = Val::Percent(current_width);

    // Color gradient based on alignment quality (cyan -> yellow -> white)
    let color = if alignment_normalized < 0.5 {
        let t = alignment_normalized * 2.0; // 0.0 to 1.0 for first half
        Color::srgba(
            0.2 + t * 0.8, // R: 0.2 -> 1.0
            0.6 + t * 0.4, // G: 0.6 -> 1.0
            1.0 - t * 0.2, // B: 1.0 -> 0.8
            0.7 + t * 0.2, // A: 0.7 -> 0.9
        )
    } else {
        let t = (alignment_normalized - 0.5) * 2.0; // 0.0 to 1.0 for second half
        Color::srgba(
            1.0,           // R: stays at 1.0
            1.0,           // G: stays at 1.0
            0.8 + t * 0.2, // B: 0.8 -> 1.0 (yellow to white)
            0.9 + t * 0.1, // A: 0.9 -> 1.0
        )
    };

    *bg_color = BackgroundColor(color);
}

