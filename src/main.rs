//! Start-up for the monkey_3d_game, with window, plugins, and resources.

/// Import necessary modules from the Bevy engine
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::*,
};

/// Import custom modules game defined
use monkey_3d_game::utils::{
    constants::game_constants::REFRESH_RATE_HZ,
    debug_functions::DebugFunctionsPlugin,
    global_inputs::InputsPlugin,
    objects::{GameState, RandomGen},
    systems_logic::SystemsLogicPlugin,
    touch_inputs::TouchInputPlugin,
};

/// Entry point for the application
fn main() {
    let window = Some(Window {
        title: "Monkey 3D Game".into(),
        fit_canvas_to_parent: true,            // Wasm size as canvas
        prevent_default_event_handling: false, // Wasm not override default (F5, ...)
        #[cfg(not(target_arch = "wasm32"))]
        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary), // Native fullscreen
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
            // Default plugins with the custom window and cursor settings
            DefaultPlugins.set(WindowPlugin {
                primary_window: window,
                primary_cursor_options: cursor,
                ..default()
            }),
            // Diagnostic plugins for logging and frame time diagnostics
            LogDiagnosticsPlugin::default(),
            FrameTimeDiagnosticsPlugin::default(),
            // Custom game plugins
            SystemsLogicPlugin, // Main game phase logic
            InputsPlugin,
            TouchInputPlugin, // Touch/mobile support
            DebugFunctionsPlugin,
        ))
        // Fixed timestep for physics calculations
        .insert_resource(Time::<Fixed>::from_hz(REFRESH_RATE_HZ))
        // Resource for generating random numbers
        .insert_resource(RandomGen::default())
        // Resource for the game state
        .insert_resource(GameState::default())
        .run();
}
