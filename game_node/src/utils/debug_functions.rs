//! Debug functions for the game.
use bevy::{prelude::*, window::*};

pub struct DebugFunctionsPlugin;

impl Plugin for DebugFunctionsPlugin {
    /// Pplugin by adding the `toggle_vsync` system to the app.
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (toggle_vsync, visualize_lights));
    }
}

/// Toggles VSync when the 'V' key is pressed.
fn toggle_vsync(input: Res<ButtonInput<KeyCode>>, mut window: Query<&mut Window>) {
    if input.just_pressed(KeyCode::KeyV) {
        let mut window = window.single_mut().unwrap();

        window.present_mode = if matches!(window.present_mode, PresentMode::AutoVsync) {
            PresentMode::AutoNoVsync
        } else {
            PresentMode::AutoVsync
        };

        info!("PRESENT_MODE: {:?}", window.present_mode);
    }
}

/// Visualizes lights when the 'L' key is pressed.
fn visualize_lights(
    mut gizmos: Gizmos,
    query: Query<(&GlobalTransform, &SpotLight)>,
    input: Res<ButtonInput<KeyCode>>,
    mut show_lights: Local<bool>,
) {
    if input.just_pressed(KeyCode::KeyL) {
        *show_lights = !*show_lights;
        info!("Light visualization: {}", *show_lights);
    }

    if *show_lights {
        for (transform, light) in &query {
            // Draw a sphere representing the light's range
            gizmos.sphere(transform.translation(), light.range, light.color);
            // Draw a smaller sphere representing the light source itself
            gizmos.sphere(transform.translation(), 0.2, Color::WHITE);
        }
    }
}
