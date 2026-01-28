//! Command handler for the Twin-Engine architecture.
//!
//! This module reads from Shared Memory and updates the game resources (`PendingRotation`, etc.).

use bevy::prelude::*;
use shared::SharedMemoryHandle;
#[cfg(not(target_arch = "wasm32"))]
use shared::create_shared_memory;
use crate::utils::objects::GameConfig;
use core::sync::atomic::Ordering;

// ============================================================================
// RESOURCES
// ============================================================================

#[derive(Resource)]
pub struct SharedMemResource(pub SharedMemoryHandle);

/// Resource to store the active game configuration.
#[derive(Resource, Default)]
pub struct ActiveConfig(pub Option<GameConfig>);

/// Resource to store a pending reset configuration.
#[derive(Resource, Default)]
pub struct PendingReset(pub Option<GameConfig>);

/// Track pending actions (deltas per frame)
#[derive(Resource, Default)]
pub struct PendingRotation(pub f32);

#[derive(Resource, Default)]
pub struct PendingZoom(pub f32);

#[derive(Resource, Default)]
pub struct PendingCheckAlignment(pub bool);

/// Pending command to blank the screen (show black overlay)
#[derive(Resource, Default)]
pub struct PendingBlankScreen(pub bool);

/// Resource tracking whether rendering is currently paused
#[derive(Resource, Default)]
pub struct RenderingPaused(pub bool);

// ============================================================================
// PLUGIN
// ============================================================================

pub struct CommandHandlerPlugin;

impl Plugin for CommandHandlerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveConfig>()
            .init_resource::<PendingReset>()
            .init_resource::<PendingRotation>()
            .init_resource::<PendingZoom>()
            .init_resource::<PendingCheckAlignment>()
            .init_resource::<PendingBlankScreen>()
            .init_resource::<RenderingPaused>()
            .add_systems(Startup, init_shared_memory_system)
            .add_systems(PreUpdate, (clear_pending_actions, read_shared_memory).chain());
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

#[cfg_attr(target_arch = "wasm32", allow(unused_variables, unused_mut))]
fn init_shared_memory_system(mut commands: Commands) {
    let name = "monkey_game";

    #[cfg(not(target_arch = "wasm32"))]
    {
        match create_shared_memory(name) {
            Ok(handle) => {
                info!("Shared Memory initialized successfully.");
                commands.insert_resource(SharedMemResource(handle));
            }
            Err(e) => {
                error!("Failed to initialize shared memory: {}", e);
            }
        }
    }
}

fn clear_pending_actions(
    mut pending_rotation: ResMut<PendingRotation>,
    mut pending_zoom: ResMut<PendingZoom>,
    mut pending_check: ResMut<PendingCheckAlignment>,
    mut pending_blank: ResMut<PendingBlankScreen>,
) {
    pending_rotation.0 = 0.0;
    pending_zoom.0 = 0.0;
    pending_check.0 = false;
    pending_blank.0 = false;
}

fn read_shared_memory(
    shm_res: Option<Res<SharedMemResource>>,
    mut pending_reset: ResMut<PendingReset>,
    mut pending_rotation: ResMut<PendingRotation>,
    mut pending_zoom: ResMut<PendingZoom>,
    mut pending_check: ResMut<PendingCheckAlignment>,
    mut pending_blank: ResMut<PendingBlankScreen>,
    mut rendering_paused: ResMut<RenderingPaused>,
    mut active_config: ResMut<ActiveConfig>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();

    // 1. Read Continuous Inputs using atomics
    const ROT_SPEED: f32 = 0.05;
    const ZOOM_SPEED: f32 = 0.10;

    if shm.commands.rotate_left.load(Ordering::Relaxed) {
        pending_rotation.0 -= ROT_SPEED;
    }
    if shm.commands.rotate_right.load(Ordering::Relaxed) {
        pending_rotation.0 += ROT_SPEED;
    }
    if shm.commands.zoom_in.load(Ordering::Relaxed) {
        pending_zoom.0 -= ZOOM_SPEED;
    }
    if shm.commands.zoom_out.load(Ordering::Relaxed) {
        pending_zoom.0 += ZOOM_SPEED;
    }

    // 2. Read Trigger Inputs (swap to clear after reading)
    if shm.commands.check_alignment.swap(false, Ordering::Relaxed) {
        pending_check.0 = true;
    }

    // 3. New rendering control commands
    if shm.commands.blank_screen.swap(false, Ordering::Relaxed) {
        pending_blank.0 = true;
    }
    if shm.commands.stop_rendering.swap(false, Ordering::Relaxed) {
        rendering_paused.0 = true;
        info!("Rendering paused via SHM command");
    }
    if shm.commands.resume_rendering.swap(false, Ordering::Relaxed) {
        rendering_paused.0 = false;
        info!("Rendering resumed via SHM command");
    }

    // 4. Reset Handshake - read config from game_structure
    if shm.commands.reset.load(Ordering::Acquire) {
        let gs = &shm.game_structure;

        let seed = gs.seed.load(Ordering::Relaxed);
        let p_type = gs.pyramid_type.load(Ordering::Relaxed);
        let radius = f32::from_bits(gs.base_radius.load(Ordering::Relaxed));
        let height = f32::from_bits(gs.height.load(Ordering::Relaxed));
        let orient = f32::from_bits(gs.start_orient.load(Ordering::Relaxed));
        let target = gs.target_door.load(Ordering::Relaxed) as usize;

        let mut colors = [[0.0; 4]; 3];
        for i in 0..12 {
            let val = f32::from_bits(gs.colors[i].load(Ordering::Relaxed));
            let face = i / 4;
            let chan = i % 4;
            colors[face][chan] = val;
        }

        let new_config = GameConfig {
            seed,
            pyramid_type_code: p_type,
            pyramid_base_radius: radius,
            pyramid_height: height,
            pyramid_start_orientation_rad: orient,
            pyramid_target_door_index: target,
            pyramid_color_faces: colors,
        };

        info!("Reset triggered from SHM. Seed: {}", seed);
        pending_reset.0 = Some(new_config.clone());
        active_config.0 = Some(new_config);

        shm.commands.reset.store(false, Ordering::Release);
    }
}
