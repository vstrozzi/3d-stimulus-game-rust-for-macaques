//! This file defines the various objects, resources, and components used in the game.
use bevy::prelude::*;
use std::time::Duration;
use std::collections::HashMap;
use shared::{SharedGameState, SharedGameStateLocal, DecorationShape, Texture};
use crate::utils::load_textures::TextureSet;

/// Holds strong handles for every texture set so they are never GC'd between resets.
/// Populated once at startup; keeps assets hot in WASM so resets don't trigger new fetches.
#[derive(Resource, Default)]
pub struct PreloadedTextures(pub HashMap<Texture, TextureSet>);

impl PreloadedTextures {
    pub fn get(&self, tex: Texture) -> &TextureSet {
        self.0.get(&tex).expect("texture not preloaded")
    }
}

/// Single decoration on a pyramid face with barycentric coordinates relative to the triangle vertices (top, corner1, corner2)
#[derive(Clone, Debug)]
pub struct Decoration {
    pub barycentric: Vec3,
    pub size: f32,
    pub thickness: f32,
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
    /// True once all texture assets for the current trial are fully loaded on the GPU.
    /// Reset to false on every reset command; the controller gates space-to-start on this.
    pub is_scene_ready: bool,
}

impl Default for GameConditions {
    fn default() -> Self {
        GameConditions {
            stop_rendering: false,
            blank_screen: false,
            is_scene_ready: false,
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

// Component for the score bar dots
#[derive(Component)]
pub struct ScoreBarDot {
    pub index: u32,
}

// Component for the score bar chains (lines connecting the dots)
#[derive(Component)]
pub struct ScoreBarChain {
    pub index: u32,
}

/// Marker component for the blank screen overlay entity
#[derive(Component)]
pub struct BlankScreen;
