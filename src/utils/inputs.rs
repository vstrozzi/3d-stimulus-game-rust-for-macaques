// This file contains the input handling for the game, specifically for toggling display and cursor modes.
use bevy::prelude::*;

use bevy::window::{
    CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, VideoModeSelection, WindowMode,
};

use std::sync::atomic::{AtomicUsize, Ordering};

/// A plugin for handling keyboard inputs.
pub struct InputsPlugin;

impl Plugin for InputsPlugin {
    /// Builds the plugin by adding the `handle_keyboard_input` system to the app.
    fn build(&self, app: &mut App) {
        app.add_systems(Update, crate::utils::inputs::handle_keyboard_input);
    }
}

/// An atomic index used to cycle through different display and cursor modes.
static DISPLAY_RING_IDX: AtomicUsize = AtomicUsize::new(0);

/// Toggles between windowed and fullscreen/locked cursor modes.
pub fn toggle_display_cursor_mode_ring(window: &mut Window, cursor: &mut CursorOptions) {
    // Compute the next index in a cycle of 2 (0, 1, 0, 1, ...).
    let next = (DISPLAY_RING_IDX.fetch_add(1, Ordering::SeqCst) + 1) % 2;
    DISPLAY_RING_IDX.store(next, Ordering::SeqCst);

    // Determine the window mode, cursor grab mode, and cursor visibility based on the next index.
    let (mode, grab, visible) = match next {
        1 => (WindowMode::Windowed, CursorGrabMode::None, true),
        0 => (
            WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
            CursorGrabMode::Locked,
            false,
        ),
        _ => unreachable!(),
    };

    // Apply the new window mode, but not on wasm.
    #[cfg(not(target_arch = "wasm32"))]
    {
        window.mode = mode;
    }

    // Apply the new cursor grab mode and visibility.
    cursor.grab_mode = grab;
    cursor.visible = visible;
}

/// Handles keyboard inputs, specifically the Escape key.
pub fn handle_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cursor: Query<&mut CursorOptions>,
) {
    // If the Escape key is pressed, toggle the display and cursor mode.
    if keyboard.just_pressed(KeyCode::Escape) {
        let mut window = windows.single_mut().unwrap();
        let mut cursor = cursor.single_mut().unwrap();
        println!("our window mode is {:?}", window.mode);
        toggle_display_cursor_mode_ring(&mut window, &mut cursor);
    }
}
