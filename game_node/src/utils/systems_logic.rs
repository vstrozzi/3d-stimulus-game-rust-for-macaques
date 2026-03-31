//! Game logic wrapped up using the various plugins.

use bevy::prelude::*;
use crate::shared_memory::shared_memory_reader::{clear_pending_commands, init_shared_memory_system, read_shared_memory_commands, read_shared_memory_game_state_local};
use crate::shared_memory::shared_memory_writer::{write_shared_memory_game_state, increment_timing, update_shared_memory_local};
use crate::utils::camera::{spawn_persistent_camera};
use crate::utils::ui::{update_ui_scale};
use crate::utils::game_functions::{
    handle_door_animation,
    update_score_bar,
};
use crate::utils::setup::setup_environment;
use crate::utils::handle_commands::{handle_check_alignment, handle_reset_command, handle_animation_door_command, handle_blank_screen, handle_stop_rendering, handle_rotation, handle_zoom};
use crate::utils::load_textures::load_texture_set;
use crate::utils::objects::{PreloadedTextures, GameConditions, GameStateLocal};

/// Load every texture set at startup and keep the handles in a resource.
/// This prevents Bevy from GC-ing images between resets, eliminating WASM fetch stalls.
fn preload_all_textures(asset_server: Res<AssetServer>, mut preloaded: ResMut<PreloadedTextures>) {
    use shared::Texture;
    use strum::IntoEnumIterator;
    for tex in Texture::iter() {
        preloaded.0.insert(tex, load_texture_set(&asset_server, &tex.asset_folder()));
    }
}

/// Each frame while `is_scene_ready` is false, check whether all textures used by the
/// current trial are fully loaded (including GPU upload). Once confirmed, set the flag so
/// the controller knows it is safe to remove the blank screen.
fn check_scene_ready(
    mut game_conditions: ResMut<GameConditions>,
    preloaded: Res<PreloadedTextures>,
    images: Res<Assets<Image>>,
    local_game_struct: Res<GameStateLocal>,
) {
    if game_conditions.is_scene_ready {
        return;
    }

    use shared::Texture;
    let gs = &local_game_struct.0;

    // All six texture slots (face + decoration) used by the current trial config must be
    // present in the Assets<Image> store before we allow the controller to start the trial.
    let all_loaded = gs.textures.iter()
        .chain(gs.decorations_texture.iter())
        .all(|&t| {
            let tex = Texture::from_u32(t);
            let res = 
            preloaded.0.get(&tex)
                .map(|set| set.all_loaded(&images))
                .unwrap_or(false);
            res
        });

    if all_loaded {
        game_conditions.is_scene_ready = true;
    }
}

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
                    setup_environment,
                    preload_all_textures))
            // Shared memory
            .add_systems(
                PreUpdate,
                (read_shared_memory_commands, read_shared_memory_game_state_local).chain(),
            )
            // Global UI responsiveness system (runs every frame)
            .add_systems(Update, update_ui_scale)
            // Check texture readiness each frame
            .add_systems(Update, check_scene_ready)
            // Command driven
            .add_systems(
                Update,
                (
                    handle_reset_command,
                    handle_animation_door_command,
                    handle_blank_screen,
                    handle_stop_rendering,
                    handle_rotation,
                    handle_zoom,
                    handle_check_alignment.after(handle_animation_door_command),
                    ),
            )
            // Commands that needs to chain

            // Animations
            .add_systems(
                Update,
                (
                    handle_door_animation,
                    update_score_bar,
                )
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
    }
}

