//! This file defines the various objects, resources, and components used in the game.
use bevy::prelude::*;
use std::time::Duration;
use shared::{SharedGameState, SharedGameStateLocal, DecorationShape};

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


/// Resource for the current conditions the system across trials
#[derive(Resource)]
pub struct GameConditions{
    pub stop_rendering: bool,
    pub blank_screen: bool,
}

impl Default for GameConditions {
    fn default() -> Self {
        GameConditions {
            stop_rendering: false,
            blank_screen: false,
        }
    }
}

/// Resource current winning doors and animation state
#[derive(Resource, Default)]
pub struct DoorWinEntities {
    // Winning door entities (set once per round in setup_round)
    pub winning_light: Option<Entity>,
    pub winning_emissive: Option<Entity>,    
    // Animation timing
    pub animation_start_time: Option<Duration>,

    // Animate all doors flag
    pub animate_all: bool,
    pub color: Color,
}

/// Resource to track the start time of the current trial
#[derive(Resource, Default)]
pub struct RoundStartTimestamp(pub Option<Duration>);

/// Local resource for the game structure
#[derive(Resource)]
pub struct GameStateLocal(pub SharedGameStateLocal);
impl Default for GameStateLocal {
    fn default() -> Self {
        GameStateLocal(SharedGameState::default().to_not_atomic())
    }
}

///

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

/// Marker component for the blank screen overlay entity
#[derive(Component)]
pub struct BlankScreen;
