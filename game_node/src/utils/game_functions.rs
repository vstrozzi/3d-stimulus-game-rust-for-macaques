//! Core game and UI functions.
use bevy::prelude::*;
use bevy::audio::Volume;

use crate::utils::objects::{BaseDoor, DoorWinEntities, HoleEmissive, HoleLight, ScoreBarDot, ScoreBarChain, ScoreBarUI, GameStateLocal};
use crate::utils::load_assets::SoundSet;
use shared::constants::game_constants::{PROGRESS_BAR_WRAP_AROUND_SIZE};
use shared::constants::pyramid_constants::LIGHT_GREEN;
use shared::constants::lighting_constants::{FAINT_ALIGNED_INTENSITY_FACTOR,FAINT_ALIGNED_SPOTLIGHT_FACTOR, FAINT_ALIGNED_SPOTLIGHT_RANGE,HOLE_SPOTLIGHT_RANGE
};
use shared::constants::sound_constants::ENABLE_SOUND_EFFECTS;

/// Handles the light animation
pub fn handle_door_animation(
    mut commands: Commands,
    sounds: Res<SoundSet>,
    mut door_win_entities: ResMut<DoorWinEntities>,
    mut local_game_struct: ResMut<GameStateLocal>,
    time: Res<Time>,
    mut light_query: Query<(&mut Visibility, &mut SpotLight), With<HoleLight>>,
    mut emissive_query: Query<
        (&mut Visibility, &MeshMaterial3d<StandardMaterial>),
        (With<HoleEmissive>, Without<HoleLight>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sink_query: Query<&mut AudioSink>,
) {
    let gs_game = &mut local_game_struct.0;
    if !gs_game.is_animating {
        return;
    }

    let Some(start_time) = door_win_entities.animation_start_time else {
        // animation_start_time is None but is_animating is true: stale flag from shared
        door_win_entities.animation_start_time = None;
        return;
    };

    let elapsed = (time.elapsed() - start_time).as_secs_f32();

    // Config values from SHM
    let fade_out_end = f32::from_bits(gs_game.door_anim_fade_out);
    let stay_open_end = fade_out_end + f32::from_bits(gs_game.door_anim_stay_open);
    let fade_in_end = stay_open_end + f32::from_bits(gs_game.door_anim_fade_in);
    // Per-level one-shot effects volume from SHM.
    let sfx_volume = f32::from_bits(gs_game.sound_effects_volume);

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
        // Stop the sound effects so they last exactly the animation duration
        for e in door_win_entities.active_sounds.drain(..) {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.despawn();
            }
        }
        // Clean up state
        door_win_entities.animation_start_time = None;
        door_win_entities.phase_sound_played = false;
        gs_game.is_animating = false;
        return;
    }

    // Not finished, calculate intensity factor (0.0 to 1.0) based on phase
    let intensity_factor = if elapsed < fade_out_end {
        // Phase 1: Fade Out (Opening) - 0.0 to 1.0. Play the win/hint sound once on entry, 
        // depending on the light color (green => win, otherwise => hint).
        if !door_win_entities.phase_sound_played {
            door_win_entities.phase_sound_played = true;
            if ENABLE_SOUND_EFFECTS {
                let settings = PlaybackSettings::ONCE.with_volume(Volume::Linear(sfx_volume));
                // Green light => win sound, otherwise => hint sound.
                let clip = if door_win_entities.color == LIGHT_GREEN { sounds.win.clone() } else { sounds.hint.clone() };
                let e = commands.spawn((AudioPlayer::new(clip), settings)).id();
                door_win_entities.active_sounds.push(e);
            }
        }

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

    // Fade the sounds out over the last 0.5s of the animation, scaled by the
    // configured effects volume.
    for eq in door_win_entities.active_sounds.iter() {
        if let Ok(mut sink) = sink_query.get_mut(*eq) {
            let volume = sfx_volume * ((fade_in_end - elapsed) / 0.5).clamp(0.0, 1.0);
            sink.set_volume(Volume::Linear(volume));
        }
    }

    let target_intensity = {
        let max_spotlight_intensity = f32::from_bits(gs_game.max_spotlight_intensity);
        let factor = if door_win_entities.animate_all { FAINT_ALIGNED_SPOTLIGHT_FACTOR } else { 1.0 };
        max_spotlight_intensity * intensity_factor * factor
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
            let lin = color.to_linear();
            material.emissive = LinearRgba::new(
                lin.red * intensity_factor,
                lin.green * intensity_factor,
                lin.blue * intensity_factor,
                lin.alpha,
            );
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

// Update the score bar based on the current level state.
pub fn update_score_bar(
    local_game_struct: Res<GameStateLocal>,
    mut dot_query: Query<(&ScoreBarDot, &mut BackgroundColor, &mut Node), (Without<ScoreBarChain>, Without<ScoreBarUI>)>,
    mut chain_query: Query<(&ScoreBarChain, &mut BackgroundColor, &mut Node), (Without<ScoreBarDot>, Without<ScoreBarUI>)>,
    mut row_query: Query<(&ScoreBarUI, &mut Node), (Without<ScoreBarDot>, Without<ScoreBarChain>)>,
) {
    let gs_game = &local_game_struct.0;
    let progress_bar_size: u32 = gs_game.progress_bar_size;
    let progress_bar_cur_size: u32 = gs_game.progress_bar_cur_size;

    let filled_color = Color::srgba(1.0, 1.0, 1.0, 0.5);
    let unfilled_dot_color = Color::srgba(1.0, 1.0, 1.0, 0.01);
    let filled_chain_color = Color::srgba(1.0, 1.0, 0.0, 0.3);
    let unfilled_chain_color = Color::srgba(1.0, 1.0, 0.0, 0.05);

    // Rows: hide a whole row when none of its dots are active.
    for (row, mut node) in row_query.iter_mut() {
        let row_active = row.row_start < progress_bar_size;
        let desired = if row_active { Display::Flex } else { Display::None };
        if node.display != desired {
            node.display = desired;
        }
    }

    // Dots: visible iff index < progress_bar_size; color depends on cur_size.
    for (dot, mut bg, mut node) in dot_query.iter_mut() {
        let visible = dot.index < progress_bar_size;
        let desired = if visible { Display::Flex } else { Display::None };
        if node.display != desired {
            node.display = desired;
        }
        if visible {
            let color = if dot.index < progress_bar_cur_size {
                filled_color
            } else {
                unfilled_dot_color
            };
            if bg.0 != color {
                *bg = BackgroundColor(color);
            }
        }
    }

    // Chains: visible iff both connected dots are within the same row AND active.
    for (chain, mut bg, mut node) in chain_query.iter_mut() {
        let dot_after = chain.index + 1;
        let same_row = chain.index / PROGRESS_BAR_WRAP_AROUND_SIZE
            == dot_after / PROGRESS_BAR_WRAP_AROUND_SIZE;
        let visible = same_row && dot_after < progress_bar_size;
        let desired = if visible { Display::Flex } else { Display::None };
        if node.display != desired {
            node.display = desired;
        }
        if visible {
            let color = if dot_after < progress_bar_cur_size {
                filled_chain_color
            } else {
                unfilled_chain_color
            };
            if bg.0 != color {
                *bg = BackgroundColor(color);
            }
        }
    }
}

pub fn update_faint_aligned_door(
    local_game_struct: Res<GameStateLocal>,
    camera_query: Query<&Transform, With<Camera3d>>,
    door_query: Query<(&BaseDoor, &GlobalTransform)>,
    mut light_query: Query<(&HoleLight, &mut Visibility, &mut SpotLight)>,
    mut emissive_query: Query<
        (&HoleEmissive, &mut Visibility, &MeshMaterial3d<StandardMaterial>),
        Without<HoleLight>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let gs_game = &local_game_struct.0;

    let set_emissive = |
        vis: &mut Visibility,
        mat_handle: &MeshMaterial3d<StandardMaterial>,
        materials: &mut Assets<StandardMaterial>,
        visible: bool,
        emissive: LinearRgba,
    | {
        let desired_vis = if visible { Visibility::Visible } else { Visibility::Hidden };
        if *vis != desired_vis {
            *vis = desired_vis;
        }
        if let Some(mat) = materials.get_mut(&mat_handle.0) {
            if mat.emissive != emissive {
                mat.emissive = emissive;
            }
        }
    };

    if gs_game.is_animating {
        for (_hole, mut vis, mat_handle) in emissive_query.iter_mut() {
            set_emissive(&mut vis, mat_handle, &mut materials, false, LinearRgba::BLACK);
        }
        for (_hole, mut vis, mut spotlight) in light_query.iter_mut() {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            spotlight.intensity = 0.0;
            spotlight.range = HOLE_SPOTLIGHT_RANGE;
        }
        return;
    }

    let Ok(camera_transform) = camera_query.single() else {
        return;
    };
    let cam_forward = camera_transform.forward();
    let cam_xz = Vec3::new(cam_forward.x, 0.0, cam_forward.z).normalize_or_zero();
    if cam_xz.length_squared() == 0.0 {
        return;
    }

    let mut best_idx: Option<usize> = None;
    let mut best_alignment = f32::NEG_INFINITY;
    for (door, door_global) in &door_query {
        let n_world = door_global.rotation() * door.normal;
        let n_xz = Vec3::new(n_world.x, 0.0, n_world.z).normalize_or_zero();
        let a = n_xz.dot(cam_xz);
        if a > best_alignment {
            best_alignment = a;
            best_idx = Some(door.door_index);
        }
    }
    let Some(best_idx) = best_idx else { return };

    let lin = Color::WHITE.to_linear();
    let faint = LinearRgba::new(
        lin.red   * FAINT_ALIGNED_INTENSITY_FACTOR,
        lin.green * FAINT_ALIGNED_INTENSITY_FACTOR,
        lin.blue  * FAINT_ALIGNED_INTENSITY_FACTOR,
        FAINT_ALIGNED_INTENSITY_FACTOR,
    );
    for (hole, mut vis, mat_handle) in emissive_query.iter_mut() {
        let is_aligned = hole.door_index == best_idx;
        let target = if is_aligned { faint } else { LinearRgba::BLACK };
        set_emissive(&mut vis, mat_handle, &mut materials, is_aligned, target);
    }

    let max_intensity = f32::from_bits(gs_game.max_spotlight_intensity);
    let faint_intensity = max_intensity * FAINT_ALIGNED_SPOTLIGHT_FACTOR;
    for (hole, mut vis, mut spotlight) in light_query.iter_mut() {
        let is_aligned = hole.door_index == best_idx;
        let desired_vis = if is_aligned { Visibility::Visible } else { Visibility::Hidden };
        if *vis != desired_vis {
            *vis = desired_vis;
        }
        if is_aligned {
            spotlight.intensity = faint_intensity;
            spotlight.color = Color::WHITE;
            spotlight.range = FAINT_ALIGNED_SPOTLIGHT_RANGE;
        } else {
            spotlight.intensity = 0.0;
            spotlight.range = HOLE_SPOTLIGHT_RANGE;
        }
    }
}