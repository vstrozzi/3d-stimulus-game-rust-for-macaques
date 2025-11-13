// This file defines the various objects, resources, and components used in the game.
use bevy::prelude::*;
use rand_chacha::rand_core::SeedableRng;
use std::time::Duration;

use crate::utils::constants::game_constants::SEED;
use rand_chacha::ChaCha8Rng;

/// An enum representing the different types of pyramids.
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

/// An enum representing the possible shapes for decorations on the pyramid faces.
#[derive(Clone, Copy)]
pub enum DecorationShape {
    Circle,
    Square,
    Star,
    Triangle,
}

/// A resource that holds the current state of the game.
#[derive(Resource, Clone, Default, Debug)]
pub struct GameState {
    // Game values
    // The seed used for random number generation.
    pub random_seed: u64,
    // The type of pyramid in the current game.
    pub pyramid_type: PyramidType,
    // The base radius of the pyramid.
    pub pyramid_base_radius: f32,
    // The height of the pyramid.
    pub pyramid_height: f32,
    // The index of the target face of the pyramid.
    pub pyramid_target_face_index: usize,
    // The starting orientation radius of the pyramid.
    pub pyramid_start_orientation_radius: f32,
    // The colors of the pyramid faces.
    pub pyramid_color_faces: [Color; 3],

    // Game state flags
    // A flag indicating whether the game is currently being played.
    pub is_playing: bool,
    // A flag indicating whether the game has started.
    pub is_started: bool,
    // A flag indicating whether the game has been won.
    pub is_won: bool,
    // A flag indicating whether the game state has changed.
    pub is_changed: bool,

    // Timing
    // The time when the game started.
    pub start_time: Option<Duration>,
    // The time when the game ended.
    pub end_time: Option<Duration>,

    // Metrics
    // The number of attempts the player has made.
    pub attempts: u32,
    // The cosine alignment of the camera with the target face when the player wins.
    pub cosine_alignment: Option<f32>,
}

/// A resource for random number generation.
#[derive(Resource)]
pub struct RandomGen {
    pub random_gen: ChaCha8Rng,
}

impl RandomGen {
    // Creates a new `RandomGen` from a given seed.
    pub fn from_seed(seed: u64) -> Self {
        Self {
            random_gen: ChaCha8Rng::seed_from_u64(seed),
        }
    }
}

impl Default for RandomGen {
    // Creates a new `RandomGen` with the default seed.
    fn default() -> Self {
        use rand_chacha::rand_core::SeedableRng;
        Self {
            random_gen: ChaCha8Rng::seed_from_u64(SEED),
        }
    }
}

/// A component that marks an entity as a pyramid.
#[derive(Component)]
pub struct Pyramid;

/// A component that marks an entity as a face of a pyramid.
#[derive(Component)]
pub struct FaceMarker {
    pub face_index: usize,
    pub color: Color,
    pub normal: Vec3,
}

/// A component that marks an entity as a game entity, which can be cleared during setup.
#[derive(Component)]
pub struct GameEntity;

/// A component that marks an entity as a UI entity.
#[derive(Component)]
pub struct UIEntity;
