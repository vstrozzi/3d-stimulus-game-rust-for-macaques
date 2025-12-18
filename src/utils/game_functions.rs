//! Core game logic and UI functions.
use bevy::prelude::*;

use crate::utils::constants::game_constants::COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD;
use crate::utils::objects::{
    FaceMarker, GameEntity, GamePhase, GameState, Pyramid, RandomGen, UIEntity,
};
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
    // Only check if the game is in Playing state
    if game_state.phase != GamePhase::Playing {
        return;
    }
    // Check for SPACE key press to check alignment
    if keyboard.just_pressed(KeyCode::Space) {
        game_state.nr_attempts += 1;

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
                    // Transition to Won state
                    game_state.phase = GamePhase::Won;
                    game_state.end_time = Some(time.elapsed());
                    game_state.cosine_alignment = Some(best_alignment);
                    game_state.is_changed = true;
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

    // State Machine using match pattern
    match game_state.phase {
        GamePhase::NotStarted => {
            if keyboard.just_pressed(KeyCode::Space) {
                // Transition: NotStarted -> Playing
                game_state.phase = GamePhase::Playing;
                game_state.start_time = Some(time.elapsed());
                game_state.nr_attempts = 0;
                game_state.is_changed = true;
                
            } else {
                // Display start screen
                let text = "Press SPACE to start the game! \nGame Commands: Arrow Keys/WASD: Rotate | SPACE: Check";
                spawn_centered_text_black_screen(&mut commands, text);
                game_state.is_changed = true;
            }
        }

        GamePhase::Playing => {
            // Display game UI
            let text = format!(
                "Arrow Keys/WASD: Rotate | SPACE: Check \nFind the RED face! | Attempts: {}",
                game_state.nr_attempts
            );
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
                UIEntity,
            ));
            game_state.is_changed = true;
        }

        GamePhase::Won => {
            if keyboard.just_pressed(KeyCode::KeyR) {
                // Transition: Won -> NotStarted (restart)
                for entity in entities.iter() {
                    commands.entity(entity).despawn();
                }
                spawn_black_screen(&mut commands);
                setup(commands, meshes, materials, random_gen, time);
            } else {
                // Display win screen
                let elapsed = game_state.end_time.unwrap().as_secs_f32()
                    - game_state.start_time.unwrap().as_secs_f32();
                let accuracy = game_state.cosine_alignment.unwrap() * 100.0;

                let mut text = format!(
                    "Refresh (R) to play again\n\n\
                    CONGRATULATIONS! YOU WIN!\n\
                    - Time taken: {:.5} seconds\n\
                    - Attempts: {}\n\
                    - Alignment accuracy: {:.1}%",
                    elapsed, game_state.nr_attempts, accuracy
                );

                if game_state.nr_attempts == 1 {
                    text.push_str("\nPERFECT! First try!");
                }

                spawn_centered_text_black_screen(&mut commands, &text);
                game_state.is_changed = true;
            }
        }
    }
}
