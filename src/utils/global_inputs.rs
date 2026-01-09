//! Input handling for the game, specifically for toggling display and cursor modes.
use bevy::prelude::*;

use bevy::window::{
    CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, VideoModeSelection, WindowMode,
};

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct InputsPlugin;

impl Plugin for InputsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, crate::utils::global_inputs::handle_keyboard_input);
    }
}

/// Atomic index to cycle different display and cursor modes
static DISPLAY_RING_IDX: AtomicUsize = AtomicUsize::new(0);

/// Cycle between windowed and fullscreen/locked cursor modes (ESC)
pub fn toggle_display_cursor_mode_ring(window: &mut Window, cursor: &mut CursorOptions) {
    // Compute the next index in a cycle of 2 (0, 1, 0, 1, ...)
    let next = (DISPLAY_RING_IDX.fetch_add(1, Ordering::SeqCst) + 1) % 2;
    DISPLAY_RING_IDX.store(next, Ordering::SeqCst);

    let (mode, grab, visible) = match next {
        1 => (WindowMode::Windowed, CursorGrabMode::None, true),
        0 => (
            WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
            CursorGrabMode::Locked,
            false,
        ),
        _ => unreachable!(),
    };

    #[cfg(not(target_arch = "wasm32"))]
    {
        window.mode = mode;
    }

    cursor.grab_mode = grab;
    cursor.visible = visible;
}

/// Handles ESC key to toggle display and cursor modes
pub fn handle_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cursor: Query<&mut CursorOptions>,
) {

    if keyboard.just_pressed(KeyCode::Escape) {
        let mut window = windows.single_mut().unwrap();
        let mut cursor = cursor.single_mut().unwrap();
        println!("our window mode is {:?}", window.mode);
        toggle_display_cursor_mode_ring(&mut window, &mut cursor);
    }
}