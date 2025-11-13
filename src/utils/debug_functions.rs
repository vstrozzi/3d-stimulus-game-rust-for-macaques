// This file contains debug functions for the game, such as toggling VSync.
use bevy::{prelude::*, window::*};

/// A plugin that provides debug functionalities for the game.
pub struct DebugFunctionsPlugin;

impl Plugin for DebugFunctionsPlugin {
    /// Builds the plugin by adding the `toggle_vsync` system to the app.
    fn build(&self, app: &mut App) {
        app.add_systems(Update, toggle_vsync);
    }
}

/// A system that toggles VSync when the 'V' key is pressed.
fn toggle_vsync(
    input: Res<ButtonInput<KeyCode>>,
    mut window: Query<&mut Window>,
) {
    if input.just_pressed(KeyCode::KeyV) {
        // Get the primary window.
        let mut window = window.single_mut().unwrap();

        // Toggle the present mode between AutoVsync and AutoNoVsync.
        window.present_mode = if matches!(window.present_mode, PresentMode::AutoVsync) {
            PresentMode::AutoNoVsync
        } else {
            PresentMode::AutoVsync
        };
        // Log the new present mode.
        info!("PRESENT_MODE: {:?}", window.present_mode);
    }
}
