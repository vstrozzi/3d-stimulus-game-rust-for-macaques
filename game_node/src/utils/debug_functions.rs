//! Debug functions.
use bevy::{prelude::*, window::*};

use crate::utils::objects::PhotodiodeMarker;

pub struct DebugFunctionsPlugin;

impl Plugin for DebugFunctionsPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_systems(Startup, setup_photodiode)
            .add_systems(Update, (
                toggle_vsync,
                visualize_lights,
                toggle_photodiode,
                flash_photodiode,
            ));
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

/// Spawn the photodiode square (hidden by default) in the top-right corner.
fn setup_photodiode(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Px(50.0),
            height: Val::Px(50.0),
            ..default()
        },
        BackgroundColor(Color::WHITE),
        GlobalZIndex(1001), // Above blank screen (GlobalZIndex 1000)
        Visibility::Hidden,
        PhotodiodeMarker,
    ));
}

/// Toggle photodiode square visibility with 'B'.
fn toggle_photodiode(
    input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut Visibility, With<PhotodiodeMarker>>,
) {
    if input.just_pressed(KeyCode::KeyB) {
        for mut vis in &mut query {
            *vis = match *vis {
                Visibility::Hidden => {
                    info!("Photodiode square: ON");
                    Visibility::Visible
                }
                _ => {
                    info!("Photodiode square: OFF");
                    Visibility::Hidden
                }
            };
        }
    }
}

/// Alternate the photodiode square between white and black every render frame.
/// A photodiode placed on this corner reads the exact render cadence.
fn flash_photodiode(
    mut query: Query<(&mut BackgroundColor, &Visibility), With<PhotodiodeMarker>>,
    mut toggle: Local<bool>,
) {
    *toggle = !*toggle;
    for (mut bg, vis) in &mut query {
        if *vis != Visibility::Hidden {
            bg.0 = if *toggle { Color::WHITE } else { Color::BLACK };
        }
    }
}
