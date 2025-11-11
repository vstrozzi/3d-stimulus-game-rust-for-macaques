use bevy::prelude::*;
use std::time::Duration;

use rand_chacha::ChaCha8Rng;
/// Pyramid types

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PyramidType {
    Type1,
    Type2,
}

impl Default for PyramidType {
    fn default() -> Self {
        PyramidType::Type1
    }
}

/// Possible decoration shapes
#[derive(Clone, Copy)]
pub enum DecorationShape {
    Circle,
    Square,
    Star,
    Triangle,
}

/// Resources
#[derive(Resource, Clone, Default, Debug)]
pub struct GameState {
    // Game values
    pub random_seed: u64,
    pub random_gen: Option<ChaCha8Rng>,
    pub pyramid_type: PyramidType,
    pub pyramid_base_radius: f32,
    pub pyramid_height: f32,
    pub pyramid_target_face_index: usize,
    pub pyramid_start_orientation_radius: f32,
    pub pyramid_color_faces: [Color; 3],

    // Game state flags
    pub is_playing: bool,
    pub is_started: bool,
    pub is_won: bool,
    pub is_changed: bool,

    // Timing
    pub start_time: Option<Duration>,
    pub end_time: Option<Duration>,


    // Metrics
    pub attempts: u32,
    pub cosine_alignment: Option<f32>,
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

// All the entities in the game that are spawned and cleared by setup
#[derive(Component)]
pub struct GameEntity;

// All the UI text/nodes
#[derive(Component)]
pub struct UIEntity;