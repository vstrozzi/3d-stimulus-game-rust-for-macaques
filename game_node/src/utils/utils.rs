//! Cleanup and other utils functions
use bevy::prelude::*;
use bevy::mesh::{Indices, PrimitiveTopology};
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

/// Builds a `TriangleList` mesh from raw attribute vectors
/// Pass an empty `indices` vec to omit explicit indices (vertices used sequentially)
pub fn build_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, Default::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    if !indices.is_empty() {
        mesh.insert_indices(Indices::U32(indices));
    }
    mesh
}