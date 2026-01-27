//! Declaration of the utils modules for monkey_3d_game.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::*,
};

/// Command handler for receiving commands from the Controller
pub mod command_handler;

/// State emitter for sending game state to the Controller
pub mod state_emitter;

/// Web adapter for WASM integration
pub mod web_adapter;

/// Various utility functions, constants, and objects
pub mod utils {
    pub mod camera;
    pub mod constants;
    pub mod debug_functions;
    pub mod game_functions;
    pub mod macros;
    pub mod objects;
    pub mod pyramid;
    pub mod setup;
    pub mod systems_logic;
}

// Re-export shared memory functions for WASM
#[cfg(target_arch = "wasm32")]
pub use shared::{create_shared_memory_wasm, WebSharedMemory};

/// WASM entry point - call this manually from JS after create_shared_memory_wasm()
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn wasm_main() {
    use crate::{
        command_handler::CommandHandlerPlugin,
        state_emitter::StateEmitterPlugin,
        web_adapter::WebAdapterPlugin,
        utils::{
            constants::game_constants::REFRESH_RATE_HZ,
            debug_functions::DebugFunctionsPlugin,
            objects::{GameState, RandomGen},
            systems_logic::SystemsLogicPlugin,
        },
    };

    let window = Some(Window {
        title: "Monkey 3D Game".into(),
        canvas: Some("#game-canvas".into()),
        fit_canvas_to_parent: true,
        prevent_default_event_handling: true,
        present_mode: PresentMode::AutoVsync,
        ..default()
    });

    let cursor = Some(CursorOptions {
        grab_mode: CursorGrabMode::Locked,
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
            CommandHandlerPlugin,
            StateEmitterPlugin,
            WebAdapterPlugin,
            SystemsLogicPlugin,
            DebugFunctionsPlugin,
        ))
        .insert_resource(Time::<Fixed>::from_hz(REFRESH_RATE_HZ))
        .insert_resource(RandomGen::default())
        .insert_resource(GameState::default())
        .run();
}
