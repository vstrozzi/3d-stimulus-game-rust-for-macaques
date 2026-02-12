//! Command handler
//! This module reads from Shared Memory and updates the game resources (`PendingRotation`, etc.).

use bevy::prelude::*;
use core::sync::atomic::Ordering;
#[cfg(not(target_arch = "wasm32"))]
use shared::create_shared_memory;
use shared::constants::camera_3d_constants::{CAMERA_3D_SPEED_ROTATE, CAMERA_3D_SPEED_ZOOM};
use shared::SharedMemoryHandle;

#[derive(Resource)]
pub struct SharedMemResource(pub SharedMemoryHandle);

#[derive(Resource, Default)]
pub struct PendingReset(pub bool);

#[derive(Resource, Default)]
pub struct PendingRotation(pub f32);

#[derive(Resource, Default)]
pub struct PendingZoom(pub f32);

#[derive(Resource, Default)]
pub struct PendingCheckAlignment(pub bool);

#[derive(Resource, Default)]
pub struct PendingBlankScreen(pub bool);

#[derive(Resource, Default)]
pub struct RenderingPaused(pub bool);

#[derive(Resource, Default)]
pub struct PendingAnimation(pub bool);

pub struct CommandHandlerPlugin;

impl Plugin for CommandHandlerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingReset>()
            .init_resource::<PendingRotation>()
            .init_resource::<PendingZoom>()
            .init_resource::<PendingCheckAlignment>()
            .init_resource::<PendingBlankScreen>()
            .init_resource::<PendingBlankScreen>()
            .init_resource::<RenderingPaused>()
            .init_resource::<PendingAnimation>()
            .add_systems(Startup, init_shared_memory_system)
            .add_systems(
                PreUpdate,
                (clear_pending_actions, read_shared_memory).chain(),
            );
    }
}

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
    mut pending_reset: ResMut<PendingReset>,
    mut pending_rotation: ResMut<PendingRotation>,
    mut pending_zoom: ResMut<PendingZoom>,
    mut pending_check: ResMut<PendingCheckAlignment>,
    mut pending_blank: ResMut<PendingBlankScreen>,
    mut rendering_paused: ResMut<RenderingPaused>,
    mut pending_anim: ResMut<PendingAnimation>,
    
) {
    pending_rotation.0 = 0.0;
    pending_zoom.0 = 0.0;
    pending_check.0 = false;
    pending_blank.0 = false;
    pending_anim.0 = false;
    pending_reset.0 = false;
    rendering_paused.0 = false;
}

fn read_shared_memory(
    shm_res: Option<Res<SharedMemResource>>,
    mut pending_reset: ResMut<PendingReset>,
    mut pending_rotation: ResMut<PendingRotation>,
    mut pending_zoom: ResMut<PendingZoom>,
    mut pending_check: ResMut<PendingCheckAlignment>,
    mut pending_blank: ResMut<PendingBlankScreen>,
    mut rendering_paused: ResMut<RenderingPaused>,
    mut pending_anim: ResMut<PendingAnimation>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();

    // Read commands from shared memory and apply pending
    if shm.commands.rotate_left.load(Ordering::Relaxed) {
        pending_rotation.0 -= CAMERA_3D_SPEED_ROTATE;
    }
    if shm.commands.rotate_right.load(Ordering::Relaxed) {
        pending_rotation.0 += CAMERA_3D_SPEED_ROTATE;
    }
    if shm.commands.zoom_in.load(Ordering::Relaxed) {
        pending_zoom.0 -= CAMERA_3D_SPEED_ZOOM;
    }
    if shm.commands.zoom_out.load(Ordering::Relaxed) {
        pending_zoom.0 += CAMERA_3D_SPEED_ZOOM;
    }

    // Read Trigger Inputs (swap to clear after reading)
    if shm.commands.check_alignment.load( Ordering::Relaxed) {
        pending_check.0 = true;
    }

    // New rendering control commands
    if shm.commands.blank_screen.load( Ordering::Relaxed) {
        pending_blank.0 = true;
    }
    if shm.commands.stop_rendering.load( Ordering::Relaxed) {
        rendering_paused.0 = true;
    }
    if shm.commands.resume_rendering.load(Ordering::Relaxed) {
        rendering_paused.0 = false;
    }

    if shm.commands.animation_door.load(Ordering::Relaxed) {
        pending_anim.0 = true;
    }

    if shm.commands.reset.load(Ordering::Relaxed) {
        pending_reset.0 = true;
    }

}
