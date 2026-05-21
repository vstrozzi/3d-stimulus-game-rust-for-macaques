//! Game logic wrapped up using the various plugins.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::shared_memory::shared_memory_reader::{clear_pending_commands, init_shared_memory_system, read_shared_memory_commands, read_shared_memory_game_state_local};
use crate::shared_memory::shared_memory_writer::{write_shared_memory_game_state, increment_timing, update_shared_memory_local, stage_render_sample, commit_render_sample};
use crate::utils::camera::{spawn_persistent_camera};
use crate::utils::ui::{spawn_score_bar_pool, update_ui_scale};
use crate::utils::game_functions::{
    handle_door_animation,
    update_score_bar,
};
use crate::utils::setup::setup_environment;
use crate::utils::handle_commands::{handle_check_alignment, handle_reset_command, handle_animation_door_command, handle_blank_screen, handle_stop_rendering, handle_rotation, handle_zoom};
use crate::utils::load_textures::{preload_all_textures, check_scene_ready};
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
            // Spawn persistent camera and static environment once at startup
            .add_systems(
                Startup,
                (
                    init_shared_memory_system,
                    spawn_persistent_camera,
                    setup_environment,
                    preload_all_textures,
                    spawn_warmup_scene,
                    spawn_score_bar_pool,
                ).chain())
            // Shared memory
            .add_systems(
                FixedPreUpdate,
                (read_shared_memory_commands, read_shared_memory_game_state_local).chain(),
            )
            // Global UI responsiveness system (runs every frame)
            .add_systems(Update, update_ui_scale)
            // Tick warmup state machine each frame; despawns warmup entities
            // and flips `WarmupState.complete` once GPU pipelines are hot.
            .add_systems(Update, tick_warmup)
            // Check texture readiness each frame (gated on warmup completion).
            .add_systems(Update, check_scene_ready)
            // Commit the previous render frame's sample at the very top of
            // the new frame (in `First`). At this point `present()` has just
            // returned and the next swapchain image was acquired, so wall-
            // clock now ≈ vsync time of the prior flip. That stamp is what we
            // write into `present_elapsed_secs` for the frame we just rendered.
            .add_systems(First, commit_render_sample)
            // Stage current render frame's data (counter, render submit time,
            // photodiode). The matching `present_elapsed_secs` is filled in
            // at the next frame's `First`.
            .add_systems(Update, stage_render_sample)
            .add_systems(Update, force_redraw_every_frame)
            // Command driven
            .add_systems(
                FixedUpdate,
                (
                    handle_reset_command,
                    handle_check_alignment,
                    handle_blank_screen,
                    handle_stop_rendering,
                    handle_rotation,
                    handle_zoom,
                    handle_door_animation,
                    handle_animation_door_command,
                    update_score_bar,
                    ).chain(),
            )
            // Post Update
            .add_systems(
                FixedPostUpdate,
                (
                clear_pending_commands,
                increment_timing,
                update_shared_memory_local,
                write_shared_memory_game_state).chain()
            );  
    }
}

