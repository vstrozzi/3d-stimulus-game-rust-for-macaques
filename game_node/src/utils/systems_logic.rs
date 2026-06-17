//! Game logic wrapped up using the various plugins.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::shared_memory::shared_memory_reader::{clear_pending_commands, init_shared_memory_system, read_shared_memory_commands, read_shared_memory_game_state_local, sync_live_state_from_shm};
use crate::shared_memory::shared_memory_writer::{write_shared_memory_game_state, increment_timing, update_shared_memory_local, stage_render_sample, commit_render_sample};
use crate::utils::camera::{spawn_persistent_camera, handle_camera_shake, setup_fixed_resolution, on_window_resized};
use crate::utils::ui::{spawn_score_bar_pool, spawn_left_score_bar, update_left_score_bar, update_ui_scale};
use crate::utils::game_functions::{
    handle_door_animation,
    update_faint_aligned_door,
    update_score_bar,
};
use crate::utils::setup::setup_environment;
use crate::utils::fog::{setup_fog, update_fog, update_fireflies, FireflyState};
use crate::utils::handle_commands::{handle_check_alignment, handle_reset_command, handle_animation_door_command, handle_blank_screen, handle_stop_rendering, handle_rotation, handle_zoom};
use crate::utils::load_assets::{preload_all_textures, check_scene_ready, load_sounds, update_background_music_volume};
use crate::utils::warmup::{spawn_warmup_scene, tick_warmup};

/// Plugin for managing all the game systems.config
pub struct SystemsLogicPlugin;

fn force_redraw_every_frame(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    // Touching the Window component flips its Changed<> flag, which causes
    // bevy_winit to request a redraw on the next about_to_wait.
    if let Ok(mut window) = windows.single_mut() {
        window.set_changed();
    }

}
impl Plugin for SystemsLogicPlugin {
    /// Builds the plugin by adding the systems to the app.
    fn build(&self, app: &mut App) {
        app
            .init_resource::<FireflyState>()
            // Spawn persistent camera and static environment once at startup
            .add_systems(
                Startup,
                (
                    init_shared_memory_system,
                    spawn_persistent_camera,
                    setup_fog,
                    setup_environment,
                    preload_all_textures,
                    load_sounds,
                    spawn_warmup_scene,
                    spawn_score_bar_pool,
                    spawn_left_score_bar,
                ).chain())
            // Shared memory
            .add_systems(
                PreUpdate,
                (read_shared_memory_commands, read_shared_memory_game_state_local, sync_live_state_from_shm).chain(),
            )
            // Offscreen render-to-texture + upscale.
            .add_systems(Update, (setup_fixed_resolution, on_window_resized))
            // Pyramid-centered distance fog + win-time gold fireflies.
            .add_systems(Update, (update_fog, update_fireflies))
            // Global UI responsiveness system (runs every frame)
            .add_systems(Update, update_ui_scale)
            // Tick warmup state machine each frame; despawns warmup entities
            // and flips `WarmupState.complete` once GPU pipelines are hot.
            .add_systems(Update, tick_warmup)
            // Check texture readiness each frame (gated on warmup completion);
            // runs the black 3-2-1 countdown before flipping `is_scene_ready`.
            .add_systems(Update, check_scene_ready)
            .add_systems(Update, update_background_music_volume)
            // Commit the previous render frame's sample at the very top of
            // the new frame (in `First`). 
            .add_systems(First, commit_render_sample)
            // Stage current render frame's data (counter, render submit time,
            // photodiode). The matching `present_elapsed_secs` is filled in
            // at the next frame's `First`.
            .add_systems(Update, stage_render_sample)
            .add_systems(Update, force_redraw_every_frame)
            // Command driven
            .add_systems(
                Update,
                (
                    handle_reset_command,
                    handle_check_alignment,
                    handle_blank_screen,
                    handle_stop_rendering,
                    handle_rotation,
                    handle_zoom,
                    update_faint_aligned_door,
                    handle_animation_door_command,
                    handle_door_animation,
                    handle_camera_shake,
                    update_score_bar,
                    update_left_score_bar,
                    ).chain(),
            )
            // Post Update
            .add_systems(
                PostUpdate,
                (
                clear_pending_commands,
                increment_timing,
                update_shared_memory_local,
                write_shared_memory_game_state).chain()
            );

        // Native: switch to exclusive fullscreen at a capped, aspect-matched
        // video mode so the whole pipeline runs at ~1080p (web stays on the
        // offscreen render path above).
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, crate::utils::camera::setup_fixed_fullscreen);
    }
}

