// This file contains the core game logic and UI functions.
use bevy::prelude::*;

use crate::utils::constants::game_constants::COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD;
use crate::utils::objects::{FaceMarker, GameEntity, GameState, Pyramid, RandomGen, UIEntity};
use crate::utils::setup::setup;

/// A plugin for handling game functions, including checking for face alignment and managing the game UI.
pub struct GameFunctionsPlugin;

impl Plugin for GameFunctionsPlugin {
    /// Builds the plugin by adding the `check_face_alignment` and `game_ui` systems to the app.
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            ((
                crate::utils::game_functions::check_face_alignment,
                crate::utils::game_functions::game_ui,
            )
                .chain(),),
        );
    }
}

/// Spawns a black screen that covers the entire viewport.
pub fn spawn_black_screen(commands: &mut Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::BLACK),
        UIEntity, // Marker for despawning
    ));
}

/// Spawns centered text on a black screen.
pub fn spawn_centered_text_black_screen(commands: &mut Commands, text: &str) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center, // horizontally center children
                align_items: AlignItems::Center,         // vertically center children
                ..default()
            },
            UIEntity,                                    // Marker for despawning
            BackgroundColor(Color::srgb(0.0, 0.0, 0.0)), // transparent container
        ))
        .with_children(|parent| {
            // Spawn the text child
            parent.spawn((
                Text::new(text),
                TextFont {
                    font_size: 32.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 1.0, 1.0)),
                Node {
                    max_width: Val::Px(1200.0), // limit text width for wrapping
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ));
        });
}

/// Checks if the player has won the game by aligning the camera with the correct face of the pyramid.
pub fn check_face_alignment(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut game_state: ResMut<GameState>,
    camera_query: Query<&Transform, With<Camera3d>>,
    face_query: Query<(&Transform, &FaceMarker), With<Pyramid>>,
) {
    // Only check if the game is active
    if !game_state.is_playing || !game_state.is_started {
        return;
    }
    // Check for SPACE key press to check alignment
    if keyboard.just_pressed(KeyCode::Space) {
        game_state.attempts += 1;

        let Ok(camera_transform) = camera_query.single() else {
            return;
        };
        // Get camera direction
        let camera_forward = camera_transform.local_z();

        // Check which face is most aligned with camera by getting the one with
        // the smallest dot product between camera dir and face dir towards origin
        // (i.e. face is facing camera)
        let mut best_alignment = 1.0;
        let mut best_face_index = None;

        for (face_transform, face_marker) in &face_query {
            // Get face normal in world space
            // The local normal is stored in `face_marker.normal`
            let face_normal = (face_transform.rotation * (face_marker.normal)).normalize();

            // Project down to XZ plane
            let face_normal_xz = Vec3::new(face_normal.x, 0.0, face_normal.z).normalize();
            // Calculate alignment (dot product) of camera direction and face normal
            let alignment = face_normal_xz.dot(*camera_forward);

            if alignment < best_alignment {
                best_alignment = alignment;
                best_face_index = Some(face_marker.face_index);
            }
        }

        // Check if aligned enough (within margin)
        if let Some(best_face_index) = best_face_index {
            // Check if the cosine alignment is good enough
            if best_alignment < COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD {
                // Check if the face is the correct one
                if best_face_index == game_state.pyramid_target_face_index {
                    // Stop playing the game and record data
                    game_state.is_playing = false;
                    game_state.end_time = Some(time.elapsed());
                    game_state.cosine_alignment = Some(best_alignment);
                }
            }
        }
    }
}

/// Manages the game's UI based on the current game state.
pub fn game_ui(
    mut commands: Commands,
    mut game_state: ResMut<GameState>,
    entities: Query<Entity, With<GameEntity>>,
    query: Query<Entity, With<UIEntity>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    random_gen: ResMut<RandomGen>,
    time: Res<Time>,
) {
    // Check if the game state has changed from last frame before doing anything
    if !game_state.is_changed {
        return;
    }
    game_state.is_changed = false;

    // Clear all texts entities
    for entity in &query {
        commands.entity(entity).despawn();
    }

    // State Machine Logic
    // If the game has not started and the SPACE key is pressed, start the game.
    if !game_state.is_started && keyboard.just_pressed(KeyCode::Space) {
        // Start the game
        game_state.is_started = true;
        game_state.is_changed = true;
        game_state.is_playing = true;
        game_state.start_time = Some(time.elapsed());
        game_state.attempts = 0;
    }
    // If the game has not started, display the start screen.
    else if !game_state.is_started {
        // Spawn text centered in the screen
        let text = "Press SPACE to start the game! \nGame Commands: Arrow Keys/WASD: Rotate | SPACE: Check";
        spawn_centered_text_black_screen(&mut commands, text);
        // The game state has changed
        game_state.is_changed = true;
    }
    // If the game is over and the 'R' key is pressed, restart the game.
    else if !game_state.is_playing && keyboard.just_pressed(KeyCode::KeyR) {
        // Despawn all game entities
        for entity in entities.iter() {
            commands.entity(entity).despawn();
        }
        // Spawn black screen
        spawn_black_screen(&mut commands);

        // Reset the game state
        setup(commands, meshes, materials, random_gen, time);
    }
    // If the game is over and the player has won, display the win screen.
    else if !game_state.is_playing {
        let elapsed = game_state.end_time.unwrap().as_secs_f32()
            - game_state.start_time.unwrap().as_secs_f32();
        let accuracy = game_state.cosine_alignment.unwrap() * 100.0;

        // Win text
        let mut text = format!(
            "Refresh (R) to play again\n\n\
            CONGRATULATIONS! YOU WIN!\n\
            - Time taken: {:.5} seconds\n\
            - Attempts: {}\n\
            - Alignment accuracy: {:.1}%",
            elapsed, game_state.attempts, accuracy
        );

        if game_state.attempts == 1 {
            text.push_str("\nPERFECT! First try!");
        }

        // Spawn text centered in the screen
        spawn_centered_text_black_screen(&mut commands, &text);
        // The game state has changed
        game_state.is_changed = true;
    }
    // If the game is ongoing, display the game UI.
    else {
        let text = format!(
            "Arrow Keys/WASD: Rotate | SPACE: Check \nFind the RED face! | Attempts: {}",
            game_state.attempts
        );
        // Spawn text
        commands.spawn((
            Text::new(text),
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
            UIEntity, // Marker for despawning
        ));
        // The game state has changed
        game_state.is_changed = true;
    }
}
