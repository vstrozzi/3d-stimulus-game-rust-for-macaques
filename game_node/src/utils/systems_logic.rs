//! Game logic wrapped up using the various plugins.

use bevy::prelude::*;
use crate::shared_memory::shared_memory_reader::{PendingCommands, clear_pending_commands, init_shared_memory_system, read_shared_memory_commands, read_shared_memory_game_state_local};
use crate::shared_memory::shared_memory_writer::{write_shared_memory_game_state, increment_frame_counter, update_shared_memory_local};
use crate::utils::camera::{spawn_persistent_camera};
use crate::utils::ui::{update_ui_scale};
use crate::utils::game_functions::{
    apply_pending_check_alignment, handle_door_animation,
    update_score_bar_animation,
};
use crate::utils::setup::setup_environment;
use crate::utils::handle_commands::{ handle_reset_command, handle_animation_door_command, handle_blank_screen, handle_rotation, handle_zoom};

// Plugin for managing all the game systems.config
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
                    setup_environment))
            // Shared memory
            .add_systems(
                PreUpdate,
                (read_shared_memory_commands, read_shared_memory_game_state_local).chain(),
            )
            // Global UI responsiveness system (runs every frame)
            .add_systems(Update, update_ui_scale)
            // Command driven
            .add_systems(
                Update,
                (
                    handle_reset_command,
                    handle_animation_door_command,
                    handle_blank_screen,
                    ),
            )
            // Animations
            .add_systems(
                Update,
                (
                    handle_door_animation,
                    update_score_bar_animation,
                )
                    .run_if(is_not_paused)
            )
            // Input and Logic Systems
            .add_systems(
                Update,
                ((
                        handle_rotation,
                        handle_zoom,
                        apply_pending_check_alignment,
                    )
                        .run_if(is_not_paused)
                        .run_if(is_not_animating),

                ).chain(),
            )
            // Post Update
            .add_systems(
                PostUpdate,
                (
                clear_pending_commands,
                increment_frame_counter,
                update_shared_memory_local,
                write_shared_memory_game_state).chain()
            );  
    }
}

fn is_not_paused(pending: Res<PendingCommands>) -> bool {
    !pending.stop_rendering
}

fn is_not_animating(pending: Res<PendingCommands>) -> bool {
    !pending.animation_door
}

