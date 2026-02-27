//! Core game and UI functions.
use bevy::prelude::*;

use crate::utils::objects::{DoorWinEntities, HoleEmissive, HoleLight, ScoreBarFill, GameStateLocal};

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
        return;
    };

    let elapsed = (time.elapsed() - start_time).as_secs_f32();

    // Config values from SHM
    let fade_out_end = f32::from_bits(gs_game.door_anim_fade_out);
    let stay_open_end = fade_out_end + f32::from_bits(gs_game.door_anim_stay_open);
    let fade_in_end = stay_open_end + f32::from_bits(gs_game.door_anim_fade_in);

    // Calculate animation intensity (0.0 to 1.0)
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

    let is_active = intensity_factor > 0.0;
    let target_visibility = if is_active { Visibility::Visible } else { Visibility::Hidden };

    // Determine target intensity
    let target_intensity = if is_active {
        let max_spotlight_intensity = f32::from_bits(gs_game.max_spotlight_intensity);
        let base_intensity = max_spotlight_intensity * intensity_factor;
        
        if door_win_entities.animate_all {
            base_intensity / 10.0
        } else {
            base_intensity
        }
    } else {
        0.0
    };

    // Helper closures to prevent repeating the mutation logic
    let update_light = |visibility: &mut Visibility, spotlight: &mut SpotLight, color: Color| {
        *visibility = target_visibility;
        spotlight.intensity = target_intensity;
        spotlight.color = color;
    };

    let update_emissive = |
        visibility: &mut Visibility,
        material_handle: &MeshMaterial3d<StandardMaterial>,
        materials: &mut Assets<StandardMaterial>,
        color: Color
    | {
        *visibility = target_visibility;
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.emissive = color.to_linear();
        }
    };

    // Color it in RED
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

    // Unify state cleanup at the end
    if !is_active {
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
    // Don't animate bar if animate all door
    if door_win_entities.animate_all {
        return;
    }
    let gs_game = &mut local_game_struct.0;

    let Ok((mut node, mut bg_color)) = fill_query.single_mut() else {
        return;
    };
    // Get alignment score (normalized to 0.0 - 1.0 range from -1.0 - 1.0)
    let alignment = f32::from_bits(gs_game.current_alignment);
    let alignment_normalized = ((alignment + 1.0) / 2.0).clamp(0.0, 1.0);

    if gs_game.is_animating {
        let current_width = {
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
        };
        node.width = Val::Percent(current_width);
    }  else {
        // Not animating: bar stays empty
        *bg_color = BackgroundColor(Color::srgba(0.2, 0.6, 1.0, 0.3)); // Dim cyan glow when empty
        node.width = Val::Percent(0.0);
        return;
    };


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

