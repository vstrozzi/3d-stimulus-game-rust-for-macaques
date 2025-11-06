use bevy::prelude::*;

use crate::log;
use crate::utils::objects::{FaceMarker, GameEntity, GameState, Pyramid};
use crate::utils::setup::setup;

/// Plugin for handling functions
pub struct GameFunctionsPlugin;

impl Plugin for GameFunctionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                crate::utils::game_functions::check_face_alignment,
                crate::utils::game_functions::game_ui,
            ),
        );
    }
}

/// Checking the winning condition
pub fn check_face_alignment(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut game_state: ResMut<GameState>,
    camera_query: Query<&Transform, With<Camera3d>>,
    face_query: Query<(&Transform, &FaceMarker), With<Pyramid>>,
) {
    // Only check if the game is active
    if !game_state.is_playing {
        return;
    }

    // Check for SPACE key press to check alignment
    if keyboard.just_pressed(KeyCode::Space) {
        game_state.attempts += 1;

        let Ok(camera_transform) = camera_query.single() else {
            return;
        };
        // Get camera direction
        let camera_forward = camera_transform.forward();

        // Check which face is most aligned with camera by getting the one with
        // the smallest dot product between camera dir and face dir
        // (i.e. face is facing camera)
        let mut best_alignment = 1.0;
        let mut best_face_index = None;

        for (face_transform, face_marker) in &face_query {
            // Get face normal in world space
            // The local normal is stored in `face_marker.normal`
            let face_normal = (face_transform.rotation * (face_marker.normal)).normalize();

            // Calculate alignment (dot product)
            // A perfect alignment is -1.0 (camera forward = -face normal)
            let alignment = face_normal.dot(*camera_forward);

            if alignment < best_alignment {
                best_alignment = alignment;
                best_face_index = Some(face_marker.face_index);
            }
        }
        log!(
            "🔍 Best aligned face: {:?} with alignment {:.3}",
            best_face_index,
            best_alignment
        );

        // Check if aligned enough (within margin)
        let alignment_cosine_threshold = -0.85;
        if let Some(best_face_index) = best_face_index {
            // Check if the cosine alignment is good enough
            if best_alignment < alignment_cosine_threshold {
                // Check if the face is the correct one
                if best_face_index == game_state.target_face_index {
                    // WIN!
                    game_state.is_playing = false;
                    let elapsed = time.elapsed() - game_state.start_time;

                    log!("🎉 CONGRATULATIONS! YOU WIN!");
                    log!("⏱️  Time taken: {:.2} seconds", elapsed.as_secs_f32());
                    log!("🎯 Attempts: {}", game_state.attempts);
                    log!(
                        "📊 Alignment accuracy: {:.1}%",
                        best_alignment.abs() * 100.0
                    );

                    if game_state.attempts == 1 {
                        log!("⭐ PERFECT! First try!");
                    }
                } else {
                    log!(
                        "❌ Wrong face! Keep trying... (Attempt {})",
                        game_state.attempts
                    );
                    log!("💡 Hint: Look for the RED face with the WHITE marker");
                }
            } else {
                log!(
                    "⚠️  Face not centered enough! Alignment: {:.1}%",
                    best_alignment.abs() * 100.0
                );
                log!(
                    "💡 Try to center it better (need {:.1}%+)",
                    alignment_cosine_threshold.abs() * 100.0
                );
            }
        }
    }
}

/// Game UI
pub fn game_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    entities: Query<Entity, With<GameEntity>>,
    query: Query<Entity, With<Text>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    // Clear old UI
    for entity in &query {
        commands.entity(entity).despawn();
    }
    // Reset the game on R key press and game over
    if !game_state.is_playing && keyboard.just_pressed(KeyCode::KeyR) {
        // Despawn all game entities
        for entity in entities.iter() {
            commands.entity(entity).despawn();
        }

        // Reset the game state
        setup(commands, meshes, materials, time);
    } else {
        let status_text: String;
        if game_state.is_playing {
            // Spawn instructions
            commands.spawn((
                Text::new("Arrow Keys/WASD: Rotate | SPACE: Check"),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.8, 0.8)),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(10.0),
                    left: Val::Px(10.0),
                    ..default()
                },
            ));
            // Status text
            status_text = format!("🎯 Find the RED face! | Attempts: {}", game_state.attempts);
        } else {
            status_text = "🎉 YOU WON! Refresh (R) to play again".to_string();
        }
        // Spawn text
        commands.spawn((
            Text::new(status_text),
            TextFont {
                font_size: 24.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(10.0),
                left: Val::Px(10.0),
                ..default()
            },
        ));
    }
}
