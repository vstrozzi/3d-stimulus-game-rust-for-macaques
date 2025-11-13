// This file contains the logic for spawning the pyramid and its decorations.
use crate::utils::constants::{
    object_constants::GROUND_Y, pyramid_constants::*,
};
use crate::utils::objects::{
    DecorationShape, FaceMarker, GameEntity, GameState, Pyramid, PyramidType, RandomGen,
};
use bevy::prelude::*;

use rand::{Rng, RngCore};
use rand_chacha::ChaCha8Rng;

/// Spawns a pyramid composed of three triangular faces.
pub fn spawn_pyramid(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    random_gen: &mut ResMut<RandomGen>,
    game_state: &mut GameState,
) {
    // Define the top vertex of the pyramid.
    let top = Vec3::new(0.0, game_state.pyramid_height, 0.0);
    // Build the symmetric triangular vertices for the base of the pyramid.
    let mut base_corners: [Vec3; 3] = [Vec3::ZERO; 3];
    let mut prev_xz = Vec2::new(
        game_state.pyramid_base_radius * game_state.pyramid_start_orientation_radius.cos(),
        game_state.pyramid_base_radius * game_state.pyramid_start_orientation_radius.sin(),
    );
    base_corners[0] = Vec3::new(prev_xz.x, GROUND_Y, prev_xz.y);
    // Compute constants for the rotation of the pyramid's base vertices.
    let pyramid_angle_increment_cos: f32 = PYRAMID_ANGLE_INCREMENT_RAD.cos();
    let pyramid_angle_increment_sin: f32 = PYRAMID_ANGLE_INCREMENT_RAD.sin();
    for i in 1..3 {
        // Construct a new face by rotating from the previous one on the 2D base-circle of the pyramid in the xz-plane.
        let x = prev_xz.x * pyramid_angle_increment_cos - prev_xz.y * pyramid_angle_increment_sin;
        let z = prev_xz.y * pyramid_angle_increment_cos + prev_xz.x * pyramid_angle_increment_sin;

        prev_xz = Vec2::new(x, z);
        // Save the new vertex.
        base_corners[i] = Vec3::new(prev_xz.x, GROUND_Y, prev_xz.y);
    }

    // Create the triangular face meshes independently.
    for i in 0..3 {
        let next = (i + 1) % 3;

        // Create a triangular mesh for the face.
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            Default::default(),
        );

        // Define the positions of the face-triangle's vertices.
        let positions = vec![
            top.to_array(), // Top vertex
            base_corners[i].to_array(),
            base_corners[next].to_array(),
        ];

        // Calculate the normal vector on the 2D plane of the face for lighting and shading.
        let v1 = base_corners[i] - top;
        let v2 = base_corners[next] - top;
        let normal = v1.cross(v2).normalize();

        // Save the normal of each vertex (they are the same).
        let normals = vec![normal.to_array(); 3];

        // Insert the positions, normals, and UVs for each vertex into the mesh.
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.5, 0.0], [0.0, 1.0], [1.0, 1.0]], // Defines how the texture is mapped to the triangular shape (flipped vertically).
        );

        // Spawn the face entity with its mesh, material, transform, and components.
        let face_entity = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: game_state.pyramid_color_faces[i],
                    cull_mode: None, // Disable backface culling to render both sides of the face.
                    double_sided: true,
                    ..default()
                })),
                Transform::default(),
                Pyramid,
                FaceMarker {
                    face_index: i,
                    color: game_state.pyramid_color_faces[i],
                    normal: if game_state.pyramid_type == PyramidType::Type1 {
                        normal
                    } else {
                        -normal
                    },
                },
                GameEntity,
            ))
            .id();

        // Spawn decorations on this face.
        spawn_face_decorations(
            commands,
            meshes,
            materials,
            &mut random_gen.random_gen,
            face_entity,
            top,
            base_corners[i],
            base_corners[next],
            normal,
        );
    }
}

/// Spawns decorative shapes on a pyramid face using a Poisson-like sampling method.
fn spawn_face_decorations(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    rng: &mut ChaCha8Rng,
    parent_face: Entity,
    top: Vec3,
    corner1: Vec3,
    corner2: Vec3,
    face_normal: Vec3,
) {
    // Determine the number of decorations to spawn.
    let decoration_count = rng.random_range(DECORATION_COUNT_MIN..=DECORATION_COUNT_MAX);

    // Store the generated decoration positions and sizes for overlap checking.
    let mut decorations: Vec<(Vec3, f32)> = Vec::new();

    // Set the maximum number of attempts to place each decoration before giving up.
    const MAX_PLACEMENT_ATTEMPTS: usize = 30;

    // Try to place the desired number of decorations.
    let mut successful_placements = 0;
    let mut total_attempts = 0;

    // Choose a random shape type, which will be the same for all decorations on this face.
    let shape = match rng.next_u64() % 4 {
        0 => DecorationShape::Circle,
        1 => DecorationShape::Square,
        2 => DecorationShape::Star,
        _ => DecorationShape::Triangle,
    };

    // Choose a random vibrant color, which will be the same for all decorations on this face.
    let color = Color::srgb(
        rng.random_range(0.2..1.0),
        rng.random_range(0.2..1.0),
        rng.random_range(0.2..1.0),
    );

    while successful_placements < decoration_count
        && total_attempts < decoration_count * MAX_PLACEMENT_ATTEMPTS
    {
        total_attempts += 1;

        // Choose a random size for the decoration.
        let size = rng.random_range(DECORATION_SIZE_MIN..DECORATION_SIZE_MAX);

        // Generate a random position using barycentric coordinates to ensure the point is inside the triangle.
        let (position, is_valid) =
            sample_point_in_triangle(rng, top, corner1, corner2, size, &decorations);

        // Skip this attempt if the position overlaps with existing decorations or is too close to the edges.
        if !is_valid {
            continue;
        }

        // Create a mesh based on the chosen shape.
        let mesh = create_decoration_mesh(shape, size);

        // Calculate the rotation to align the decoration with the face plane.
        // First, rotate from Z-up (the default for the mesh) to Y-up, then align Y-up to the face normal.
        let base_rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2); // Rotate 90 degrees to make the mesh face up in the Y direction.
        let face_rotation = Quat::from_rotation_arc(Vec3::Y, face_normal);
        let final_rotation = face_rotation * base_rotation;

        // Add a small random rotation around the normal for variety.
        let random_spin =
            Quat::from_axis_angle(face_normal, rng.random_range(0.0..std::f32::consts::TAU));
        let rotation = random_spin * final_rotation;

        // Offset the position slightly along the normal to prevent z-fighting with the face.
        let offset_position = position - face_normal * 0.001;

        // Spawn the decoration as a child of the face.
        commands.entity(parent_face).with_children(|parent| {
            parent.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color,
                    emissive: color.to_linear() * 0.3, // Add a slight glow.
                    ..default()
                })),
                Transform {
                    translation: offset_position,
                    rotation,
                    scale: Vec3::ONE,
                },
                GameEntity,
            ));
        });

        // Store this decoration's position and size for future collision checks.
        decorations.push((position, size));
        successful_placements += 1;
    }
}

/// Samples a random point inside a triangle using barycentric coordinates, with collision checking against existing decorations.
fn sample_point_in_triangle(
    rng: &mut ChaCha8Rng,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    size: f32,
    existing_decorations: &[(Vec3, f32)],
) -> (Vec3, bool) {
    // Generate random barycentric coordinates using the square root method for a uniform distribution.
    let r1 = rng.random_range(0.0..1.0_f32).sqrt();
    let r2 = rng.random_range(0.0..1.0_f32);

    // The barycentric weights ensure that the point is inside the triangle.
    let w0 = 1.0 - r1;
    let w1 = r1 * (1.0 - r2);
    let w2 = r1 * r2;

    // Calculate the 3D position of the point.
    let position = v0 * w0 + v1 * w1 + v2 * w2;

    // Set a minimum distance from the edges, proportional to the decoration's size.
    let edge_margin = size * 1.5;

    // Check if the point is too close to the triangle's edges.
    let dist_to_edge_01 = point_to_line_segment_distance(position, v0, v1);
    let dist_to_edge_12 = point_to_line_segment_distance(position, v1, v2);
    let dist_to_edge_20 = point_to_line_segment_distance(position, v2, v0);

    if dist_to_edge_01 < edge_margin
        || dist_to_edge_12 < edge_margin
        || dist_to_edge_20 < edge_margin
    {
        return (position, false);
    }

    // Check for overlap with existing decorations (Poisson disk constraint).
    let min_spacing = size * 2.0; // The minimum distance between decoration centers.

    for (existing_pos, existing_size) in existing_decorations {
        let distance = position.distance(*existing_pos);
        let required_distance = (size + existing_size) * 1.2; // Add 20% extra spacing.

        if distance < required_distance.max(min_spacing) {
            return (position, false);
        }
    }

    (position, true)
}

/// Calculates the minimum distance from a point to a line segment.
fn point_to_line_segment_distance(point: Vec3, line_start: Vec3, line_end: Vec3) -> f32 {
    let line_vec = line_end - line_start;
    let point_vec = point - line_start;
    let line_length_sq = line_vec.length_squared();

    if line_length_sq < 1e-6 {
        return point_vec.length();
    }

    // Project the point onto the line and clamp it to the segment.
    let t = (point_vec.dot(line_vec) / line_length_sq).clamp(0.0, 1.0);
    let projection = line_start + line_vec * t;

    point.distance(projection)
}

/// Creates a mesh for a decoration shape.
fn create_decoration_mesh(shape: DecorationShape, size: f32) -> Mesh {
    match shape {
        DecorationShape::Circle => Circle::new(size).mesh().resolution(16).build(),
        DecorationShape::Square => Rectangle::new(size * 2.0, size * 2.0).mesh().build(),
        DecorationShape::Star => create_star_mesh(size, 5),
        DecorationShape::Triangle => create_triangle_mesh(size),
    }
}

/// Creates a star-shaped mesh.
fn create_star_mesh(size: f32, points: usize) -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    );

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Add the center point of the star.
    positions.push([0.0, 0.0, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    uvs.push([0.5, 0.5]);

    // Create the points of the star.
    let angle_step = std::f32::consts::TAU / (points * 2) as f32;
    for i in 0..(points * 2) {
        let angle = i as f32 * angle_step;
        let radius = if i % 2 == 0 { size } else { size * 0.4 };
        let x = angle.cos() * radius;
        let y = angle.sin() * radius;

        positions.push([x, y, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([x / size * 0.5 + 0.5, y / size * 0.5 + 0.5]);
    }

    // Create the triangles of the star.
    for i in 1..=(points * 2) {
        let next = if i == points * 2 { 1 } else { i + 1 };
        indices.extend_from_slice(&[0, i as u32, next as u32]);
    }

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));

    mesh
}

/// Creates a triangle-shaped mesh.
fn create_triangle_mesh(size: f32) -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    );

    let height = size * 1.732; // sqrt(3)
    let positions = vec![
        [0.0, height * 0.666, 0.0],
        [-size, -height * 0.333, 0.0],
        [size, -height * 0.333, 0.0],
    ];

    let normals = vec![[0.0, 1.0, 0.0]; 3];
    let uvs = vec![[0.5, 1.0], [0.0, 0.0], [1.0, 0.0]];

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);

    mesh
}
