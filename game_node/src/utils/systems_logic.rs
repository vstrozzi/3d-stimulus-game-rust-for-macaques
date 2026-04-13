//! Game logic wrapped up using the various plugins.

use bevy::prelude::*;
use crate::shared_memory::shared_memory_reader::{clear_pending_commands, init_shared_memory_system, read_shared_memory_commands, read_shared_memory_game_state_local};
use crate::shared_memory::shared_memory_writer::{write_shared_memory_game_state, increment_timing, update_shared_memory_local, increment_render_frame_counter};
use crate::utils::camera::{spawn_persistent_camera};
use crate::utils::ui::{update_ui_scale};
use crate::utils::game_functions::{
    handle_door_animation,
    update_score_bar,
};
use crate::utils::setup::setup_environment;
use crate::utils::handle_commands::{handle_check_alignment, handle_reset_command, handle_animation_door_command, handle_blank_screen, handle_stop_rendering, handle_rotation, handle_zoom};
use crate::utils::load_textures::{preload_all_textures, check_scene_ready};

/// Plugin for managing all the game systems.config
pub struct SystemsLogicPlugin;

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
                ).chain())
            // Shared memory
            .add_systems(
                FixedPreUpdate,
                (read_shared_memory_commands, read_shared_memory_game_state_local).chain(),
            )
            // Global UI responsiveness system (runs every frame)
            .add_systems(Update, update_ui_scale)
            // Check texture readiness each frame
            .add_systems(Update, check_scene_ready)
            // Render frame counter (once per actual render frame)
            .add_systems(Update, increment_render_frame_counter)
            // Command driven
            .add_systems(
                FixedUpdate,
                (
                    handle_reset_command,
                    handle_animation_door_command,
                    handle_blank_screen,
                    handle_stop_rendering,
                    handle_rotation,
                    handle_zoom,
                    handle_check_alignment,
                    handle_door_animation,
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

