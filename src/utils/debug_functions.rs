use bevy::{prelude::*, window::*};
pub struct DebugFunctionsPlugin;

impl Plugin for DebugFunctionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, toggle_vsync);
    }
}

fn toggle_vsync(
    input: Res<ButtonInput<KeyCode>>,
    mut window: Query<&mut Window>, //
) {
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
