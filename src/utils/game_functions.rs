//! Core game and UI functions.
use bevy::prelude::*;

use crate::utils::constants::game_constants::{
    COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD, DOOR_ANIMATION_FADE_IN_DURATION,
    DOOR_ANIMATION_FADE_OUT_DURATION, DOOR_ANIMATION_STAY_OPEN_DURATION,
};
use crate::utils::objects::{
    BaseDoor, BaseFrame, GameEntity, GamePhase, GameState, HoleLight,
    UIEntity,
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
    let text = "Press SPACE to start the game! \nGame Commands: Arrow Keys/WASD: Rotate | SPACE: Check";
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera_query: Query<&Transform, With<Camera3d>>,
    mut door_query: Query<(Entity, &BaseDoor, &Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    light_query: Query<Entity, With<HoleLight>>,
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


        for (_, door, door_transform, _) in &door_query {
            // Get door normal in world space and move to camera door's actual rotation
            let door_normal_world = door_transform.rotation * door.normal;
            
            // Project to XZ plane
            let door_normal_xz = Vec3::new(door_normal_world.x, 0.0, door_normal_world.z).normalize();
            
            // Calculate alignment (dot product)
            let alignment = door_normal_xz.dot(camera_forward_xz);
            
            // Most negative = door facing toward camera
            if alignment > best_alignment {
                best_alignment = alignment;
                best_door_index = door.door_index;
            }
            
        }

        // Determine if the player wins
        let has_won = best_alignment > COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD && best_door_index == game_state.pyramid_target_door_index;

        // Set pending phase based on win condition
        if has_won {
            game_state.pending_phase = Some(GamePhase::Won); // Go to Won
            game_state.end_time = Some(time.elapsed());
            game_state.cosine_alignment = Some(best_alignment);
        } else {
            game_state.pending_phase = Some(GamePhase::Playing); // Continue playing if lost
        }

        // Start animation for the winning door
        let mut winning_door = None;

        // Ensure the door has a unique material so we only fade THIS door.
        for (door_entity, door, _, _) in &door_query {
            if door.door_index == game_state.pyramid_target_door_index {
                winning_door = Some(door_entity);
                break;
            }
        }

        if let Ok(( _, _, _, mut mat_handle)) = door_query.get_mut(winning_door.unwrap()) {
            if let Some(material) = materials.get(&mat_handle.0) {
                let mut new_material = material.clone();
                new_material.base_color.set_alpha(1.0); 
                mat_handle.0 = materials.add(new_material);
            }
        }

        // Find the corresponding light.
        let mut found_light = None;

        // Iterate frames to find the one with correct side index
        for (frame, children) in &frame_query {
            if frame.door_index == game_state.pyramid_target_door_index {
                // Check children for HoleLight
                for child in children {
                    if light_query.get(*child).is_ok() {
                        found_light = Some(*child);
                        break;
                    }
                }
            }
            if found_light.is_some() { break; }
        }

        if let Some(light_entity) = found_light {
            // Start Animation
            game_state.animating_door = Some(winning_door.unwrap());
            game_state.animating_light = Some(light_entity);
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
                    max_width: Val::Px(1200.0), // limit text width for wrapping
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ));
        });
}

/// Handles the door animation state machine
pub fn handle_door_animation(
    mut game_state: ResMut<GameState>,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut light_query: Query<&mut Visibility, With<HoleLight>>,
    door_query: Query<&MeshMaterial3d<StandardMaterial>, With<BaseDoor>>,
    mut next_state: ResMut<NextState<GamePhase>>,
) {
    // If not animating, exit
    if !game_state.is_animating {
        return;
    }

    let Some(start_time) = game_state.animation_start_time else { return; };
    let elapsed = (time.elapsed() - start_time).as_secs_f32();

    let door_entity = game_state.animating_door.unwrap();
    let light_entity = game_state.animating_light.unwrap();

    let fade_out_end = DOOR_ANIMATION_FADE_OUT_DURATION;
    let stay_open_end = fade_out_end + DOOR_ANIMATION_STAY_OPEN_DURATION;
    let fade_in_end = stay_open_end + DOOR_ANIMATION_FADE_IN_DURATION;

    // Get material handle
    let Ok(material_handle) = door_query.get(door_entity) else { return; };
    let Some(material) = materials.get_mut(material_handle) else { return; };

    // Get light visibility
    let Ok(mut light_visibility) = light_query.get_mut(light_entity) else { return; };


    if elapsed < fade_out_end {
        // Phase 1: Fade Out (Opening)
        *light_visibility = Visibility::Visible;
        let t = elapsed / DOOR_ANIMATION_FADE_OUT_DURATION;
        let alpha = t.clamp(0.0, 1.0);
        material.base_color.set_alpha(alpha);

    } else if elapsed < stay_open_end {
        // Phase 2: Stay Open
        *light_visibility = Visibility::Visible;
        material.base_color.set_alpha(1.0);

    } else if elapsed < fade_in_end {
        // Phase 3: Fade In (Closing)
        *light_visibility = Visibility::Visible;
        let t = (elapsed - stay_open_end) / DOOR_ANIMATION_FADE_IN_DURATION;
        let alpha = 1.0 -  t.clamp(0.0, 1.0);
        material.base_color.set_alpha(alpha);

    } else {
        // Animation Finished
        material.base_color.set_alpha(1.0);
        *light_visibility = Visibility::Hidden; // Turn off light

        game_state.is_animating = false;
        game_state.animating_door = None;
        game_state.animating_light = None;
        game_state.animation_start_time = None;

        // Transition to pending phase
        if let Some(pending) = game_state.pending_phase {
            next_state.set(pending);
        }
        game_state.pending_phase = None;
    }
}
