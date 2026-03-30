//! Command handler
//! This module reads from Shared Memory and updates the game resources (`PendingRotation`, etc.).

use bevy::prelude::*;
use core::sync::atomic::Ordering;
#[cfg(not(target_arch = "wasm32"))]
use shared::create_shared_memory;
use shared::constants::camera_3d_constants::{CAMERA_3D_SPEED_ROTATE, CAMERA_3D_SPEED_ZOOM};
use shared::SharedMemoryHandle;
use crate::utils::objects::GameStateLocal;

#[derive(Resource)]
pub struct SharedMemResource(pub SharedMemoryHandle);

#[derive(Resource, Default)]
pub struct PendingCommands {
    pub reset: bool,
    pub rotation: f32,
    pub zoom: f32,
    pub check_alignment: bool,
    pub blank_screen: bool,
    pub stop_rendering: bool,
    pub animation_door: bool,
    pub animation_all_door: bool,
    pub animation_colored: bool,
}
pub struct SharedMemoryReaderPlugin;

impl Plugin for SharedMemoryReaderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingCommands>();
    }
}

#[cfg_attr(target_arch = "wasm32", allow(unused_variables, unused_mut))]
pub fn init_shared_memory_system(mut commands: Commands) {
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

pub fn clear_pending_commands(
    mut pending_commands: ResMut<PendingCommands>,
) {
    pending_commands.reset = false;
    pending_commands.rotation = 0.0;
    pending_commands.zoom = 0.0;
    pending_commands.check_alignment = false;
    pending_commands.blank_screen = false;
    pending_commands.stop_rendering = false;
    pending_commands.animation_door = false;
    pending_commands.animation_all_door = false;
    pending_commands.animation_colored = false;
}

pub fn read_shared_memory_commands(
    shm_res: Option<Res<SharedMemResource>>,
    mut pending_commands: ResMut<PendingCommands>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();

    // Read commands from shared memory and apply pending
    if shm.commands.rotate_left.load(Ordering::Relaxed) {
        pending_commands.rotation -= CAMERA_3D_SPEED_ROTATE;
    }
    if shm.commands.rotate_right.load(Ordering::Relaxed) {
        pending_commands.rotation += CAMERA_3D_SPEED_ROTATE;
    }
    if shm.commands.zoom_in.load(Ordering::Relaxed) {
        pending_commands.zoom -= CAMERA_3D_SPEED_ZOOM;
    }
    if shm.commands.zoom_out.load(Ordering::Relaxed) {
        pending_commands.zoom += CAMERA_3D_SPEED_ZOOM;
    }

    // New rendering control commands
    pending_commands.stop_rendering = shm.commands.stop_rendering.load(Ordering::Relaxed);    
    pending_commands.animation_door = shm.commands.animation_door.load(Ordering::Relaxed);
    pending_commands.check_alignment = shm.commands.check_alignment.load(Ordering::Relaxed);
    pending_commands.blank_screen = shm.commands.blank_screen.load(Ordering::Relaxed);
    pending_commands.reset = shm.commands.reset.load(Ordering::Relaxed);
    pending_commands.animation_all_door = shm.commands.animation_all_door.load(Ordering::Relaxed);
    pending_commands.animation_colored = shm.commands.animation_colored.load(Ordering::Relaxed);

    // Clear all commands in shared memory after reading
    clear_shared_memory_commands(Some(shm_res));
}

// Clear shared memory of commands after reading
pub fn clear_shared_memory_commands(
    shm_res: Option<Res<SharedMemResource>>
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();

    // Clear all commands in shared memory after reading
    shm.commands.rotate_left.store(false, Ordering::Relaxed);
    shm.commands.rotate_right.store(false, Ordering::Relaxed);
    shm.commands.zoom_in.store(false, Ordering::Relaxed);
    shm.commands.zoom_out.store(false, Ordering::Relaxed);
    shm.commands.stop_rendering.store(false, Ordering::Relaxed);
    shm.commands.animation_door.store(false, Ordering::Relaxed);
    shm.commands.check_alignment.store(false, Ordering::Relaxed);
    shm.commands.blank_screen.store(false, Ordering::Relaxed);
    shm.commands.reset.store(false, Ordering::Relaxed);
    shm.commands.animation_all_door.store(false, Ordering::Relaxed);
    shm.commands.animation_colored.store(false, Ordering::Relaxed);
}

// Read shared memory to local structure (from game to local)
pub fn read_shared_memory_game_state_local(
    shm_res: Option<Res<SharedMemResource>>,
    mut local_game_struct: ResMut<GameStateLocal>,
) {
    let Some(shm_res) = shm_res else { return };
    let shm = shm_res.0.get();

    let gs_game = &shm.game_structure_control;

    // Update local to copy
    local_game_struct.0 = gs_game.to_not_atomic();
}