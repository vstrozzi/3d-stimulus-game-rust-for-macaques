//! This file defines the various objects, resources, and components used in the game.
use bevy::prelude::*;
use rand_chacha::rand_core::SeedableRng;
use std::time::Duration;

use crate::utils::constants::game_constants::SEED;

use rand_chacha::ChaCha8Rng;

/// Game state enum representing the different states the game can be in
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, States, Hash)]
pub enum GamePhase {
    #[default]
    // The game is currently being played
    Playing,
    // The game has been won
    Won,
    // Transient state for resetting - immediately transitions to Playing
    Resetting,
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

/// Configuration used to setup the game (received from Controller)
#[derive(Debug, Clone, PartialEq)]
pub struct GameConfig {
    pub seed: u64,
    /// 0 or 1
    pub pyramid_type_code: u32,
    pub pyramid_base_radius: f32,
    pub pyramid_height: f32,
    pub pyramid_start_orientation_rad: f32,
    pub pyramid_target_door_index: usize,
    /// 3 faces, 4 channels
    pub pyramid_color_faces: [[f32; 4]; 3],
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            seed: SEED,
            pyramid_type_code: 0,
            pyramid_base_radius: 2.5,
            pyramid_height: 4.0,
            pyramid_start_orientation_rad: 0.0,
            pyramid_target_door_index: 5,
            pyramid_color_faces: [
                 [1.0, 0.2, 0.2, 1.0],
                 [0.2, 0.5, 1.0, 1.0],
                 [0.2, 1.0, 0.3, 1.0],
            ],
        }
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
    pub random_seed: u64,

    pub pyramid_type: PyramidType,
    pub pyramid_base_radius: f32,
    pub pyramid_height: f32,
    pub pyramid_start_orientation_rad: f32,
    pub pyramid_color_faces: [Color; 3],

    // The winning door side index
    pub pyramid_target_door_index: usize,

    // The time when the game started.
    pub start_time: Option<Duration>,
    // The time when the game ended.
    pub end_time: Option<Duration>,

    // Metrics
    // The number of attempts the player has made.
    pub nr_attempts: u32,
    // The cosine alignment of the camera with the target face when the player wins.
    pub cosine_alignment: Option<f32>,

    // Animation state
    pub animating_door: Option<Entity>,
    pub animating_light: Option<Entity>,
    pub animating_emissive: Option<Entity>,
    pub animation_start_time: Option<Duration>,
    pub is_animating: bool,
    pub pending_phase: Option<GamePhase>, // Phase to transition to after animation
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
        Self {
            random_gen: ChaCha8Rng::seed_from_u64(SEED),
        }
    }
}

/// Pyramid component
#[derive(Component)]
pub struct Pyramid;

// A component that marks an entity to be rotated by the camera controls
#[derive(Component)]
pub struct RotableComponent;

// A component that marks a pointlight as being one of the hole
#[derive(Component)]
pub struct HoleLight;

// A component that marks an emissive mesh as being the hole glow effect
#[derive(Component)]
pub struct HoleEmissive;

/// A component that marks an entity as a game entity, which can be cleared during setup
#[derive(Component)]
pub struct GameEntity;

/// A component that marks an entity as a UI entity
#[derive(Component)]
pub struct UIEntity;

/// A component that marks an entity as persistent (not despawned on reset)
#[derive(Component)]
pub struct PersistentCamera;

/// Component to mark the base frame (wooden panel with hole)
#[derive(Component)]
pub struct BaseFrame {
    pub door_index: usize,
}

/// Component to mark the base door (pentagon that covers the hole)
#[derive(Component)]
pub struct BaseDoor {
    pub door_index: usize,
    pub normal: Vec3, // In world coordinates
    pub is_open: bool,
}

// Component of the UI bar showing the score with lights
#[derive(Component)]
pub struct ScoreBarUI;
// Component marking the fill bar inside the ScoreBarUI
#[derive(Component)]
pub struct ScoreBarFill;
