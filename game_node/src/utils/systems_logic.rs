//! Systems logic based on the gamephase.
//!
//! Twin-Engine Architecture: The game no longer handles inputs directly.
//! All inputs are processed by the Controller which sends GameCommands.

use crate::command_handler::{PendingBlankScreen, PendingReset, RenderingPaused};
use crate::utils::camera::{apply_pending_rotation, apply_pending_zoom};
use crate::utils::game_functions::{
    apply_pending_check_alignment, handle_door_animation,
    spawn_score_bar, update_score_bar_animation, update_ui_scale,
};
use crate::utils::objects::{GameEntity, GamePhase, GameState, PersistentCamera, UIEntity};
use crate::utils::setup::{setup, SetupConfig};
use crate::utils::constants::camera_3d_constants::{
    CAMERA_3D_INITIAL_X, CAMERA_3D_INITIAL_Y, CAMERA_3D_INITIAL_Z,
};
use bevy::prelude::*;

// Plugin for managing all the game systems based on the current game phase.
pub struct SystemsLogicPlugin;

impl Plugin for SystemsLogicPlugin {
    /// Builds the plugin by adding the systems to the app.
    fn build(&self, app: &mut App) {
        // Start directly in Playing phase (menu is handled externally by Controller)
        app.insert_state(GamePhase::Playing)
            .init_resource::<SetupConfig>()
            .init_resource::<BlankScreenState>()
            // Spawn persistent camera once at startup
            .add_systems(Startup, spawn_persistent_camera)
            // Global UI responsiveness system (runs every frame)
            .add_systems(Update, update_ui_scale)
            // Global command-driven system for reset (runs any time, handles reset from any state)
            .add_systems(Update, handle_reset_command)
            // Rendering control systems (run any time)
            .add_systems(Update, (apply_blank_screen, handle_rendering_pause))
            // Resetting State - transient state that immediately goes to Playing
            .add_systems(OnEnter(GamePhase::Resetting), on_enter_resetting)
            // Playing State
            .add_systems(OnEnter(GamePhase::Playing), (setup, spawn_score_bar).chain())
            .add_systems(
                Update,
                (
                    // Command-driven systems (from Twin-Engine Controller)
                    (apply_pending_rotation, apply_pending_zoom, apply_pending_check_alignment)
                        .run_if(in_state(GamePhase::Playing).and(is_not_animating).and(is_not_paused)),
                    // Animation systems (run while animating, but not when paused)
                    (handle_door_animation, update_score_bar_animation)
                        .run_if(in_state(GamePhase::Playing).and(is_not_paused)),
                ),
            )
            .add_systems(
                OnExit(GamePhase::Playing),
                despawn_all_game_and_ui,
            );
            // Won State is now passive - controller handles black screen and timing
    }
}

// ============================================================================
// RUN CONDITIONS
// ============================================================================

fn is_not_animating(game_state: Res<GameState>) -> bool {
    !game_state.is_animating
}

fn is_not_paused(rendering_paused: Res<RenderingPaused>) -> bool {
    !rendering_paused.0
}

// ============================================================================
// PERSISTENT CAMERA SETUP
// ============================================================================

/// Spawns the 3D camera once at startup. This camera persists across resets.
fn spawn_persistent_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(
            CAMERA_3D_INITIAL_X,
            CAMERA_3D_INITIAL_Y,
            CAMERA_3D_INITIAL_Z,
        )
        .looking_at(Vec3::ZERO, Vec3::Y),
        PersistentCamera,
    ));
}

// ============================================================================
// BLANK SCREEN RESOURCES AND COMPONENTS
// ============================================================================

/// Resource tracking blank screen state
#[derive(Resource, Default)]
pub struct BlankScreenState {
    pub is_active: bool,
}

/// Marker component for the blank screen overlay entity
#[derive(Component)]
pub struct BlankScreenOverlay;

/// Helper function to spawn a fullscreen black overlay
fn spawn_blank_overlay(commands: &mut Commands) {
    commands.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(1000),
        BlankScreenOverlay,
    ));
}

// ============================================================================
// RESET HANDLING
// ============================================================================

/// Unified reset handler that works from any state.
/// Always transitions to Resetting state first, which then goes to Playing.
fn handle_reset_command(
    mut pending_reset: ResMut<PendingReset>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<GamePhase>>,
) {
    let Some(config) = pending_reset.0.take() else {
        return;
    };

    info!("Reset command received with config seed: {}", config.seed);

    // Store config for setup to use when entering Playing state
    commands.insert_resource(SetupConfig(Some(config)));

    // Always go through Resetting state - this ensures:
    // 1. OnExit of current state runs (cleanup)
    // 2. OnEnter(Resetting) runs (clears overlays, transitions to Playing)
    // 3. OnEnter(Playing) runs (setup)
    next_state.set(GamePhase::Resetting);
}

/// Called when entering Resetting state - cleanup and immediately transition to Playing
fn on_enter_resetting(
    mut commands: Commands,
    entities_query: Query<Entity, With<GameEntity>>,
    ui_entities_query: Query<Entity, With<UIEntity>>,
    mut next_state: ResMut<NextState<GamePhase>>,
) {
    info!("Entering Resetting state - cleaning up and transitioning to Playing");

    // Despawn all game entities
    for entity in &entities_query {
        commands.entity(entity).try_despawn();
    }

    // Despawn all UI entities
    for entity in &ui_entities_query {
        commands.entity(entity).try_despawn();
    }

    // Note: BlankScreenOverlay is preserved - only removed via explicit B key toggle

    // Immediately transition to Playing
    next_state.set(GamePhase::Playing);
}

// Win state is now passive - controller handles black screen and timing via shared memory.
// The game just remains in Won state until controller sends reset command.

// ============================================================================
// RENDERING CONTROL SYSTEMS
// ============================================================================

/// System to apply blank screen command - spawns/despawns a black fullscreen overlay
fn apply_blank_screen(
    mut commands: Commands,
    pending_blank: Res<PendingBlankScreen>,
    mut blank_state: ResMut<BlankScreenState>,
    overlay_query: Query<Entity, With<BlankScreenOverlay>>,
) {
    if pending_blank.0 {
        // Toggle blank screen state
        blank_state.is_active = !blank_state.is_active;

        if blank_state.is_active {
            // Spawn black fullscreen overlay
            spawn_blank_overlay(&mut commands);
            info!("Blank screen activated");
        } else {
            // Despawn the overlay
            for entity in overlay_query.iter() {
                commands.entity(entity).despawn();
            }
            info!("Blank screen deactivated");
        }
    }
}

/// System to handle rendering pause - hides/shows the persistent camera
fn handle_rendering_pause(
    rendering_paused: Res<RenderingPaused>,
    mut visibility_query: Query<&mut Visibility, With<PersistentCamera>>,
) {
    // Only act when the resource has changed
    if !rendering_paused.is_changed() {
        return;
    }

    // When paused, we can hide the 3D camera to stop rendering
    for mut visibility in visibility_query.iter_mut() {
        if rendering_paused.0 {
            *visibility = Visibility::Hidden;
        } else {
            *visibility = Visibility::Visible;
        }
    }
}

// ============================================================================
// CLEANUP SYSTEMS
// ============================================================================

/// Despawn all game and UI entities
fn despawn_all_game_and_ui(
    mut commands: Commands,
    game_query: Query<Entity, With<GameEntity>>,
    ui_query: Query<Entity, With<UIEntity>>,
) {
    for entity in &game_query {
        commands.entity(entity).try_despawn();
    }
    for entity in &ui_query {
        commands.entity(entity).try_despawn();
    }
}
