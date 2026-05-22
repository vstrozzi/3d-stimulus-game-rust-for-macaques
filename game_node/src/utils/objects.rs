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

/// Single decoration on a pyramid face. `uv` is the bilinear coordinate in
/// the (tl, tr, bl, br) quad: u=0 left, u=1 right, v=0 top, v=1 bottom.
/// `rotation` is the in-face rotation (radians) around the face normal.
#[derive(Clone, Debug)]
pub struct Decoration {
    pub uv: Vec2,
    pub size: f32,
    pub thickness: f32,
    pub rotation: f32,
}

/// Set of decorations for a pyramid face, which all share same shape and color
#[derive(Clone, Debug)]
pub struct DecorationSet {
    pub shape: DecorationShape,
    pub color: Color,
    pub decorations: Vec<Decoration>,
}

/// Resource for the current conditions the system across trials
#[derive(Resource, Default)]
pub struct GameConditions{
    pub stop_rendering: bool,
    /// True once all texture assets for the current trial are fully loaded on the GPU.
    /// Reset to false on every reset command; the controller gates space-to-start on this.
    pub is_scene_ready: bool,
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

// A component that marks a pointlight as being one of the hole.
// `door_index` is the same value as on the parent `BaseFrame`, duplicated
// here so the faint-aligned-door system can filter without joining through
// `ChildOf` every frame.
#[derive(Component)]
pub struct HoleLight {
    pub door_index: usize,
}

// A component that marks an emissive mesh as being the hole glow effect.
// `door_index` mirrors the parent `BaseFrame`.
#[derive(Component)]
pub struct HoleEmissive {
    pub door_index: usize,
}

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

// Component of the UI bar showing the score with lights.
// `row_start` is the first dot index displayed in this row; the row is
// hidden when no dots in `[row_start, row_start+WRAP)` are active.
#[derive(Component)]
pub struct ScoreBarUI {
    pub row_start: u32,
}

/// Root entity of the persistent score-bar entity pool. Spawned once at
/// startup and never despawned; `update_score_bar` toggles `Node.display`
/// on the dot/chain children to show only the active subset.
#[derive(Component)]
pub struct ScoreBarRoot;

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

/// All parameters needed to spawn a pyramid for one trial.
/// Built from shared memory in setup.rs and consumed by pyramid.rs.
pub struct PyramidConfig {
    pub decoration_seeds: [u64; 3],
    pub radius: f32,
    pub height: f32,
    /// Pre-adjusted orientation (start_orient + FRAC_PI_6)
    pub orientation_rad: f32,
    pub colors: [Color; 3],
    pub decoration_counts: [u32; 3],
    pub decoration_sizes: [f32; 3],
    pub decoration_shapes: [DecorationShape; 3],
    pub face_textures: [u32; 3],
    pub decoration_colors: [Color; 3],
    pub decoration_textures: [u32; 3],
    pub decoration_thicknesses: [f32; 3],
    /// Per-face decoration rotation in degrees. `>= 0` is a fixed angle;
    /// `-1` means each decoration on the face gets an independent random angle.
    pub decoration_rotations: [i32; 3],
    pub target_door: usize,
}
