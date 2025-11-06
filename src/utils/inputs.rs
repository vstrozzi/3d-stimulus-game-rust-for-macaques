use bevy::prelude::*;

use bevy::window::{
    CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, VideoModeSelection, WindowMode,
};

use std::sync::atomic::{AtomicUsize, Ordering};

/// Plugin for handling inputs
pub struct InputsPlugin;

impl Plugin for InputsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, crate::utils::inputs::handle_keyboard_input);
    }
}

/// Atomic index for cycling through display/cursor modes
static DISPLAY_RING_IDX: AtomicUsize = AtomicUsize::new(0);

pub fn toggle_display_cursor_mode_ring(window: &mut Window, cursor: &mut CursorOptions) {
    // Compute next index (0/1)
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

/// Handle keyboard inputs
pub fn handle_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cursor: Query<&mut CursorOptions>,
) {
    // If escape is pressed, cycle between release cursor and size of window
    if keyboard.just_pressed(KeyCode::Escape) {
        let mut window = windows.single_mut().unwrap();
        let mut cursor = cursor.single_mut().unwrap();
        println!("our window mode is {:?}", window.mode);
        toggle_display_cursor_mode_ring(&mut window, &mut cursor);
    }
}
