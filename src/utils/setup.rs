//! Setup logic for the monkey_3d_game, with main setup plugin and functions for initializing the game scene and state.
use bevy::prelude::*;

use crate::log;
use crate::utils::constants::{
    camera_3d_constants::{CAMERA_3D_INITIAL_X, CAMERA_3D_INITIAL_Y, CAMERA_3D_INITIAL_Z},
    game_constants::SEED,
    object_constants::GROUND_Y,
    pyramid_constants::*,
};
use crate::utils::objects::*;
use crate::utils::pyramid::spawn_pyramid;

use rand::{Rng, RngCore};

/// Plugin for initial setup of the game
pub struct SetupPlugin;

impl Plugin for SetupPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, crate::utils::setup::setup);
    }
}

/// Initial game scene, with the camera, ground, lights, and the pyramid
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut random_gen: ResMut<RandomGen>,
    time: Res<Time>,
) {
    // Camera looks at the origin.
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(
            CAMERA_3D_INITIAL_X,
            CAMERA_3D_INITIAL_Y,
            CAMERA_3D_INITIAL_Z,
        )
        .looking_at(Vec3::ZERO, Vec3::Y),
        GameEntity,
    ));

    // Ground plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
        GameEntity,
    ));

    // Game light
    commands.spawn((
        PointLight {
            intensity: 2_000_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(2.0, 2.0, -2.0),
        GameEntity,
    ));

    // Ambient light
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 50.0,
        affects_lightmapped_meshes: true,
    });

    // Game State with per session parameters
    let mut game_state = setup_game_state(&mut commands, &time, &mut random_gen);

    // Pyramid
    spawn_pyramid(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut random_gen,
        &mut game_state,
    );

    log!("🎮 Pyramid Game Started!");
}

/// Initialize the GameState
pub fn setup_game_state(
    commands: &mut Commands,
    time: &Res<Time>,
    random_gen: &mut ResMut<RandomGen>,
) -> GameState {

    // Determine the pyramid type randomly
    let pyramid_type = if random_gen.random_gen.next_u64() % 2 == 0 {
        PyramidType::Type1
    } else {
        PyramidType::Type2
    };

    // Determine the pyramid's base radius and height randomly
    let pyramid_base_radius = random_gen
        .random_gen
        .random_range(PYRAMID_BASE_RADIUS_MIN..=PYRAMID_BASE_RADIUS_MAX);
    let pyramid_height = random_gen
        .random_gen
        .random_range(PYRAMID_HEIGHT_MIN..=PYRAMID_HEIGHT_MAX);

    // Determine the pyramid's starting orientation randomly
    let pyramid_start_orientation_rad = random_gen
        .random_gen
        .random_range(PYRAMID_ANGLE_OFFSET_RAD_MIN..PYRAMID_ANGLE_OFFSET_RAD_MAX);
    let pyramid_target_face_index = 0;

    let mut pyramid_colors = PYRAMID_COLORS;

    // If the pyramid is of Type2, make two of its sides the same color
    if pyramid_type == PyramidType::Type2 {
        if random_gen.random_gen.next_u64() % 2 == 0 {
            pyramid_colors[1] = pyramid_colors[2];
        } else {
            pyramid_colors[2] = pyramid_colors[1];
        }
    }

    // Create the initial game state
    let game_state = GameState {
        random_seed: SEED,
        pyramid_type: pyramid_type,
        pyramid_base_radius: pyramid_base_radius,
        pyramid_height: pyramid_height,
        pyramid_target_face_index: pyramid_target_face_index as usize,
        pyramid_start_orientation_rad: pyramid_start_orientation_rad,
        pyramid_color_faces: pyramid_colors,

        phase: GamePhase::NotStarted,
        is_changed: true,

        start_time: Some(time.elapsed()),
        end_time: None,

        nr_attempts: 0,
        cosine_alignment: None,
    };


    let cloned_game_state = game_state.clone();
    commands.insert_resource(game_state);

    return cloned_game_state;
}
