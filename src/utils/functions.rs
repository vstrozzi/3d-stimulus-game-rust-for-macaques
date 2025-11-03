use bevy::prelude::*;

use crate::utils::objects::{FaceMarker, GameState, Pyramid};

use crate::log;

/// Function for defining the winning situation
pub fn check_face_alignment(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut game_state: ResMut<GameState>,
    camera_query: Query<&Transform, With<Camera3d>>,
    // Query for faces, getting their FaceMarker component
    face_query: Query<(&Transform, &FaceMarker), (With<Pyramid>, Without<Camera3d>)>,
) {
    if !game_state.is_playing {
        return;
    }

    if keyboard.just_pressed(KeyCode::Space) {
        game_state.attempts += 1;

        let Ok(camera_transform) = camera_query.single() else {
            return;
        };
        // This is the direction the camera is pointing
        let camera_forward = camera_transform.forward();

        // Check which face is most aligned with camera
        let mut best_alignment = -1.0;
        let mut best_face_index = None;

        for (face_transform, face_marker) in &face_query {
            // Get face normal in world space
            // The local normal is stored in `face_marker.normal`
            let face_normal = face_transform.rotation * face_marker.normal;

            // Calculate alignment (dot product)
            // A perfect alignment is -1.0 (camera forward = -face normal)
            let alignment = face_normal.dot(*camera_forward);

            if alignment < best_alignment {
                best_alignment = alignment;
                best_face_index = Some(face_marker.face_index);
            }
        }

        // Check if aligned enough (within margin) and correct face
        let alignment_threshold = -0.85; // Needs to be very closely aligned

        if let Some(face_index) = best_face_index {
            if best_alignment < alignment_threshold {
                if face_index == game_state.target_face_index {
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
                    alignment_threshold.abs() * 100.0
                );
            }
        }
    }
}

/// Game UI
pub fn game_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    query: Query<Entity, With<Text>>,
) {
    // Clear old UI
    for entity in &query {
        commands.entity(entity).despawn();
    }

    let status_text = if game_state.is_playing {
        format!("🎯 Find the RED face! | Attempts: {}", game_state.attempts)
    } else {
        "🎉 YOU WON! Refresh to play again".to_string()
    };

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

    // Instructions
    if game_state.is_playing {
        commands.spawn((
            // Updated text to remove Q/E
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
    }
}
