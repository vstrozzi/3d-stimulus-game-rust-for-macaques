use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::*,
};

use monkey_3d_game::utils::{
    camera::Camera3dFpovPlugin, constants::game_constants::REFRESH_RATE_HZ,
    debug_functions::DebugFunctionsPlugin, game_functions::GameFunctionsPlugin,
    inputs::InputsPlugin, objects::GameState, setup::SetupPlugin,
};

/// Main application function
fn main() {
    // Window configuration
    let window = Some(Window {
        title: "Monkey 3D Game".into(),
        // Tells Wasm to resize the window according to the available canvas
        fit_canvas_to_parent: true,
        // Tells Wasm not to override default event handling, like F5, Ctrl+R etc.
        prevent_default_event_handling: false,
        // Set fullscreen on default (not working on wasm)
        #[cfg(not(target_arch = "wasm32"))]
        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
        // Enable vsync
        present_mode: PresentMode::AutoVsync,
        ..default()
    });

    // Mouse configuration
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
            // DEBUG PLUGINS
            LogDiagnosticsPlugin::default(),
            FrameTimeDiagnosticsPlugin::default(),
            // My Plugin
            SetupPlugin,
            GameFunctionsPlugin,
            Camera3dFpovPlugin,
            InputsPlugin,
            DebugFunctionsPlugin,
        ))
        // Timer for physics (fixed timestep timer)
        .insert_resource(Time::<Fixed>::from_hz(REFRESH_RATE_HZ))
        // Insert source of randomness
        .insert_resource(RandomState::default())
        // Insert Gamestate
        .insert_resource(GameState::default())
        .run();
}
