//! Core game and UI functions.
use bevy::prelude::*;

use crate::utils::constants::game_constants::{
    COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD, DOOR_ANIMATION_FADE_IN_DURATION,
    DOOR_ANIMATION_FADE_OUT_DURATION, DOOR_ANIMATION_STAY_OPEN_DURATION,
    SCORE_BAR_BORDER_THICKNESS, SCORE_BAR_HEIGHT, SCORE_BAR_TOP_OFFSET, SCORE_BAR_WIDTH_PERCENT,
};
use crate::utils::objects::{
    BaseDoor, BaseFrame, GameEntity, GamePhase, GameState, HoleEmissive, HoleLight, ScoreBarFill,
    ScoreBarUI, UIEntity,
};

/// Helper to despawn ui entities given a mutable commands reference
pub fn despawn_ui_helper(commands: &mut Commands, query: &Query<Entity, With<UIEntity>>) {
    for entity in query {
        commands.entity(entity).despawn();
    }
}

/// Helper system to cleanup UI entities (System Wrapper)
pub fn despawn_ui(mut commands: Commands, query: Query<Entity, With<UIEntity>>) {
    despawn_ui_helper(&mut commands, &query);
}

/// Helper system to cleanup Game entities
pub fn cleanup_game_entities(mut commands: Commands, query: Query<Entity, With<GameEntity>>) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

/// Setup UI for MenuUI state
pub fn setup_intro_ui(mut commands: Commands) {
    commands.spawn((Camera2d::default(), UIEntity));
    let text =
        "Press SPACE to start the game! \nGame Commands: Arrow Keys/WASD: Rotate | SPACE: Check";
    spawn_centered_text_black_screen(&mut commands, text);
}

/// Input handling for Menu Phase
pub fn menu_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GamePhase>>,
    mut game_state: ResMut<GameState>,
    time: Res<Time>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        game_state.start_time = Some(time.elapsed());
        game_state.nr_attempts = 0;
        next_state.set(GamePhase::Playing);
    }
}

/// Input handling for Playing Phase
pub fn playing_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    time: Res<Time>,
    // Queries needed for Playing logic
    camera_query: Query<&Transform, With<Camera3d>>,
    door_query: Query<(
        Entity,
        &BaseDoor,
        &Transform,
    )>,
    light_query: Query<Entity, With<HoleLight>>,
    emissive_query: Query<Entity, With<HoleEmissive>>,
    frame_query: Query<(&BaseFrame, &Children)>,
    mut commands: Commands,
    query: Query<Entity, With<UIEntity>>,
) {
    if game_state.is_animating {
        return; // Do not allow camera inputs while animating
    }
    // Check for SPACE key press to check alignment
    if keyboard.just_pressed(KeyCode::Space) {
        game_state.nr_attempts += 1;
        game_state.is_animating = true; // Ensure not animating
        // Clean old ui using helper
        despawn_ui_helper(&mut commands, &query);
        // Spawn new ui using helper
        spawn_playing_hud(&mut commands, &game_state);

        let Ok(camera_transform) = camera_query.single() else {
            return;
        };

        // Get local camera direction
        let camera_forward = camera_transform.forward();

        // Project camera forward to XZ plane
        let camera_forward_xz = Vec3::new(camera_forward.x, 0.0, camera_forward.z).normalize();

        let mut best_alignment = -1.0;
        let mut best_door_index = 0;
        let mut winning_door_alignment = -1.0;

        for (_, door, door_transform) in &door_query {
            // Get door normal in world space and move to camera door's actual rotation
            let door_normal_world = door_transform.rotation * door.normal;

            // Project to XZ plane
            let door_normal_xz =
                Vec3::new(door_normal_world.x, 0.0, door_normal_world.z).normalize();

            // Calculate alignment (dot product)
            let alignment = door_normal_xz.dot(camera_forward_xz);

            // Most negative = door facing toward camera
            if alignment > best_alignment {
                best_alignment = alignment;
                best_door_index = door.door_index;
            }
            
            // Save the real door_alignment
            if door.door_index == game_state.pyramid_target_door_index {
                winning_door_alignment = alignment;
            }
        }

        // Determine if the player wins
        let has_won = best_alignment > COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD
            && best_door_index == game_state.pyramid_target_door_index;

        // Always store the alignment for the score bar animation
        game_state.cosine_alignment = Some(winning_door_alignment);

        // Set pending phase based on win condition
        if has_won {
            game_state.pending_phase = Some(GamePhase::Won); // Go to Won
            game_state.end_time = Some(time.elapsed());
        } else {
            game_state.pending_phase = Some(GamePhase::Playing); // Continue playing if lost
        }

        // Start animation for the winning door
        let mut winning_door = None;

        // Ensure the door has a unique material so we only fade THIS door.
        for (door_entity, door, _) in &door_query {
            if door.door_index == game_state.pyramid_target_door_index {
                winning_door = Some(door_entity);
                break;
            }
        }


        // Find the corresponding light and emissive.
        let mut found_light = None;
        let mut found_emissive = None;

        // Iterate frames to find the one with correct side index
        for (frame, children) in &frame_query {
            if frame.door_index == game_state.pyramid_target_door_index {
                // Check children for HoleLight and HoleEmissive
                for child in children {
                    if light_query.get(*child).is_ok() {
                        found_light = Some(*child);
                    }
                    if emissive_query.get(*child).is_ok() {
                        found_emissive = Some(*child);
                    }
                }
            }
            if found_light.is_some() && found_emissive.is_some() {
                break;
            }
        }

        if let Some(light_entity) = found_light {
            // Start Animation
            game_state.animating_door = Some(winning_door.unwrap());
            game_state.animating_light = Some(light_entity);
            game_state.animating_emissive = found_emissive;
            game_state.animation_start_time = Some(time.elapsed());
        }
    }
}

/// Input handling for Won Phase
pub fn won_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GamePhase>>,
) {
    if keyboard.just_pressed(KeyCode::KeyR) {
        next_state.set(GamePhase::MenuUI);
    }
}

/// Setup UI for Playing state
pub fn setup_playing_ui(mut commands: Commands, game_state: Res<GameState>) {
    spawn_playing_hud(&mut commands, &game_state);
}

pub fn spawn_playing_hud(commands: &mut Commands, game_state: &GameState) {
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

    // Spawn the score bar
    spawn_score_bar(commands);
}

/// Spawns the energy score bar at the top center of the screen
pub fn spawn_score_bar(commands: &mut Commands) {
    // Container for the score bar (centered at top)
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                top: Val::Px(SCORE_BAR_TOP_OFFSET),
                justify_content: JustifyContent::Center,
                ..default()
            },
            UIEntity,
        ))
        .with_children(|parent| {
            // Outer border/background of the bar
            parent
                .spawn((
                    Node {
                        width: Val::Percent(SCORE_BAR_WIDTH_PERCENT),
                        height: Val::Px(SCORE_BAR_HEIGHT),
                        border: UiRect::all(Val::Px(SCORE_BAR_BORDER_THICKNESS)),
                        padding: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.5)), // Dark subtle background
                    ScoreBarUI,
                ))
                .with_children(|bar_parent| {
                    // Inner fill bar (starts empty)
                    bar_parent.spawn((
                        Node {
                            width: Val::Percent(0.0), // Starts empty
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.2, 0.6, 1.0, 0.3)), // Dim cyan glow when empty
                        ScoreBarFill,
                    ));
                });
        });
}

/// Setup UI for Won state
pub fn setup_won_ui(mut commands: Commands, game_state: Res<GameState>) {
    commands.spawn((Camera2d::default(), UIEntity));
    // Display win screen
    let elapsed = game_state.end_time.unwrap_or_default().as_secs_f32()
        - game_state.start_time.unwrap_or_default().as_secs_f32();
    let accuracy = game_state.cosine_alignment.unwrap_or(0.0) * 100.0;

    let mut text = format!(
        "Refresh (R) to play again\n\n        CONGRATULATIONS! YOU WIN!\n        - Time taken: {:.5} seconds\n        - Attempts: {}\n        - Alignment accuracy: {:.1}%",
        elapsed, game_state.nr_attempts, accuracy
    );

    if game_state.nr_attempts == 1 {
        text.push_str("\nPERFECT! First try!");
    }

    spawn_centered_text_black_screen(&mut commands, &text);
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
                    max_width: Val::Percent(90.0), // limit text width for wrapping (responsive)
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ));
        });
}

/// Handles the light animation
pub fn handle_door_animation(
    mut game_state: ResMut<GameState>,
    time: Res<Time>,
    mut light_query: Query<(&mut Visibility, &mut SpotLight), With<HoleLight>>,
    mut emissive_query: Query<(&mut Visibility, &MeshMaterial3d<StandardMaterial>), (With<HoleEmissive>, Without<HoleLight>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut next_state: ResMut<NextState<GamePhase>>,
) {
    // If not animating, exit
    if !game_state.is_animating {
        return;
    }

    let Some(start_time) = game_state.animation_start_time else {
        return;
    };
    let elapsed = (time.elapsed() - start_time).as_secs_f32();

    let light_entity = game_state.animating_light.unwrap();

    let fade_out_end = DOOR_ANIMATION_FADE_OUT_DURATION;
    let stay_open_end = fade_out_end + DOOR_ANIMATION_STAY_OPEN_DURATION;
    let fade_in_end = stay_open_end + DOOR_ANIMATION_FADE_IN_DURATION;

    // Get light visibility and component
    let Ok((mut light_visibility, mut spotlight)) = light_query.get_mut(light_entity) else {
        return;
    };

    // Calculate animation intensity (0.0 to 1.0)
    let intensity_factor = if elapsed < fade_out_end {
        // Phase 1: Fade Out (Opening) - 0.0 to 1.0
        elapsed / fade_out_end
    } else if elapsed < stay_open_end {
        // Phase 2: Stay Open - 1.0
        1.0
    } else if elapsed < fade_in_end {
        // Phase 3: Fade In (Closing) - 1.0 to 0.0
        1.0 - ((elapsed - stay_open_end) / DOOR_ANIMATION_FADE_IN_DURATION)
    } else {
        // Animation finished
        0.0
    };

    // Max intensity values
    const MAX_SPOTLIGHT_INTENSITY: f32 = 2_000_000.0;
    const MAX_EMISSIVE_INTENSITY: f32 = 100.0;

    if elapsed < fade_in_end {
        // Animation in progress
        *light_visibility = Visibility::Visible;
        spotlight.intensity = MAX_SPOTLIGHT_INTENSITY * intensity_factor;

        // Update emissive material if available
        if let Some(emissive_entity) = game_state.animating_emissive {
            if let Ok((mut emissive_visibility, material_handle)) = emissive_query.get_mut(emissive_entity) {
                *emissive_visibility = Visibility::Visible;
                
                if let Some(material) = materials.get_mut(&material_handle.0) {
                    // Use spotlight color for emissive
                    let light_color = spotlight.color.to_linear();
                    material.emissive = LinearRgba::new(
                        light_color.red * MAX_EMISSIVE_INTENSITY * intensity_factor,
                        light_color.green * MAX_EMISSIVE_INTENSITY * intensity_factor,
                        light_color.blue * MAX_EMISSIVE_INTENSITY * intensity_factor,
                        1.0,
                    );
                }
            }
        }
    } else {
        // Animation Finished
        *light_visibility = Visibility::Hidden;
        spotlight.intensity = MAX_SPOTLIGHT_INTENSITY; // Reset to default

        // Hide and reset emissive
        if let Some(emissive_entity) = game_state.animating_emissive {
            if let Ok((mut emissive_visibility, material_handle)) = emissive_query.get_mut(emissive_entity) {
                *emissive_visibility = Visibility::Hidden;
                
                if let Some(material) = materials.get_mut(&material_handle.0) {
                    material.emissive = LinearRgba::new(0.0, 0.0, 0.0, 0.0);
                }
            }
        }

        game_state.is_animating = false;
        game_state.animating_door = None;
        game_state.animating_light = None;
        game_state.animating_emissive = None;
        game_state.animation_start_time = None;

        // Transition to pending phase
        if let Some(pending) = game_state.pending_phase {
            next_state.set(pending);
        }
        game_state.pending_phase = None;
    }
}

/// Updates the score bar fill and color during the door animation
pub fn update_score_bar_animation(
    game_state: Res<GameState>,
    time: Res<Time>,
    mut fill_query: Query<(&mut Node, &mut BackgroundColor), With<ScoreBarFill>>,
) {
    let Ok((mut node, mut bg_color)) = fill_query.single_mut() else {
        return;
    };

    if !game_state.is_animating {
        // Not animating - show empty/dim state
        node.width = Val::Percent(0.0);
        *bg_color = BackgroundColor(Color::srgba(0.2, 0.6, 1.0, 0.3)); // Dim cyan
        return;
    }

    // Get animation progress
    let Some(start_time) = game_state.animation_start_time else {
        return;
    };
    let elapsed = (time.elapsed() - start_time).as_secs_f32();

    let total_duration = DOOR_ANIMATION_FADE_OUT_DURATION
        + DOOR_ANIMATION_STAY_OPEN_DURATION
        + DOOR_ANIMATION_FADE_IN_DURATION;

    // Calculate fill progress (0.0 to 1.0)
    let fill_progress = (elapsed / total_duration).clamp(0.0, 1.0);

    // Get alignment score (normalized to 0.0 - 1.0 range from -1.0 - 1.0)
    let alignment_normalized = if let Some(alignment) = game_state.cosine_alignment {
        ((alignment + 1.0) / 2.0).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Fill width based on both animation progress and alignment score
    // The bar fills up to the alignment level during the animation
    let target_width = alignment_normalized * 100.0;
    let current_width = fill_progress * target_width;
    node.width = Val::Percent(current_width);

    // Color gradient based on alignment quality (cyan -> yellow -> white)
    // Low alignment (0.0-0.5): cyan to yellow
    // High alignment (0.5-1.0): yellow to bright white
    let color = if alignment_normalized < 0.5 {
        let t = alignment_normalized * 2.0; // 0.0 to 1.0 for first half
        Color::srgba(
            0.2 + t * 0.8, // R: 0.2 -> 1.0
            0.6 + t * 0.4, // G: 0.6 -> 1.0
            1.0 - t * 0.2, // B: 1.0 -> 0.8
            0.7 + t * 0.2, // A: 0.7 -> 0.9
        )
    } else {
        let t = (alignment_normalized - 0.5) * 2.0; // 0.0 to 1.0 for second half
        Color::srgba(
            1.0,               // R: stays at 1.0
            1.0,               // G: stays at 1.0
            0.8 + t * 0.2,     // B: 0.8 -> 1.0 (yellow to white)
            0.9 + t * 0.1,     // A: 0.9 -> 1.0
        )
    };

    *bg_color = BackgroundColor(color);
}

/// Updates UI scale based on window size for responsive design
/// Targets 1080p (1920x1080) as the reference resolution
pub fn update_ui_scale(
    mut ui_scale: ResMut<UiScale>,
    window_query: Query<&Window>,
) {
    let Ok(window) = window_query.single() else {
        return;
    };

    // Reference height for UI design (1080p)
    const REFERENCE_HEIGHT: f32 = 1080.0;
    
    // Calculate scale based on window height
    let scale = window.height() / REFERENCE_HEIGHT;
    
    // Clamp scale to reasonable bounds (0.5x to 2.0x)
    let clamped_scale = scale.clamp(0.5, 2.0);
    
    ui_scale.0 = clamped_scale;
}
