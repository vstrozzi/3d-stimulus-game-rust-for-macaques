//! Setup logic for the monkey_3d_game, with main setup plugin and functions for initializing the game scene and state.
use bevy::prelude::*;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;

use crate::log;
use crate::utils::objects::*;
use crate::utils::pyramid::spawn_pyramid;
use shared::constants::{
    lighting_constants::{GLOBAL_AMBIENT_LIGHT_INTENSITY, SPOTLIGHT_LIGHT_INTENSITY},
    object_constants::GROUND_Y,
};

use crate::command_handler::SharedMemResource;
use core::sync::atomic::Ordering;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Initial game scene, with the camera, ground, lights, and the pyramid.
/// Setup the persistent entitites across resets.
pub fn setup_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Ground Plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::BLACK,
            perceptual_roughness: 0.8,
            ..default()
        })),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
    ));

    // Curved Background
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
    ));

    // Main Spotlight
    commands.spawn((
        SpotLight {
            intensity: SPOTLIGHT_LIGHT_INTENSITY, // Default start value
            shadows_enabled: true,
            outer_angle: std::f32::consts::PI / 3.0,
            range: 45.0,
            radius: 0.0,
            ..default()
        },
        Transform::from_xyz(0.0, 15.0, 0.0).looking_at(Vec3::ZERO, -Vec3::Y),
    ));

    // Ambient Light
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: GLOBAL_AMBIENT_LIGHT_INTENSITY, // Default start value
        affects_lightmapped_meshes: true,
    });

    log!("🌍 Environment Setup Complete");
}

/// Setup a specific game trial.
/// This spawns the pyramid and resets the camera. All spawned entities are marked with GameEntity.
pub fn setup_round(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut random_gen: ResMut<RandomGen>,
    mut camera_query: Query<&mut Transform, With<PersistentCamera>>,
    mut spotlight_query: Query<&mut SpotLight, (Without<HoleLight>, Without<GameEntity>)>,
    ambient_light: Option<ResMut<GlobalAmbientLight>>,
    shm_res: Option<Res<SharedMemResource>>,
    mut round_start: ResMut<crate::utils::objects::RoundStartTimestamp>,
    time: Res<Time>,
    mut door_win_entities: ResMut<DoorWinEntities>,
) {
    // Read shared memory
    let Some(shm_res) = shm_res else {
        error!("Shared Memory not initialized in setup_round");
        return;
    };

    let shm = shm_res.0.get();

    // Set round start time
    round_start.0 = Some(time.elapsed());

    // Read control values from sh,
    let gs_ctrl = &shm.game_structure_control;
    // Reset all fields of game structure
    let gs_game = &shm.game_structure_game;
    gs_game.reset_all_fields(gs_ctrl);

    // Update all the game resoruces based on the new configuration
    let seed = gs_game.seed.load(Ordering::Relaxed);
    random_gen.random_gen = ChaCha8Rng::seed_from_u64(seed);

    let main_intensity = f32::from_bits(gs_game.main_spotlight_intensity.load(Ordering::Relaxed));
    let ambient_intensity = f32::from_bits(gs_game.ambient_brightness.load(Ordering::Relaxed));
    // Update Lights
    for mut spot in spotlight_query.iter_mut() {
        spot.intensity = main_intensity;
    }

    if let Some(mut ambient) = ambient_light {
        ambient.brightness = ambient_intensity;
    }

    // Reset the persistent camera position
    if let Ok(mut camera_transform) = camera_query.single_mut() {
        *camera_transform = Transform::from_xyz(
            f32::from_bits(gs_ctrl.camera_x.load(Ordering::Relaxed)),
            f32::from_bits(gs_ctrl.camera_y.load(Ordering::Relaxed)),
            f32::from_bits(gs_ctrl.camera_z.load(Ordering::Relaxed)),
        )
        .looking_at(Vec3::ZERO, Vec3::Y);
    }

    gs_game.win_time.store(0, Ordering::Relaxed);

    let p_type_code = gs_game.pyramid_type.load(Ordering::Relaxed);
    let p_type = if p_type_code == 0 {
        PyramidType::Type1
    } else {
        PyramidType::Type2
    };

    let radius = f32::from_bits(gs_game.base_radius.load(Ordering::Relaxed));
    let height = f32::from_bits(gs_game.height.load(Ordering::Relaxed));
    let orient = f32::from_bits(gs_game.start_orient.load(Ordering::Relaxed));

    let mut colors = [Color::WHITE; 3];
    for i in 0..3 {
        let r = f32::from_bits(gs_game.colors[i * 4 + 0].load(Ordering::Relaxed));
        let g = f32::from_bits(gs_game.colors[i * 4 + 1].load(Ordering::Relaxed));
        let b = f32::from_bits(gs_game.colors[i * 4 + 2].load(Ordering::Relaxed));
        let a = f32::from_bits(gs_game.colors[i * 4 + 3].load(Ordering::Relaxed));
        colors[i] = Color::srgba(r, g, b, a);
    }

    let mut decoration_counts = [0; 3];
    for i in 0..3 {
        decoration_counts[i] = gs_game.decorations_count[i].load(Ordering::Relaxed);
    }

    let mut decoration_sizes = [0.0; 3];
    for i in 0..3 {
        decoration_sizes[i] = f32::from_bits(gs_game.decorations_size[i].load(Ordering::Relaxed));
    }

    // Read target door from shared memory
    let target_door = gs_game.target_door.load(Ordering::Relaxed) as usize;
    
    // Spawn the pyramid and capture winning door entities
    let (winning_light, winning_emissive) = spawn_pyramid(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut random_gen,
        p_type,
        radius,
        height,
        orient,
        colors,
        decoration_counts,
        decoration_sizes,
        target_door,
    );

    // Populate DoorWinEntities with the target door's entities and reset timer
    door_win_entities.winning_light = winning_light;
    door_win_entities.winning_emissive = winning_emissive;
    door_win_entities.animation_start_time = None;

    log!("🎮 Round Started! target_door={}, winning_light={:?}, winning_emissive={:?}", target_door, winning_light, winning_emissive);
}



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
