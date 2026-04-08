//! Core game and UI functions.
use bevy::prelude::*;

use crate::utils::objects::{DoorWinEntities, HoleEmissive, HoleLight, ScoreBarDot, ScoreBarChain,  GameStateLocal};
use shared::constants::game_constants::{PROGRESS_BAR_WRAP_AROUND_SIZE};

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
    if !gs_game.is_animating {
        return;
    }

    let Some(start_time) = door_win_entities.animation_start_time else {
        // animation_start_time is None but is_animating is true: stale flag from shared
        // memory overwrite (game_structure_control had a previous frame's value).
        // Clear it so the controller can exit WAITING_ANIMATION_END and resume.
        return;
    };

    let elapsed = (time.elapsed() - start_time).as_secs_f32();

    // Config values from SHM
    let fade_out_end = f32::from_bits(gs_game.door_anim_fade_out);
    let stay_open_end = fade_out_end + f32::from_bits(gs_game.door_anim_stay_open);
    let fade_in_end = stay_open_end + f32::from_bits(gs_game.door_anim_fade_in);

    // Cleanup if animation is finished
    let is_finished = elapsed >= fade_in_end;
    if is_finished {
        // Ensure final state is fully reset
        for (mut vis, mut spotlight) in light_query.iter_mut() {
            *vis = Visibility::Hidden;
            spotlight.intensity = 0.0;
        }
        for (mut vis, mat_handle) in emissive_query.iter_mut() {
            *vis = Visibility::Hidden;
            if let Some(material) = materials.get_mut(&mat_handle.0) {
                material.emissive = Color::BLACK.to_linear();
            }
        }
        // Clean up state
        door_win_entities.animation_start_time = None;
        gs_game.is_animating = false;
        return;
    }

    // Not finished, calculate intensity factor (0.0 to 1.0) based on phase
    let intensity_factor = if elapsed < fade_out_end {
        // Phase 1: Fade Out (Opening) - 0.0 to 1.0
        elapsed / fade_out_end + 0.0001 // Add to avoid edge case
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

    // Determine target intensity
    let target_intensity = {
        let max_spotlight_intensity = f32::from_bits(gs_game.max_spotlight_intensity);
        let base_intensity = max_spotlight_intensity * intensity_factor;
        
        // If animating everything
        if door_win_entities.animate_all {  
            0.0
        } else {
            base_intensity
        }
    };

    // Update helpers
    let update_light = |visibility: &mut Visibility, spotlight: &mut SpotLight, color: Color| {
        *visibility = Visibility::Visible;
        spotlight.intensity = target_intensity;
        spotlight.color = color;
    };

    let update_emissive = |
        visibility: &mut Visibility,
        material_handle: &MeshMaterial3d<StandardMaterial>,
        materials: &mut Assets<StandardMaterial>,
        color: Color
    | {
        *visibility = Visibility::Visible;
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.emissive = color.to_linear();
        }
    };

    // Color all lamp
    if door_win_entities.animate_all {
        for (mut vis, mut spotlight) in light_query.iter_mut() {
            update_light(&mut vis, &mut spotlight, door_win_entities.color);
        }
        for (mut vis, mat_handle) in emissive_query.iter_mut() {
            update_emissive(&mut vis, mat_handle, &mut materials, door_win_entities.color);
        }
    } else {
        // Single entity branch
        if let Some(light_entity) = door_win_entities.winning_light {
            if let Ok((mut vis, mut spotlight)) = light_query.get_mut(light_entity) {
                update_light(&mut vis, &mut spotlight, door_win_entities.color);

                if let Some(emissive_entity) = door_win_entities.winning_emissive {
                    if let Ok((mut evis, mat_handle)) = emissive_query.get_mut(emissive_entity) {
                        // Inherit color from the spotlight in the single case
                        update_emissive(&mut evis, mat_handle, &mut materials, door_win_entities.color);
                    }
                }
            }
        }
    }

}

// Update the score bar based on the current level state
pub fn update_score_bar(
    mut local_game_struct: ResMut<GameStateLocal>,
    mut dot_query: Query<(&ScoreBarDot, &mut BackgroundColor), Without<ScoreBarChain>>,
    mut chain_query: Query<(&ScoreBarChain, &mut BackgroundColor), Without<ScoreBarDot>>,
) {

    let gs_game = &mut local_game_struct.0;
    let progress_bar_cur_size: u32 = gs_game.progress_bar_cur_size; //gs_game.progress_bar_cur_size;

    let filled_color = Color::srgba(1.0, 1.0, 1.0, 0.5);
    let unfilled_dot_color = Color::srgba(1.0, 1.0, 1.0, 0.01);
    let filled_chain_color = Color::srgba(1.0, 1.0, 0.0, 0.3);
    let unfilled_chain_color = Color::srgba(1.0, 1.0, 0.0, 0.05);

    // Update dots
    for (dot, mut bg) in dot_query.iter_mut() {
        if dot.index < progress_bar_cur_size {
            *bg = BackgroundColor(filled_color);
        } else {
            *bg = BackgroundColor(unfilled_dot_color);
        }
    }

    // Update chains
    for (chain, mut bg) in chain_query.iter_mut() {
        let dot_after = chain.index + 1;
        let same_row = chain.index / PROGRESS_BAR_WRAP_AROUND_SIZE
            == dot_after / PROGRESS_BAR_WRAP_AROUND_SIZE;

        if same_row && dot_after < progress_bar_cur_size {
            *bg = BackgroundColor(filled_chain_color);
        } else {
            *bg = BackgroundColor(unfilled_chain_color);
        }
    }
}