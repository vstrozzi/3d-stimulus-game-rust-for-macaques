use bevy::prelude::*;

#[cfg(target_arch = "wasm32")]
use crate::command_handler::SharedMemResource;

#[cfg(target_arch = "wasm32")]
use shared::open_shared_memory;



/// Plugin to add wasm shared memory to the Bevy systems
pub struct WebAdapterPlugin;

impl Plugin for WebAdapterPlugin {
    fn build(&self, _app: &mut App) {
        #[cfg(target_arch = "wasm32")]
        _app.add_systems(Startup, init_web_shm);
    }
}

#[cfg(target_arch = "wasm32")]
fn init_web_shm(mut commands: Commands) {
    match open_shared_memory("monkey_game") {
        Ok(handle) => {
             info!("Web Shared Memory attached.");
             commands.insert_resource(SharedMemResource(handle));
        },
        Err(e) => {
             warn!("Web Shared Memory not ready yet: {}", e);
        }
    }
}
