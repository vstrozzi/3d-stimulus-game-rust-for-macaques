//! This file defines the various objects, resources, and components used in the game.
use bevy::prelude::*;
use rand_chacha::rand_core::SeedableRng;
use std::time::Duration;

use crate::utils::constants::game_constants::SEED;
use rand_chacha::ChaCha8Rng;


/// Game state enum representing the different states the game can be in
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GamePhase {
    #[default]
    // The game has not started yet
    NotStarted,
    // The game is currently being played
    Playing,
    // The game has been won
    Won,
}


/// Different types of pyramids
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

/// Shapes for decorations on the pyramid faces
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecorationShape {
    Circle,
    Square,
    Star,
    Triangle,
}

/// Single decoration on a pyramid face with barycentric coordinates relative to the triangle vertices (top, corner1, corner2)
#[derive(Clone, Debug)]
pub struct Decoration {
    /// Barycentric coordinates (w0, w1, w2) where:
    /// - w0 = weight for top vertex
    /// - w1 = weight for corner1 vertex
    /// - w2 = weight for corner2 vertex
    /// Position can be reconstructed as: position = w0*top + w1*corner1 + w2*corner2
    pub barycentric: Vec3,

    pub size: f32,
}

/// Set of decorations for a pyramid face, which all share same shape and color
#[derive(Clone, Debug)]
pub struct DecorationSet {
    pub shape: DecorationShape,
    pub color: Color,
    pub decorations: Vec<Decoration>,
}


/// The resource of the current state of the game
#[derive(Resource, Clone, Default, Debug)]
pub struct GameState {
    pub phase: GamePhase,

    pub random_seed: u64,

    pub pyramid_type: PyramidType,
    pub pyramid_base_radius: f32,
    pub pyramid_height: f32,
    // The index of the target face of the pyramid.
    pub pyramid_target_face_index: usize,
    pub pyramid_start_orientation_rad: f32,
    pub pyramid_color_faces: [Color; 3],

    /// A flag indicating whether the game state has changed.
    pub is_changed: bool,

    // The time when the game started.
    pub start_time: Option<Duration>,
    // The time when the game ended.
    pub end_time: Option<Duration>,

    // Metrics
    // The number of attempts the player has made.
    pub nr_attempts: u32,
    // The cosine alignment of the camera with the target face when the player wins.
    pub cosine_alignment: Option<f32>,
}

/// Random number generator
#[derive(Resource)]
pub struct RandomGen {
    pub random_gen: ChaCha8Rng,
}

impl RandomGen {
    pub fn from_seed(seed: u64) -> Self {
        Self {
            random_gen: ChaCha8Rng::seed_from_u64(seed),
        }
    }
}
impl Default for RandomGen {
    fn default() -> Self {
        use rand_chacha::rand_core::SeedableRng;
        Self {
            random_gen: ChaCha8Rng::seed_from_u64(SEED),
        }
    }
}

/// Pyramid component
#[derive(Component)]
pub struct Pyramid;

/// Face of pyramid component.
#[derive(Component)]
pub struct FaceMarker {
    pub face_index: usize,
    pub color: Color,
    pub normal: Vec3,
    /// The decorations on this face (if any)
    pub decorations: Option<DecorationSet>,
}


// A component that mars an entity to be rotated by the camera controls
#[derive(Component)]
pub struct RotableComponent;

/// A component that marks an entity as a game entity, which can be cleared during setup
#[derive(Component)]
pub struct GameEntity;

/// A component that marks an entity as a UI entity
#[derive(Component)]
pub struct UIEntity;
