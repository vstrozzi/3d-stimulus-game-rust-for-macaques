use bevy::prelude::*;
use std::time::Duration;

/// Resources
#[derive(Resource)]
pub struct GameState {
    pub start_time: Duration,
    pub is_playing: bool,
    pub target_face_index: usize,
    pub attempts: u32,
}

/// Components
#[derive(Component)]
pub struct Pyramid;

#[derive(Component)]
pub struct FaceMarker {
    pub face_index: usize,
    pub color: Color,
    pub normal: Vec3,
}

// All the entities in the game that are cleared and respawn by setup
#[derive(Component)]
pub struct GameEntity;
