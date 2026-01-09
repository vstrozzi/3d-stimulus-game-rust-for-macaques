//! Systems logic based on the gamephase.

use bevy::prelude::*;
use crate::utils::game_functions::{
    despawn_ui, menu_inputs, playing_inputs, handle_won_input,
    setup_intro_ui, setup_playing_ui, setup_won_ui, handle_door_animation
};
use crate::utils::objects::{GamePhase, GameState};
use crate::utils::setup::{setup, despawn_setup};
use crate::utils::camera::camera_3d_fpov_inputs;

// Plugin for managing all the game systems based on the current game phase.
pub struct SystemsLogicPlugin;


impl Plugin for SystemsLogicPlugin {
    /// Builds the plugin by adding the systems to the app.
    fn build(&self, app: &mut App) {
        app.init_state::<GamePhase>()
            // Intro State
            .add_systems(OnEnter(GamePhase::MenuUI), setup_intro_ui)
            .add_systems(
                Update,
                menu_inputs.run_if(in_state(GamePhase::MenuUI)),
            )
            .add_systems(OnExit(GamePhase::MenuUI), despawn_ui)
            
            // Playing State
            .add_systems(OnEnter(GamePhase::Playing), (setup, setup_playing_ui).chain())
            
            .add_systems(
                Update,
                (
                    // Group A: only in Playing AND not animating
                    (playing_inputs, camera_3d_fpov_inputs)
                        .run_if(in_state(GamePhase::Playing).and(is_animating)),

                    // Group B: only in Playing
                    (handle_door_animation)
                        .run_if(in_state(GamePhase::Playing)),
                ),
            )

            .add_systems(OnExit(GamePhase::Playing), (despawn_ui, despawn_setup))
            
            // Won State
            .add_systems(OnEnter(GamePhase::Won), setup_won_ui)
            .add_systems(
                Update,
                handle_won_input.run_if(in_state(GamePhase::Won)),
            )
            .add_systems(OnExit(GamePhase::Won), despawn_ui);
    }
}


// Bevy needs a function for systems
fn is_animating(game_state: Res<GameState>) -> bool {
    ! game_state.is_animating
}