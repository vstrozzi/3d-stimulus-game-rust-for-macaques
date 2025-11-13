// This file is responsible for setting up the Bevy application, including the window, plugins, and resources.
// Import necessary modules from the Bevy engine.
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::*,
};

// Import custom modules from the game.
use monkey_3d_game::utils::{
    camera::Camera3dFpovPlugin,
    constants::game_constants::REFRESH_RATE_HZ,
    debug_functions::DebugFunctionsPlugin,
    game_functions::GameFunctionsPlugin,
    inputs::InputsPlugin,
    objects::{GameState, RandomGen},
    setup::SetupPlugin,
};

/// The main function, which serves as the entry point for the application.
fn main() {
    // Configure the window for the game.
    let window = Some(Window {
        title: "Monkey 3D Game".into(),
        // Tells Wasm to resize the window according to the available canvas.
        fit_canvas_to_parent: true,
        // Tells Wasm not to override default event handling, like F5, Ctrl+R etc.
        prevent_default_event_handling: false,
        // Set fullscreen on default (not working on wasm).
        #[cfg(not(target_arch = "wasm32"))]
        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
        // Enable vsync.
        present_mode: PresentMode::AutoVsync,
        ..default()
    });

    // Configure the cursor for the game.
    let cursor = Some(CursorOptions {
        grab_mode: CursorGrabMode::Locked,
        visible: false,
        ..default()
    });

    // Create and run the Bevy application.
    App::new()
        .add_plugins((
            // Set up the default Bevy plugins with the custom window and cursor settings.
            DefaultPlugins.set(WindowPlugin {
                primary_window: window,
                primary_cursor_options: cursor,
                ..default()
            }),
            // Add diagnostic plugins for logging and frame time diagnostics.
            LogDiagnosticsPlugin::default(),
            FrameTimeDiagnosticsPlugin::default(),
            // Add custom game plugins.
            SetupPlugin,
            GameFunctionsPlugin,
            Camera3dFpovPlugin,
            InputsPlugin,
            DebugFunctionsPlugin,
        ))
        // Set a fixed timestep for physics calculations.
        .insert_resource(Time::<Fixed>::from_hz(REFRESH_RATE_HZ))
        // Add a resource for generating random numbers.
        .insert_resource(RandomGen::default())
        // Add a resource for the game state.
        .insert_resource(GameState::default())
        .run();
}
