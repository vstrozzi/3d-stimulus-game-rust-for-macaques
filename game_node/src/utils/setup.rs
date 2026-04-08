//! Setup logic for the monkey_3d_game, with main setup plugin and functions for initializing the game scene and state.
use std::time::Duration;

use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;

use shared::DecorationShape;
use crate::utils::objects::*;
use crate::utils::pyramid::spawn_pyramid;
use crate::utils::load_textures::{load_texture_set, natural_material_tiled};
use crate::utils::objects::PreloadedTextures;
use shared::constants::{
    lighting_constants::{GLOBAL_AMBIENT_LIGHT_INTENSITY, SPOTLIGHT_LIGHT_INTENSITY},
    object_constants::GROUND_Y,
};
use crate::shared_memory::shared_memory_reader::SharedMemResource;

/// Initial game scene, with the camera, ground, lights, and the pyramid.
/// Setup the persistent entities across resets
pub fn setup_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let marble = load_texture_set(&asset_server, "textures/Rock024_1K-JPG/bevy_ready");
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            ..natural_material_tiled(&marble, 8.0)
        })),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
    ));

    let metal = load_texture_set(&asset_server, "textures/Tiles017_1K-JPG/bevy_ready");
    commands.spawn((
        Mesh3d(meshes.add(create_extended_semicircle_mesh(9.0, 30.0, 20.0, 64))),
        MeshMaterial3d(materials.add(natural_material_tiled(&metal, 8.0))),
        Transform::from_xyz(0.0, GROUND_Y, 0.0),
    ));

    commands.spawn((
        SpotLight {
            intensity: SPOTLIGHT_LIGHT_INTENSITY,
            shadows_enabled: true,
            outer_angle: std::f32::consts::PI / 7.0,
            inner_angle: std::f32::consts::PI / 18.0,
            range: 35.0,
            radius: 0.0,
            ..default()
        },
        Transform::from_xyz(0.0, 15.0, 10.0).looking_at(Vec3::new(0.0, 2.0, 1.5), -Vec3::Y),
    ));

    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: GLOBAL_AMBIENT_LIGHT_INTENSITY,
        affects_lightmapped_meshes: true,
    });
}

/// Setup a specific game trial.
/// Despawns all game entities, resets the camera, and spawns a fresh pyramid
pub fn setup_round(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    preloaded: Res<PreloadedTextures>,
    mut camera_query: Query<&mut Transform, With<PersistentCamera>>,
    mut spotlight_query: Query<&mut SpotLight, (Without<HoleLight>, Without<GameEntity>)>,
    ambient_light: Option<ResMut<GlobalAmbientLight>>,
    shm_res: Option<Res<SharedMemResource>>,
    mut round_start: ResMut<RoundStartTimestamp>,
    mut local_game_struct: ResMut<GameStateLocal>,
    mut door_win_entities: ResMut<DoorWinEntities>,
    time: Res<Time>,
) {
    let Some(shm_res) = shm_res else {
        error!("Shared Memory not initialized in setup_round");
        return;
    };

    let shm = shm_res.0.get();
    round_start.0 = Some(Duration::from_secs(0));

    let gs_game = &mut local_game_struct.0;
    *gs_game = shm.game_structure_control.to_not_atomic();
    gs_game.win_time = 0;

    for mut spot in spotlight_query.iter_mut() {
        spot.intensity = f32::from_bits(gs_game.main_spotlight_intensity);
    }
    if let Some(mut ambient) = ambient_light {
        ambient.brightness = f32::from_bits(gs_game.ambient_brightness);
    }
    if let Ok(mut cam) = camera_query.single_mut() {
        *cam = Transform::from_xyz(
            f32::from_bits(gs_game.camera_x),
            f32::from_bits(gs_game.camera_y),
            f32::from_bits(gs_game.camera_z),
        )
        .looking_at(Vec3::ZERO, Vec3::Y);
    }

    let config = build_pyramid_config(gs_game);
    let (winning_light, winning_emissive) =
        spawn_pyramid(&mut commands, &mut meshes, &mut materials, &preloaded, &config);

    door_win_entities.winning_light = winning_light;
    door_win_entities.winning_emissive = winning_emissive;
    door_win_entities.animation_start_time = Some(time.elapsed());
}

/// Constructs a `PyramidConfig` from the current local game state
fn build_pyramid_config(gs_game: &shared::SharedGameStateLocal) -> PyramidConfig {
    PyramidConfig {
        decoration_seeds: gs_game.decorations_seeds,
        radius: f32::from_bits(gs_game.base_radius),
        height: f32::from_bits(gs_game.height),
        orientation_rad: f32::from_bits(gs_game.start_orient) + std::f32::consts::FRAC_PI_6,
        colors: std::array::from_fn(|i| read_color_bits(&gs_game.colors, i)),
        decoration_counts: gs_game.decorations_count,
        decoration_sizes: std::array::from_fn(|i| f32::from_bits(gs_game.decorations_size[i])),
        decoration_shapes: std::array::from_fn(|i| match gs_game.decorations_shape[i] {
            1 => DecorationShape::Square,
            2 => DecorationShape::Star,
            3 => DecorationShape::Triangle,
            _ => DecorationShape::Circle,
        }),
        face_textures: gs_game.textures,
        decoration_colors: std::array::from_fn(|i| read_color_bits(&gs_game.decorations_color, i)),
        decoration_textures: gs_game.decorations_texture,
        decoration_thicknesses: std::array::from_fn(|i| {
            f32::from_bits(gs_game.decorations_thickness[i])
        }),
        target_door: gs_game.target_door as usize,
    }
}

/// Reads a `Color::srgba` from a flat `&[u32]` array encoded as `f32` bits,
/// using stride-4 layout: `arr[i*4 .. i*4+3]` = (r, g, b, a)
fn read_color_bits(arr: &[u32], i: usize) -> Color {
    Color::srgba(
        f32::from_bits(arr[i * 4]),
        f32::from_bits(arr[i * 4 + 1]),
        f32::from_bits(arr[i * 4 + 2]),
        f32::from_bits(arr[i * 4 + 3]),
    )
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

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
