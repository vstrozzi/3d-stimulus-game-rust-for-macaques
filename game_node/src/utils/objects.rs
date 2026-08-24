//! This file defines the various objects, resources, and components used in the game.
use bevy::prelude::*;
use std::time::Duration;
use std::collections::{HashMap, HashSet};
use shared::{SharedGameState, SharedGameStateLocal, DecorationShape, Texture};
use crate::utils::load_assets::TextureSet;

/// Holds strong handles for every texture set so they are never GC'd between resets.
/// Populated once at startup; keeps assets hot in WASM so resets don't trigger new fetches.
#[derive(Resource, Default)]
pub struct PreloadedTextures(pub HashMap<Texture, TextureSet>);

impl PreloadedTextures {
    pub fn get(&self, tex: Texture) -> &TextureSet {
        self.0.get(&tex).expect("texture not preloaded")
    }
}

/// Texture variants this session needs. Both controllers publish the trial
/// indices through shared memory; the game adds the two structural materials
/// used by every pyramid.
#[derive(Resource, Clone, Default)]
pub struct TexturePreloadManifest(pub HashSet<Texture>);

impl TexturePreloadManifest {
    pub fn from_indices(indices: impl IntoIterator<Item = u32>) -> Self {
        let mut textures: HashSet<_> = indices
            .into_iter()
            .map(Texture::from_u32)
            .collect();
        textures.insert(Texture::Wood035_1K);
        textures.insert(Texture::Metal061B_1K);
        Self(textures)
    }

    pub fn includes(&self, texture: Texture) -> bool {
        self.0.contains(&texture)
    }
}

/// One-time startup state for the controller-driven texture preload.
#[derive(Resource, Default)]
pub struct TexturePreloadState {
    pub initialized: bool,
    pub warmup_spawned: bool,
}

#[cfg(test)]
mod texture_preload_manifest_tests {
    use super::*;

    #[test]
    fn unpublished_default_does_not_include_textures() {
        let manifest = TexturePreloadManifest::default();
        assert!(!manifest.includes(Texture::PavingStones143_1K));
    }

    #[test]
    fn manifest_includes_requested_structural_and_invalid_fallback_textures() {
        let manifest = TexturePreloadManifest::from_indices([
            Texture::Tiles128B_1K as u32,
            u32::MAX,
        ]);
        assert!(manifest.includes(Texture::Tiles128B_1K));
        assert!(manifest.includes(Texture::Wood035_1K));
        assert!(manifest.includes(Texture::Metal061B_1K));
        assert!(manifest.includes(Texture::WoodFloor057_1K));
        assert!(!manifest.includes(Texture::PavingStones143_1K));
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
    // Guards the one-shot  sound effect so it plays once per animation
    pub phase_sound_played: bool,
    // Sound-effect entities spawned for the current animation, despawned when it ends
    pub active_sounds: Vec<Entity>,
}

/// State for the pre-start loading countdown that gates `is_scene_ready`.
/// While it runs, a black overlay covers a fake pyramid and muted background
/// music, so the spawn/render/audio paths warm up before the controller starts.
#[derive(Resource, Default)]
pub struct LoadingCountdown {
    /// `time.elapsed()` when the countdown started, or `None` before it begins.
    pub start: Option<Duration>,
    /// Root entity of the black overlay (despawned recursively at the end).
    pub overlay: Option<Entity>,
    /// Background-music entity, spawned muted to warm the audio graph.
    pub music: Option<Entity>,
}

/// Marker for the countdown number text, updated each frame (3, 2, 1).
#[derive(Component)]
pub struct LoadingCountdownText;

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

#[derive(Resource, Default)]
pub struct CameraShakeState {
    /// time.elapsed() at which the current shake started, or None when idle.
    pub start: Option<Duration>,
    pub amplitude: f32,
    pub duration: f32,
    /// Last frame's applied offset, so the next frame can subtract it out
    /// before applying the new one (camera state stays clean after the
    /// shake finishes).
    pub last_offset: Vec3,
}

#[derive(Component)]
pub struct LeftScoreBarRoot;

#[derive(Component)]
pub struct LeftScoreBarFill;

/// Overlay on the left bar covering the step just gained or lost. Blinks for
/// the duration of the door animation, then disappears as the fill takes over.
#[derive(Component)]
pub struct LeftScoreBarDelta;

/// Round session clock (top-right): a disc whose spent wedge sweeps
/// clockwise from noon as the session runs down.
#[derive(Component)]
pub struct SessionClock;

#[derive(Component)]
pub struct HoleLight {
    pub door_index: usize,
}

#[derive(Component)]
pub struct HoleEmissive {
    pub door_index: usize,
}

/// A component that marks an entity as a game entity, which can be cleared during setup
#[derive(Component)]
pub struct GameEntity;

/// Marker for entities spawned during warmup; despawned once warmup
/// completes. Distinct from `GameEntity` so the normal trial reset path
/// never touches these.
#[derive(Component)]
pub struct WarmupEntity;

/// Marker for the photodiode square UI element.
#[derive(Component)]
pub struct PhotodiodeMarker;

/// One drifting firefly of the win-time swarm. Position is
/// `base + amp * sin(freq * t + phase)`. Driven by `fog.rs`.
#[derive(Component)]
pub struct Firefly {
    pub base: Vec3,
    pub amp: Vec3,
    pub freq: Vec3,
    pub phase: Vec3,
    pub flicker_phase: f32,
}

/// One ambient mote. Drifts like a [`Firefly`], but never bursts in and never
/// despawns: the pool is spawned once and motes past the current density are
/// simply scaled to zero. Driven by `fog.rs`.
#[derive(Component)]
pub struct AmbientMote {
    /// Position in the pool; only motes with `index < count` are shown.
    pub index: u32,
    pub base: Vec3,
    pub amp: Vec3,
    pub freq: Vec3,
    pub phase: Vec3,
    pub flicker_phase: f32,
}

/// Marks the two persistent backdrop surfaces, which are re-skinned from the
/// level config at every trial reset.
#[derive(Component)]
pub enum Backdrop {
    /// The ground plane the whole scene stands on.
    Platform,
    /// The curved wall behind the object.
    Background,
}

/// A component that marks an entity as a UI entity
#[derive(Component)]
pub struct UIEntity;

/// A component that marks an entity as persistent (not despawned on reset)
#[derive(Component)]
pub struct PersistentCamera;

/// Offscreen image the 3D scene renders into when `RENDER_AT_FIXED_RESOLUTION`
/// is on. 
#[derive(Resource, Default)]
pub struct RenderTargetImage {
    pub handle: Option<Handle<Image>>,
    pub width: u32,
    pub height: u32,
}

/// Set true once native exclusive fullscreen downscaling is active, so the
/// offscreen render-to-texture path (`setup_fixed_resolution`) stays disabled
/// — it would be redundant once the window itself runs at the capped mode.
/// Starts false; flipped on by `setup_fixed_fullscreen` (native only). On web
/// it stays false and the offscreen path always runs, since winit ignores the
/// window-resolution cap there.
#[derive(Resource, Default)]
pub struct FixedFullscreenActive(pub bool);

/// Marker for the native-resolution 2D camera that draws the upscaled
/// backdrop and the UI on top.
#[derive(Component)]
pub struct UpscaleCamera;

/// Marker for the full-window UI node that shows the upscaled render image.
#[derive(Component)]
pub struct RenderBackdrop;

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
