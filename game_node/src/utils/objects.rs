//! This file defines the various objects, resources, and components used in the game.
use bevy::prelude::*;
use rand_chacha::rand_core::SeedableRng;

use shared::constants::game_constants::SEED;

use rand_chacha::ChaCha8Rng;
use std::time::Duration;

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

/// The current winning doors and animation state
#[derive(Resource, Default)]
pub struct DoorWinEntities {
    // Winning door entities (set once per round in setup_round)
    pub winning_light: Option<Entity>,
    pub winning_emissive: Option<Entity>,
    // Animation entities (active during door animation)
    
    // Animation timing
    pub animation_start_time: Option<Duration>,
}

/// Resource to track the start time of the current round
#[derive(Resource, Default)]
pub struct RoundStartTimestamp(pub Option<Duration>);

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
