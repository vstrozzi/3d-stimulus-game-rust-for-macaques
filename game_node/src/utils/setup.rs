//! Setup logic for the monkey_3d_game, with main setup plugin and functions for initializing the game scene and state.
use bevy::prelude::*;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;

use crate::log;
use crate::utils::constants::{
    camera_3d_constants::{CAMERA_3D_INITIAL_X, CAMERA_3D_INITIAL_Y, CAMERA_3D_INITIAL_Z},
    game_constants::SEED,
    lighting_constants::{AMBIENT_BRIGHTNESS, MAIN_SPOTLIGHT_INTENSITY},
    object_constants::GROUND_Y,
    pyramid_constants::*,
};
use crate::utils::objects::*;
use crate::utils::pyramid::spawn_pyramid;

use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Initial game scene, with the camera, ground, lights, and the pyramid
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut random_gen: ResMut<RandomGen>,
    time: Res<Time>,
    setup_config: Option<Res<SetupConfig>>,
) {
    let config_to_use = setup_config.and_then(|c| c.0.clone());

    if let Some(ref config) = config_to_use {
        random_gen.random_gen = ChaCha8Rng::seed_from_u64(config.seed);
    }

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

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::BLACK,
            perceptual_roughness: 0.8,
            ..default()
        })),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
        GameEntity,
    ));

    commands.spawn((
        Mesh3d(meshes.add(create_extended_semicircle_mesh(9.0, 10.0, 20.0, 64))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.2, 0.2),
            perceptual_roughness: 0.2,
            reflectance: 1.0,
            ior: 3.5,
            cull_mode: None,
            ..default()
        })),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
        GameEntity,
    ));

    commands.spawn((
        SpotLight {
            intensity: MAIN_SPOTLIGHT_INTENSITY,
            shadows_enabled: true,
            outer_angle: std::f32::consts::PI / 3.0,
            range: 45.0,
            radius: 0.0,
            ..default()
        },
        Transform::from_xyz(0.0, 15.0, 0.0).looking_at(Vec3::ZERO, -Vec3::Y),
        GameEntity,
    ));

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: AMBIENT_BRIGHTNESS,
        affects_lightmapped_meshes: true,
    });

    let mut game_state = if let Some(ref config) = config_to_use {
        setup_game_state_from_config(&mut commands, &time, config)
    } else {
        setup_game_state(&mut commands, &time, &mut random_gen)
    };

    spawn_pyramid(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut random_gen,
        &mut game_state,
    );

    if config_to_use.is_some() {
        commands.insert_resource(SetupConfig(None));
    }

    log!("🎮 Pyramid Game Started!");
}

pub fn despawn_setup(mut commands: Commands, query: Query<Entity, With<GameEntity>>) {
    for entity in &query {
        commands.entity(entity).try_despawn();
    }
}

pub fn setup_game_state(
    commands: &mut Commands,
    time: &Res<Time>,
    random_gen: &mut ResMut<RandomGen>,
) -> GameState {
    let pyramid_type = if random_gen.random_gen.next_u64() % 2 == 0 {
        PyramidType::Type1
    } else {
        PyramidType::Type2
    };

    let pyramid_base_radius = random_gen
        .random_gen
        .random_range(PYRAMID_BASE_RADIUS_MIN..=PYRAMID_BASE_RADIUS_MAX);
    let pyramid_height = random_gen
        .random_gen
        .random_range(PYRAMID_HEIGHT_MIN..=PYRAMID_HEIGHT_MAX);

    let pyramid_start_orientation_rad = random_gen
        .random_gen
        .random_range(PYRAMID_ANGLE_OFFSET_RAD_MIN..PYRAMID_ANGLE_OFFSET_RAD_MAX);

    let mut pyramid_colors = PYRAMID_COLORS;
    let mut pyramid_target_door_index = 5;
    if pyramid_type == PyramidType::Type2 {
        pyramid_colors[2] = pyramid_colors[1];
        pyramid_target_door_index = 2;
    }

    let game_state = GameState {
        random_seed: SEED,
        pyramid_type: pyramid_type,
        pyramid_base_radius: pyramid_base_radius,
        pyramid_height: pyramid_height,
        pyramid_start_orientation_rad: pyramid_start_orientation_rad,
        pyramid_color_faces: pyramid_colors,
        pyramid_target_door_index: pyramid_target_door_index,
        start_time: Some(time.elapsed()),
        end_time: None,

        nr_attempts: 0,
        cosine_alignment: Some(0.0),

        animating_emissive: None,
        animating_door: None,
        animating_light: None,
        animation_start_time: None,
        is_animating: false,
        pending_phase: None,
    };

    let cloned_game_state = game_state.clone();
    commands.insert_resource(game_state);

    return cloned_game_state;
}

pub fn setup_game_state_from_config(
    commands: &mut Commands,
    time: &Res<Time>,
    config: &GameConfig,
) -> GameState {
    let pyramid_type = if config.pyramid_type_code == 0 {
        PyramidType::Type1
    } else {
        PyramidType::Type2
    };

    let pyramid_colors = [
        Color::srgba(
            config.pyramid_color_faces[0][0],
            config.pyramid_color_faces[0][1],
            config.pyramid_color_faces[0][2],
            config.pyramid_color_faces[0][3],
        ),
        Color::srgba(
            config.pyramid_color_faces[1][0],
            config.pyramid_color_faces[1][1],
            config.pyramid_color_faces[1][2],
            config.pyramid_color_faces[1][3],
        ),
        Color::srgba(
            config.pyramid_color_faces[2][0],
            config.pyramid_color_faces[2][1],
            config.pyramid_color_faces[2][2],
            config.pyramid_color_faces[2][3],
        ),
    ];

    let game_state = GameState {
        random_seed: config.seed,
        pyramid_type,
        pyramid_base_radius: config.pyramid_base_radius,
        pyramid_height: config.pyramid_height,
        pyramid_start_orientation_rad: config.pyramid_start_orientation_rad,
        pyramid_color_faces: pyramid_colors,
        pyramid_target_door_index: config.pyramid_target_door_index,
        start_time: Some(time.elapsed()),
        end_time: None,
        nr_attempts: 0,
        cosine_alignment: Some(0.0),
        animating_emissive: None,
        animating_door: None,
        animating_light: None,
        animation_start_time: None,
        is_animating: false,
        pending_phase: None,
    };

    let cloned_game_state = game_state.clone();
    commands.insert_resource(game_state);

    cloned_game_state
}

use crate::command_handler::PendingReset;

/// Legacy reset handler - replaced by handle_reset_command in systems_logic.rs
#[allow(dead_code)]
pub fn apply_pending_reset(
    mut pending_reset: ResMut<PendingReset>,
    mut commands: Commands,
    entities_query: Query<Entity, With<GameEntity>>,
    ui_entities_query: Query<Entity, With<UIEntity>>,
    mut next_state: ResMut<NextState<GamePhase>>,
) {
    let Some(config) = pending_reset.0.take() else {
        return;
    };

    info!("Applying reset with config seed: {}", config.seed);

    for entity in &entities_query {
        commands.entity(entity).try_despawn();
    }

    for entity in &ui_entities_query {
        commands.entity(entity).try_despawn();
    }

    commands.insert_resource(SetupConfig(Some(config)));
    next_state.set(GamePhase::Playing);
}

#[derive(Resource, Default)]
pub struct SetupConfig(pub Option<GameConfig>);

fn create_extended_semicircle_mesh(
    radius: f32,
    height: f32,
    extension: f32,
    segments: u32,
) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    let arc_len = std::f32::consts::PI * radius;
    let total_len = arc_len + (2.0 * extension);

    let mut push_column = |x: f32, z: f32, normal: Vec3, u_dist: f32| {
        let u = u_dist / total_len;
        positions.push([x, 0.0, z]);
        normals.push([normal.x, normal.y, normal.z]);
        uvs.push([u, 1.0]);

        positions.push([x, height, z]);
        normals.push([normal.x, normal.y, normal.z]);
        uvs.push([u, 0.0]);
    };

    push_column(radius, extension, Vec3::NEG_X, 0.0);

    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = t * std::f32::consts::PI;
        let x = radius * angle.cos();
        let z = -radius * angle.sin();
        let normal = -Vec3::new(x, 0.0, z).normalize();
        let current_dist = extension + (t * arc_len);
        push_column(x, z, normal, current_dist);
    }

    push_column(-radius, extension, Vec3::X, total_len);

    let total_columns = positions.len() as u32 / 2;

    for i in 0..(total_columns - 1) {
        let base = i * 2;
        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 1);

        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base + 3);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
