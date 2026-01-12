//! Setup logic for the monkey_3d_game, with main setup plugin and functions for initializing the game scene and state.
use bevy::prelude::*;

use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{PrimitiveTopology};
use bevy::mesh::Indices;

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


/// Initial game scene, with the camera, ground, lights, and the pyramid
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut random_gen: ResMut<RandomGen>,
    time: Res<Time>,
) {
    // Two cameras for looks at the origin.
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
            base_color: Color::BLACK,
            perceptual_roughness: 0.8,
            ..default()
        })),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
        GameEntity,
    ));

    // Semicircle Wall surrounding the pyramid
    commands.spawn((
        Mesh3d(meshes.add(create_semicircle_mesh(9.0, 10.0, 64))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.2, 0.2),
            perceptual_roughness: 0.2,
            reflectance: 1.0,
            ior: 3.5,
            cull_mode: None,
            ..default()
        })),
        // The mesh is generated centered at (0,0) with radius 12.
        // It forms a semicircle from +X through -Z to -X.
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
        GameEntity,
    ));

    // Game light - PointLight positioned high to provide more uniform lighting
    commands.spawn((
        SpotLight {
            intensity: 4_000_000.0,
            shadows_enabled: true,
            outer_angle: std::f32::consts::PI / 3.0,
            range: 15.0, // Increased range since light is higher
            radius: 0.0,
            ..default()
        },
        Transform::from_xyz(0.0, 9.0, 0.0).looking_at(Vec3::ZERO, -Vec3::Y), // Higher position for more uniform lighting
        GameEntity,
    ));



    // Ambient light
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 200.0,
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


// Despawn all entities, needed when transitioning from Playing to MenuUI
pub fn despawn_setup(
    mut commands: Commands,
    query: Query<Entity, With<GameEntity>>,
) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
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

    // If the pyramid is of Type2, make two of its sides the same color
    // and set the door index to opposite direction of red side (counterclockwise)
    let mut pyramid_colors = PYRAMID_COLORS;
    let mut pyramid_target_door_index = 5;
    if pyramid_type == PyramidType::Type2 {
        pyramid_colors[2] = pyramid_colors[1];
        
        pyramid_target_door_index = 2;
    }

    // Create the initial game state
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
        cosine_alignment: None,

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


/// Spawn a semicircle around the pyramid object for reflections and hints to user.
fn create_semicircle_mesh(radius: f32, height: f32, segments: u32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Semicircle from 0 to PI (Right to Left, facing inward)
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = t * std::f32::consts::PI; 

        // x = R * cos(angle), z = -R * sin(angle)
        // angle=0 -> (R, 0, 0)
        // angle=PI/2 -> (0, 0, -R) (Back)
        // angle=PI -> (-R, 0, 0)
        let x = radius * angle.cos();
        let z = -radius * angle.sin();

        // Normal points outwards (to center)
        let normal = -Vec3::new(x, 0.0, z).normalize();

        // Bottom vertex
        positions.push([x, 0.0, z]);
        normals.push([normal.x, normal.y, normal.z]);
        uvs.push([t, 1.0]);

        // Top vertex
        positions.push([x, height, z]);
        normals.push([normal.x, normal.y, normal.z]);
        uvs.push([t, 0.0]);
    }

    for i in 0..segments {
        let base = i * 2;
        // CCW winding for inward face
        indices.push(base);     // Bottom Current
        indices.push(base + 2); // Bottom Next
        indices.push(base + 1); // Top Current

        indices.push(base + 1); // Top Current
        indices.push(base + 2); // Bottom Next
        indices.push(base + 3); // Top Next
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
