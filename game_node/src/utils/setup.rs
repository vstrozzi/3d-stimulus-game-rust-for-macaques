//! Setup logic for the monkey_3d_game, with main setup plugin and functions for initializing the game scene and state.
use std::time::Duration;

use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;

use shared::{DecorationShape};
use crate::utils::objects::*;
use crate::utils::pyramid::spawn_pyramid;
use crate::utils::load_textures::{load_texture_set, natural_material_tiled};
use crate::utils::objects::PreloadedTextures;
// TODO: Add these to the shared memory
use shared::constants::{
    lighting_constants::{GLOBAL_AMBIENT_LIGHT_INTENSITY, SPOTLIGHT_LIGHT_INTENSITY},
    object_constants::GROUND_Y,
};
use crate::shared_memory::shared_memory_reader::SharedMemResource;

/// Initial game scene, with the camera, ground, lights, and the pyramid.
/// Setup the persistent entitites across resets.
pub fn setup_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,

) {
    // Load marble texture
    let marble = load_texture_set(&asset_server, "textures/Rock024_1K-JPG/bevy_ready");

    // Ground Plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            ..natural_material_tiled(&marble, 8.0)
        })),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
    ));

    // Load metal texture
    let metal = load_texture_set(&asset_server, "textures/Tiles017_1K-JPG/bevy_ready");
    
    // Curved Background
    commands.spawn((
        Mesh3d(meshes.add(create_extended_semicircle_mesh(9.0, 10.0, 20.0, 64))),
        MeshMaterial3d(materials.add(natural_material_tiled(&metal, 8.0))),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
    ));

    commands.spawn((
        SpotLight {
            intensity: SPOTLIGHT_LIGHT_INTENSITY, // Default start value
            shadows_enabled: true,
            outer_angle: std::f32::consts::PI / 7.0,
            inner_angle: std::f32::consts::PI / 18.0,
            range: 35.0,
            radius: 0.0,
            ..default()
        },
        Transform::from_xyz(0.0, 15.0, 10.0).looking_at(Vec3::new(0.0, 2.0, 1.5), -Vec3 ::Y),
    ));

    // Ambient Light
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: GLOBAL_AMBIENT_LIGHT_INTENSITY, // Default start value
        affects_lightmapped_meshes: true,
    });
}

/// Setup a specific game trial.
/// This spawns the pyramid and resets the camera. All spawned entities are marked with GameEntity.
pub fn setup_round(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    preloaded: Res<PreloadedTextures>,
    mut camera_query: Query<&mut Transform, With<PersistentCamera>>,
    mut spotlight_query: Query<&mut SpotLight, (Without<HoleLight>, Without<GameEntity>)>,
    ambient_light: Option<ResMut<GlobalAmbientLight>>,
    shm_res: Option<Res<SharedMemResource>>,
    mut round_start: ResMut<crate::utils::objects::RoundStartTimestamp>,
    mut local_game_struct: ResMut<GameStateLocal>,
    mut door_win_entities: ResMut<DoorWinEntities>,
    time: Res<Time>,
) {
    // Read shared memory
    let Some(shm_res) = shm_res else {
        error!("Shared Memory not initialized in setup_round");
        return;
    };

    let shm = shm_res.0.get();

    // Set round start time
    round_start.0 = Some(Duration::from_secs(0));

    // Read control values from sh
    let gs_ctrl = &shm.game_structure_control;

    // Reset all fields of local game structure
    let gs_game = &mut local_game_struct.0;
    *gs_game = gs_ctrl.to_not_atomic();

    gs_game.win_time = 0;
    
    // Update all the game resoruces based on the new configuration
    let mut decoration_seeds = [0u64; 3];
    for i in 0..3 {
        decoration_seeds[i] = gs_game.decorations_seeds[i];
    }

    let main_intensity = f32::from_bits(gs_game.main_spotlight_intensity);
    let ambient_intensity = f32::from_bits(gs_game.ambient_brightness);
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
            f32::from_bits(gs_game.camera_x),
            f32::from_bits(gs_game.camera_y),
            f32::from_bits(gs_game.camera_z),
        )
        .looking_at(Vec3::ZERO, Vec3::Y);
    }

    let radius = f32::from_bits(gs_game.base_radius);
    let height = f32::from_bits(gs_game.height);
    let orient = f32::from_bits(gs_game.start_orient);

    let mut colors = [Color::WHITE; 3];
    for i in 0..3 {
        let r = f32::from_bits(gs_game.colors[i * 4 + 0]);
        let g = f32::from_bits(gs_game.colors[i * 4 + 1]);
        let b = f32::from_bits(gs_game.colors[i * 4 + 2]);
        let a = f32::from_bits(gs_game.colors[i * 4 + 3]);
        colors[i] = Color::srgba(r, g, b, a);
    }

    let mut decoration_counts = [0; 3];
    for i in 0..3 {
        decoration_counts[i] = gs_game.decorations_count[i];
    }

    let mut decoration_sizes = [0.0; 3];
    let mut decoration_shapes = [DecorationShape::Circle; 3];
    let mut decoration_thicknesses = [0.0; 3];
    let mut face_textures = [0u32; 3];
    let mut decoration_textures = [0u32; 3];
    let mut decoration_colors = [Color::WHITE; 3];
    for i in 0..3 {
        decoration_sizes[i] = f32::from_bits(gs_game.decorations_size[i]);
        decoration_shapes[i] = match gs_game.decorations_shape[i] {
            0 => DecorationShape::Circle,
            1 => DecorationShape::Square,
            2 => DecorationShape::Star,
            3 => DecorationShape::Triangle,
            _ => DecorationShape::Circle,
        };
        decoration_thicknesses[i] = f32::from_bits(gs_game.decorations_thickness[i]);
        face_textures[i] = gs_game.textures[i];
        decoration_textures[i] = gs_game.decorations_texture[i];
        let r = f32::from_bits(gs_game.decorations_color[i * 4 + 0]);
        let g = f32::from_bits(gs_game.decorations_color[i * 4 + 1]);
        let b = f32::from_bits(gs_game.decorations_color[i * 4 + 2]);
        let a = f32::from_bits(gs_game.decorations_color[i * 4 + 3]);
        decoration_colors[i] = Color::srgba(r, g, b, a);
    }

    // Read target door from shared memory
    let target_door = gs_game.target_door as usize;

    // Spawn the pyramid and capture winning door entities
    let (winning_light, winning_emissive) = spawn_pyramid(
        &mut commands,
        &mut meshes,
        &mut materials,
        &preloaded,
        decoration_seeds,
        radius,
        height,
        orient + std::f32::consts::FRAC_PI_6, // Correct alignment
        colors,
        decoration_counts,
        decoration_sizes,
        decoration_shapes,
        face_textures,
        decoration_colors,
        decoration_textures,
        decoration_thicknesses,
        target_door,
    );

    // Populate DoorWinEntities with the target door's entities and reset timer
    door_win_entities.winning_light = winning_light;
    door_win_entities.winning_emissive = winning_emissive;
    door_win_entities.animation_start_time = Some(time.elapsed());
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
        indices.push(base + 1);
        indices.push(base + 2);

        indices.push(base + 1);
        indices.push(base + 3);
        indices.push(base + 2);
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
