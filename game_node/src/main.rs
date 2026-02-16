//! Start-up for the monkey_3d_game, with window, plugins, and resources.
//! This is the Game Node. It receives commands from the Controller and emits state via Shared Memory.

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::*,
};


// Re-export shared memory functions for WASM
#[cfg(target_arch = "wasm32")]
use shared::{create_shared_memory_wasm, WebSharedMemory};

// TODO: use constants from structure
use shared::constants::game_constants::REFRESH_RATE_HZ;
use game_node::{
    shared_memory::{
        shared_memory_reader::SharedMemoryReaderPlugin,
        shared_memory_writer::StateEmitterPlugin,
        shared_memory_web_extension::WebAdapterPlugin,
    },
    utils::{
        debug_functions::DebugFunctionsPlugin,
        objects::{DoorWinEntities, RoundStartTimestamp, GameStateLocal},
        systems_logic::SystemsLogicPlugin,
    },
};

/// Entry point for the application
fn main() {
    let window = Some(Window {
        title: "Monkey 3D Game".into(),
        #[cfg(target_arch = "wasm32")]
        canvas: Some("#game-canvas".into()),
        fit_canvas_to_parent: true,
        prevent_default_event_handling: true,
        #[cfg(not(target_arch = "wasm32"))]
        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
        present_mode: PresentMode::AutoVsync,
        ..default()
    });

    let cursor = Some(CursorOptions {
        grab_mode: CursorGrabMode::None,
        visible: false,
        ..default()
    });

    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: window,
                primary_cursor_options: cursor,
                ..default()
            }),
            LogDiagnosticsPlugin::default(),
            FrameTimeDiagnosticsPlugin::default(),
            SharedMemoryReaderPlugin, // Read shared memory and init bevy resources, preupdate
            SystemsLogicPlugin,   // Game logic systems, update
            DebugFunctionsPlugin, // Debug functions, update
            StateEmitterPlugin,   // Write shared memory, update timing, init timing resource, postupdate
            WebAdapterPlugin, 
        ))
        // Fixed resources across trials
        .insert_resource(Time::<Fixed>::from_hz(REFRESH_RATE_HZ)) 
        .insert_resource(DoorWinEntities::default())
        .insert_resource(RoundStartTimestamp::default())
        .insert_resource(GameStateLocal::default())
        .run();
}