//! Start-up for the monkey_3d_game, with window, plugins, and resources.
//!
//! Twin-Engine Architecture: This is the Game Node. It receives commands from
//! the Controller and emits state via Shared Memory.

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::*,
};

use game_node::{
    command_handler::CommandHandlerPlugin,
    state_emitter::StateEmitterPlugin,
    web_adapter::WebAdapterPlugin,
    // native_adapter removed, integrated into command_handler
    utils::{
        constants::game_constants::REFRESH_RATE_HZ,
        debug_functions::DebugFunctionsPlugin,
        objects::{GameState, RandomGen},
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
            // Twin-Engine plugins
            CommandHandlerPlugin, // Now handles SHM reading
            StateEmitterPlugin,   // Now handles SHM writing
            WebAdapterPlugin,     // Handles WASM SHM init
            // Custom game plugins
            SystemsLogicPlugin,
            DebugFunctionsPlugin,
        ))
        .insert_resource(Time::<Fixed>::from_hz(REFRESH_RATE_HZ))
        .insert_resource(RandomGen::default())
        .insert_resource(GameState::default())
        .run();
}
