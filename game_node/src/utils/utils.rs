//! Cleanup and other utils functions
use bevy::prelude::*;
use crate::utils::objects::{GameEntity, UIEntity, BlankScreen};

/// Despawn all game and UI entities
pub fn despawn_all_game_and_ui(
    mut commands: Commands,
    game_query: Query<Entity, With<GameEntity>>,
    ui_query: Query<Entity, With<UIEntity>>,
) {
    for entity in &game_query {
        commands.entity(entity).try_despawn();
    }
    for entity in &ui_query {
        commands.entity(entity).try_despawn();
    }
}

/// Helper to despawn ui entities given a mutable commands reference
pub fn despawn_ui(commands: &mut Commands, query: &Query<Entity, With<UIEntity>>) {
    for entity in query {
        commands.entity(entity).despawn();
    }
}

/// Helper function to spawn a fullscreen black overlay
pub fn spawn_blank_screen(commands: &mut Commands) {
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(1000), // In front
        BlankScreen,
    ));
}
