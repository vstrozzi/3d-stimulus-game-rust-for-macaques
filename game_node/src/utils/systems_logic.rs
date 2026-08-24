//! Game logic wrapped up using the various plugins.

use bevy::prelude::*;
use crate::shared_memory::shared_memory_reader::{clear_pending_commands, init_shared_memory_system, read_shared_memory_commands, read_shared_memory_game_state_local, sync_live_state_from_shm};
use crate::shared_memory::shared_memory_writer::{write_shared_memory_game_state, increment_timing, update_shared_memory_local, stage_render_sample, commit_render_sample, discard_staged_samples_on_reset};
use crate::utils::camera::{spawn_persistent_camera, handle_camera_shake, setup_fixed_resolution, on_window_resized};
use crate::utils::ui::{spawn_score_bar_pool, spawn_left_score_bar, update_left_score_bar, spawn_session_clock, update_session_clock, update_ui_scale};
use crate::utils::game_functions::{
    handle_door_animation,
    update_faint_aligned_door,
    update_score_bar,
};
use crate::utils::setup::setup_environment;
use crate::utils::fog::{setup_ambient_motes, setup_fog, update_ambient_motes, update_fog, update_fireflies, FireflyState};
use crate::utils::handle_commands::{handle_check_alignment, handle_reset_command, handle_animation_door_command, handle_blank_screen, handle_stop_rendering, handle_rotation, handle_zoom};
use crate::utils::load_assets::{preload_required_textures, check_scene_ready, load_sounds, update_background_music_volume};
use crate::utils::warmup::{spawn_warmup_scene, tick_warmup};

/// Plugin for managing all the game systems.config
pub struct SystemsLogicPlugin;

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
                    load_sounds,
                    spawn_score_bar_pool,
                    spawn_left_score_bar,
                    spawn_session_clock,
                    setup_ambient_motes,
                ).chain())
            // The controller publishes the session textures through
            // shared memory. Load it once, then create the GPU warmup scene.
            .add_systems(Update, (preload_required_textures, spawn_warmup_scene).chain())
            // Shared memory
            .add_systems(
                PreUpdate,
                (read_shared_memory_commands, read_shared_memory_game_state_local, sync_live_state_from_shm).chain(),
            )
            // Offscreen render-to-texture + upscale.
            .add_systems(Update, (setup_fixed_resolution, on_window_resized))
            // Pyramid-centered distance fog + win-time gold fireflies.
            .add_systems(Update, (update_fog, update_fireflies, update_ambient_motes))
            // Global UI responsiveness system (runs every frame)
            .add_systems(Update, update_ui_scale)
            // Tick warmup state machine each frame; despawns warmup entities
            // and flips `WarmupState.complete` once GPU pipelines are hot.
            .add_systems(Update, tick_warmup)
            // Check texture readiness each frame (gated on warmup completion);
            // runs the black 3-2-1 countdown before flipping `is_scene_ready`.
            .add_systems(Update, check_scene_ready)
            .add_systems(Update, update_background_music_volume)
            // Commit render-world completion markers at the start of the main
            // frame. Each marker carries the ID of its exact state snapshot.
            .add_systems(First, commit_render_sample)
            // Command driven
            .add_systems(
                Update,
                (
                    discard_staged_samples_on_reset,
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
                    update_session_clock,
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
            )
            // Snapshot only after all Update/PostUpdate state and photodiode
            // changes are finished. Extraction carries this ID to RenderApp.
            .add_systems(Last, stage_render_sample);

        // Native: switch to exclusive fullscreen at a capped, aspect-matched
        // video mode so the whole pipeline runs at ~1080p (web stays on the
        // offscreen render path above).
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, crate::utils::camera::setup_fixed_fullscreen);
    }
}
