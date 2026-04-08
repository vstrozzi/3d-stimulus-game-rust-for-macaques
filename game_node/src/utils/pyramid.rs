//! Logic for spawning the pyramid base with interactive doors.

use crate::utils::load_textures::{natural_material, tinted_material};
use crate::utils::objects::{
    BaseDoor, BaseFrame, DecorationSet, GameEntity, HoleEmissive, HoleLight, Pyramid,
    PyramidConfig, RotableComponent, PreloadedTextures,
};
use crate::utils::decorations::{generate_decoration_set, spawn_decorations_from_set};
use crate::utils::utils::build_mesh;
use bevy::prelude::*;
use shared::{Texture};
use shared::constants::{object_constants::GROUND_Y, pyramid_constants::*};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Creates a pentagon mesh for the hole emissive effect.
fn create_pentagon_mesh(
    center: Vec3,
    radius: f32,
    local_right: Vec3,
    local_up: Vec3,
    normal: Vec3,
) -> Mesh {
    const N: usize = 5;
    const ANGLE_OFFSET: f32 = -std::f32::consts::FRAC_PI_2;

    let mut positions = Vec::with_capacity(N + 1);
    let mut normals_vec = Vec::with_capacity(N + 1);
    let mut uvs = Vec::with_capacity(N + 1);
    let mut indices = Vec::with_capacity(N * 3);

    positions.push(center.to_array());
    normals_vec.push(normal.to_array());
    uvs.push([0.5, 0.5]);

    for i in 0..N {
        let angle = (i as f32 * std::f32::consts::TAU / N as f32) + ANGLE_OFFSET;
        let x_offset = angle.cos() * radius;
        let y_offset = angle.sin() * radius;
        let vertex = center + local_right * x_offset + local_up * y_offset;
        positions.push(vertex.to_array());
        normals_vec.push(normal.to_array());
        uvs.push([x_offset / radius * 0.5 + 0.5, y_offset / radius * 0.5 + 0.5]);
    }

    for i in 1..=N {
        let next = if i == N { 1 } else { i + 1 };
        indices.extend_from_slice(&[0, next as u32, i as u32]);
    }

    build_mesh(positions, normals_vec, uvs, indices)
}

/// Creates a polygonal lid mesh for the top of the base
fn create_top_lid_mesh(radius: f32, sides: usize, start_orientation: f32) -> Mesh {
    let angle_increment = std::f32::consts::TAU / sides as f32;
    let mut positions = vec![[0.0f32, 0.0, 0.0]];
    let mut normals = vec![[0.0f32, 1.0, 0.0]];
    let mut uvs = vec![[0.5f32, 0.5]];
    let mut indices = Vec::new();

    for i in 0..sides {
        let angle = i as f32 * angle_increment + start_orientation + std::f32::consts::PI / 2.0;
        let x = radius * angle.cos();
        let z = radius * angle.sin();
        positions.push([x, 0.0, z]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([x / radius * 0.5 + 0.5, z / radius * 0.5 + 0.5]);
    }

    for i in 1..=sides {
        let next = if i == sides { 1 } else { i + 1 };
        indices.extend_from_slice(&[0, next as u32, i as u32]);
    }

    build_mesh(positions, normals, uvs, indices)
}

/// Creates a rectangular frame mesh with a pentagonal hole cut out in the center
/// Returns `(mesh, normal, local_right, local_up, center, pentagon_radius)`
fn create_frame_with_hole(
    bottom_left: Vec3,
    bottom_right: Vec3,
    top_left: Vec3,
    top_right: Vec3,
) -> (Mesh, Vec3, Vec3, Vec3, Vec3, f32) {
    let center = (bottom_left + bottom_right + top_left + top_right) / 4.0;
    let width = bottom_left.distance(bottom_right);
    let height = bottom_left.distance(top_left);

    let side_vec = bottom_right - bottom_left;
    let up_vec = top_left - bottom_left;
    let normal = -side_vec.cross(up_vec).normalize();

    const HOLE_SCALE: f32 = 0.4;
    let pentagon_radius = (width.min(height) * HOLE_SCALE) / 2.0;

    const PENTAGON_POINTS: usize = 5;
    const PENTAGON_ANGLE_OFFSET: f32 = -std::f32::consts::FRAC_PI_2;
    let local_right = (bottom_right - bottom_left).normalize();
    let local_up = (top_left - bottom_left).normalize();

    let mut pentagon_vertices = Vec::new();
    for i in 0..PENTAGON_POINTS {
        let angle =
            (i as f32 * std::f32::consts::TAU / PENTAGON_POINTS as f32) + PENTAGON_ANGLE_OFFSET;
        let vertex = center
            + local_right * (angle.cos() * pentagon_radius)
            + local_up * (angle.sin() * pentagon_radius);
        pentagon_vertices.push(vertex);
    }

    // 4 outer corners (0–3) + 5 pentagon vertices (4–8)
    let mut positions = vec![
        bottom_left.to_array(),
        bottom_right.to_array(),
        top_right.to_array(),
        top_left.to_array(),
    ];
    for v in &pentagon_vertices {
        positions.push(v.to_array());
    }

    let normals = vec![normal.to_array(); positions.len()];

    let mut uvs = vec![
        [0.0f32, 0.0], // bottom_left
        [1.0, 0.0],    // bottom_right
        [1.0, 1.0],    // top_right
        [0.0, 1.0],    // top_left
    ];
    for vertex in &pentagon_vertices {
        let diff = *vertex - center;
        uvs.push([
            0.5 + diff.dot(local_right) / width,
            0.5 + diff.dot(local_up) / height,
        ]);
    }

    #[rustfmt::skip]
    let indices: Vec<u32> = vec![
        1, 5, 2,  2, 5, 6,
        2, 6, 3,  3, 6, 7,
        3, 8, 0,  3, 7, 8,
        0, 8, 4,
        0, 4, 1,
        1, 4, 5,
    ];

    let mesh = build_mesh(positions, normals, uvs, indices);
    (mesh, normal, local_right, local_up, center, pentagon_radius)
}

/// Spawns the wooden base with pentagonal holes for the pyramid.
/// Returns `(winning_light, winning_emissive)` for the target door.
pub fn spawn_pyramid_base(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    preloaded: &PreloadedTextures,
    p_start_orientation_rad: f32,
    target_door: usize,
) -> (Option<Entity>, Option<Entity>) {
    let base_radius = BASE_RADIUS;
    let angle_increment = std::f32::consts::TAU / BASE_NR_SIDES as f32;

    let mut winning_light: Option<Entity> = None;
    let mut winning_emissive: Option<Entity> = None;

    for i in 0..BASE_NR_SIDES {
        let angle1 = i as f32 * angle_increment
            + p_start_orientation_rad
            + std::f32::consts::PI / 2.0;
        let angle2 = (i + 1) as f32 * angle_increment
            + p_start_orientation_rad
            + std::f32::consts::PI / 2.0;

        let bottom_outer_1 =
            Vec3::new(base_radius * angle1.cos(), GROUND_Y, base_radius * angle1.sin());
        let bottom_outer_2 =
            Vec3::new(base_radius * angle2.cos(), GROUND_Y, base_radius * angle2.sin());
        let top_outer_1 = Vec3::new(
            base_radius * angle1.cos(),
            GROUND_Y + BASE_HEIGHT,
            base_radius * angle1.sin(),
        );
        let top_outer_2 = Vec3::new(
            base_radius * angle2.cos(),
            GROUND_Y + BASE_HEIGHT,
            base_radius * angle2.sin(),
        );

        let (frame_mesh, normal, local_right, local_up, center, pentagon_radius) =
            create_frame_with_hole(bottom_outer_1, bottom_outer_2, top_outer_1, top_outer_2);

        let pentagon_center_inset = center + normal * 0.01;
        let pentagon_mesh =
            create_pentagon_mesh(pentagon_center_inset, pentagon_radius, local_right, local_up, normal);

        let is_target = i == target_door;
        let wood = preloaded.get(Texture::WoodFloor057_1K);

        let frame_id = commands
            .spawn((
                Mesh3d(meshes.add(frame_mesh)),
                MeshMaterial3d(materials.add(natural_material(&wood))),
                Transform::default(),
                BaseFrame { door_index: i },
                GameEntity,
                RotableComponent,
            ))
            .id();

        let emissive_id = commands
            .spawn((
                Mesh3d(meshes.add(pentagon_mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    emissive: LinearRgba::new(0.0, 0.0, 0.0, 1.0),
                    cull_mode: None,
                    ..default()
                })),
                Transform::default(),
                HoleEmissive,
                GameEntity,
                Visibility::Hidden,
                ChildOf(frame_id),
            ))
            .id();

        let light_id = commands
            .spawn((
                SpotLight {
                    intensity: 0.0,
                    shadows_enabled: true,
                    inner_angle: std::f32::consts::PI / 6.0,
                    outer_angle: std::f32::consts::PI / 4.0,
                    range: 25.0,
                    radius: 0.5,
                    ..default()
                },
                GameEntity,
                HoleLight,
                Visibility::Hidden,
                Transform::from_translation(center)
                    .looking_at(center + 2.0 * normal, Vec3::Y),
                ChildOf(frame_id),
            ))
            .id();

        if is_target {
            winning_light = Some(light_id);
            winning_emissive = Some(emissive_id);
        }

        commands.spawn((
            Transform::default(),
            BaseDoor {
                door_index: i,
                normal: -normal,
                is_open: false,
            },
            GameEntity,
            RotableComponent,
        ));
    }

    let top_y = GROUND_Y + BASE_HEIGHT;
    let top_lid_mesh = create_top_lid_mesh(base_radius, BASE_NR_SIDES, p_start_orientation_rad);
    let wood = preloaded.get(Texture::WoodFloor057_1K);

    commands.spawn((
        Mesh3d(meshes.add(top_lid_mesh)),
        MeshMaterial3d(materials.add(natural_material(wood))),
        Transform::from_xyz(0.0, top_y, 0.0),
        RotableComponent,
        GameEntity,
    ));

    (winning_light, winning_emissive)
}

/// Spawns a triangular prism pyramid with textured faces, decorations, and a base
pub fn spawn_pyramid(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    preloaded: &PreloadedTextures,
    config: &PyramidConfig,
) -> (Option<Entity>, Option<Entity>) {
    // Build the 3 base and top corners by rotating an initial point by 120° steps.
    let mut base_corners = [Vec3::ZERO; 3];
    let mut top_corners = [Vec3::ZERO; 3];

    let mut prev_xz = Vec2::new(
        config.radius * config.orientation_rad.cos(),
        config.radius * config.orientation_rad.sin(),
    );
    base_corners[0] = Vec3::new(prev_xz.x, GROUND_Y + BASE_HEIGHT, prev_xz.y);
    top_corners[0] = Vec3::new(prev_xz.x, config.height, prev_xz.y);

    let cos_inc = PYRAMID_ANGLE_INCREMENT_RAD.cos();
    let sin_inc = PYRAMID_ANGLE_INCREMENT_RAD.sin();
    for i in 1..3 {
        let x = prev_xz.x * cos_inc - prev_xz.y * sin_inc;
        let z = prev_xz.y * cos_inc + prev_xz.x * sin_inc;
        prev_xz = Vec2::new(x, z);
        base_corners[i] = Vec3::new(prev_xz.x, GROUND_Y + BASE_HEIGHT, prev_xz.y);
        top_corners[i] = Vec3::new(prev_xz.x, config.height, prev_xz.y);
    }

    // Top cap (flat triangle pointing up)
    let top_mesh = build_mesh(
        vec![
            top_corners[0].to_array(),
            top_corners[1].to_array(),
            top_corners[2].to_array(),
        ],
        vec![[0.0, 1.0, 0.0]; 3],
        vec![[0.5, 0.0], [0.0, 1.0], [1.0, 1.0]],
        vec![],
    );
    commands.spawn((
        Mesh3d(meshes.add(top_mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            cull_mode: None,
            double_sided: true,
            ..default()
        })),
        Transform::default(),
        Pyramid,
        RotableComponent,
        GameEntity,
    ));

    // Generate decoration sets for all 3 faces (2 triangles each = 6 sets total)
    let mut dec_sets: Vec<Option<DecorationSet>> = Vec::with_capacity(6);
    for i in 0..3 {
        let mut face_rng = ChaCha8Rng::seed_from_u64(config.decoration_seeds[i]);
        let next = (i + 1) % 3;
        let (tl, tr, bl, br) = (top_corners[i], top_corners[next], base_corners[i], base_corners[next]);

        dec_sets.push(Some(generate_decoration_set(
            &mut face_rng, tl, bl, br,
            config.decoration_counts[i], config.decoration_sizes[i],
            config.decoration_shapes[i], config.decoration_colors[i],
            config.decoration_thicknesses[i],
        )));
        dec_sets.push(Some(generate_decoration_set(
            &mut face_rng, tl, br, tr,
            config.decoration_counts[i], config.decoration_sizes[i],
            config.decoration_shapes[i], config.decoration_colors[i],
            config.decoration_thicknesses[i],
        )));
    }

    // Spawn faces and attach decorations
    for i in 0..3 {
        let next = (i + 1) % 3;
        let (tl, tr, bl, br) = (top_corners[i], top_corners[next], base_corners[i], base_corners[next]);

        let normal = -(bl - tl).cross(tr - tl).normalize();
        let mesh = build_mesh(
            vec![tl.to_array(), bl.to_array(), br.to_array(), tr.to_array()],
            vec![normal.to_array(); 4],
            vec![[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            vec![0, 2, 1, 0, 3, 2],
        );

        let face_tex = preloaded.get(Texture::from_u32(config.face_textures[i]));
        let face_entity = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    ..tinted_material(&face_tex, config.colors[i])
                })),
                Transform::default(),
                Pyramid,
                RotableComponent,
                GameEntity,
            ))
            .id();

        if let Some(ref set_a) = dec_sets[i * 2] {
            spawn_decorations_from_set(
                commands, meshes, materials, face_entity, set_a, preloaded,
                tl, bl, br, normal, config.decoration_textures[i],
            );
        }
        if let Some(ref set_b) = dec_sets[i * 2 + 1] {
            spawn_decorations_from_set(
                commands, meshes, materials, face_entity, set_b, preloaded,
                tl, br, tr, normal, config.decoration_textures[i],
            );
        }
    }

    spawn_pyramid_base(
        commands, meshes, materials, preloaded,
        config.orientation_rad, config.target_door,
    )
}

