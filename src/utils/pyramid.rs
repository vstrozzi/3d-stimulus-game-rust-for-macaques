//! Logic for spawning the pyramid base with interactive doors.

use crate::utils::constants::{object_constants::GROUND_Y, pyramid_constants::*};
use crate::utils::objects::{
    Decoration, DecorationSet, DecorationShape, GameEntity, GameState, Pyramid,
    PyramidType, RandomGen, RotableComponent, BaseFrame, BaseDoor, HoleLight,
};
use bevy::prelude::*;

use rand::{Rng, RngCore};
use rand_chacha::ChaCha8Rng;


/// Spawns the wooden base with holes for the pyramid
pub fn spawn_pyramid_base(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    game_state: &mut GameState,
) {
    let base_radius = BASE_RADIUS;
    let angle_increment = std::f32::consts::TAU / BASE_NR_SIDES as f32;

    
    for i in 0..BASE_NR_SIDES {
        let angle1 = i as f32 * angle_increment + game_state.pyramid_start_orientation_rad + std::f32::consts::PI / 2.0;
        let angle2 = (i + 1) as f32 * angle_increment + game_state.pyramid_start_orientation_rad + std::f32::consts::PI / 2.0;
        
        // Calculate the four corners of the rectangular side
        let bottom_outer_1 = Vec3::new(
            base_radius * angle1.cos(),
            GROUND_Y,
            base_radius * angle1.sin(),
        );
        let bottom_outer_2 = Vec3::new(
            base_radius * angle2.cos(),
            GROUND_Y,
            base_radius * angle2.sin(),
        );
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
        
        // Create the frame mesh with a pentagonal hole
        let frame_mesh = create_frame_with_hole(
            bottom_outer_1,
            bottom_outer_2,
            top_outer_1,
            top_outer_2,
        );
        
        // Calculate the normal for the side
        let side_vec = bottom_outer_2 - bottom_outer_1;
        let up_vec = Vec3::Y;
        let normal = side_vec.cross(up_vec).normalize();
        
        // Calculate light position 
        let center = (bottom_outer_1 + bottom_outer_2 + top_outer_1 + top_outer_2) / 4.0;
        let light_pos = center - normal * BASE_HOLES_LIGHT_OFFSET_CENTER + Vec3::Y * BASE_HOLES_LIGHT_Y_OFFSET ;


        let right = normal.cross(Vec3::Y).normalize();  // Perpendicular to both normal and Y
        let up = right.cross(normal).normalize();       // Perpendicular to both right and look direction
        // Spawn the base frame and a light in front to have a nice effect
        commands
            .spawn((
                Mesh3d(meshes.add(frame_mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: BASE_COLOR,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                })),
                Transform::default(),
                BaseFrame {door_index: i },
                GameEntity,
                RotableComponent
            ))
            .with_children(|parent| {
                parent.spawn((
                    SpotLight {
                        intensity: 2_000_000.0,
                        shadows_enabled: true,
                        outer_angle: std::f32::consts::PI / 6.0,
                        ..default()
                    },
                    Transform::from_translation(light_pos).looking_at(light_pos - 2.0*normal, up),
                    GameEntity,
                    // Initially hidden
                    HoleLight,
                    Visibility::Hidden,
                ));
            });
        
        // Create and spawn the door (pentagon) that covers the hole
        let door_mesh = create_pentagon_door(
            bottom_outer_1,
            bottom_outer_2,
            top_outer_1,
            top_outer_2,
        );
        
        // Door color: slightly darker brown with a visible border effect
        // Alpha mode Blend to allow transparency changes
        let door_color = Color::srgba(0.49, 0.24, 0.00, 0.0);
        
        // Spawn the door entity
        commands.spawn((
            Mesh3d(meshes.add(door_mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: door_color,
                cull_mode: None,
                double_sided: false,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })),
            Transform::default(),
            BaseDoor {
                door_index: i,
                normal: normal,
                is_open: false,
            },
            GameEntity,
            RotableComponent,
        ));
        
    }

    // Spawn the top lid of the base
    let top_y = GROUND_Y + BASE_HEIGHT;
    
    // Create a polygon mesh matching the base's shape
    let top_lid_mesh = create_top_lid_mesh(base_radius, BASE_NR_SIDES, game_state.pyramid_start_orientation_rad);
    
    commands.spawn((
        Mesh3d(meshes.add(top_lid_mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: BASE_COLOR,
            cull_mode: None,
            double_sided: false,
            ..default()
        })),
        Transform::from_xyz(0.0, top_y, 0.0),
        RotableComponent, 
        GameEntity,
    ));
}


/// Creates a polygonal lid mesh for the top of the base
fn create_top_lid_mesh(radius: f32, sides: usize, start_orientation: f32) -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    );
    
    let angle_increment = std::f32::consts::TAU / sides as f32;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    
    // Center vertex
    positions.push([0.0, 0.0, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    uvs.push([0.5, 0.5]);
    
    // Create vertices around the perimeter
    for i in 0..sides {
        let angle = i as f32 * angle_increment + start_orientation + std::f32::consts::PI / 2.0;
        let x = radius * angle.cos();
        let z = radius * angle.sin();
        
        positions.push([x, 0.0, z]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([x / radius * 0.5 + 0.5, z / radius * 0.5 + 0.5]);
    }
    
    // Create triangles (fan triangulation from center)
    for i in 1..=sides {
        let next = if i == sides { 1 } else { i + 1 };
        indices.extend_from_slice(&[0, i as u32, next as u32]);
    }
    
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    
    mesh
}



/// Creates a rectangular frame mesh with a pentagonal hole cut out in the center
fn create_frame_with_hole(
    bottom_left: Vec3,
    bottom_right: Vec3,
    top_left: Vec3,
    top_right: Vec3,
) -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    );
    
    // Calculate the center of the rectangle
    let center = (bottom_left + bottom_right + top_left + top_right) / 4.0;
    
    // Calculate the width and height of the rectangle
    let width = bottom_left.distance(bottom_right);
    let height = bottom_left.distance(top_left);
    
    // Calculate the normal
    let side_vec = bottom_right - bottom_left;
    let up_vec = top_left - bottom_left;
    let normal = side_vec.cross(up_vec).normalize();
    
    // Create pentagon hole vertices (scaled down from center)
    let hole_scale = 0.4; // Pentagon is 40% of the panel size
    let pentagon_radius = (width.min(height) * hole_scale) / 2.0;
    
    // Pentagon vertices (5 points)
    let pentagon_points = 5;
    let pentagon_angle_offset = -std::f32::consts::FRAC_PI_2; // Start from top
    let mut pentagon_vertices = Vec::new();
    
    // Local coordinate system for the rectangle
    let local_right = (bottom_right - bottom_left).normalize();
    let local_up = (top_left - bottom_left).normalize();
    
    for i in 0..pentagon_points {
        let angle = (i as f32 * std::f32::consts::TAU / pentagon_points as f32) + pentagon_angle_offset;
        let x_offset = angle.cos() * pentagon_radius;
        let y_offset = angle.sin() * pentagon_radius;
        
        let vertex = center + local_right * x_offset + local_up * y_offset;
        pentagon_vertices.push(vertex);
    }
    
    // Build vertices: 4 outer corners + 5 pentagon vertices
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    
    // Outer rectangle vertices (0-3)
    positions.push(bottom_left.to_array());
    positions.push(bottom_right.to_array());
    positions.push(top_right.to_array());
    positions.push(top_left.to_array());
    
    // Pentagon hole vertices (4-8)
    for vertex in &pentagon_vertices {
        positions.push(vertex.to_array());
    }
    
    // All vertices share the same normal
    for _ in 0..positions.len() {
        normals.push(normal.to_array());
    }
    
    // Create triangles connecting the outer rectangle to the inner pentagon
    let mut indices = Vec::new();

    indices.extend_from_slice(&[1, 2, 5]); 
    indices.extend_from_slice(&[2, 6, 5]);

    indices.extend_from_slice(&[2, 3, 6]);
    indices.extend_from_slice(&[3, 7, 6]);


    indices.extend_from_slice(&[3, 0, 8]); // TL -> BL -> PentBL
    indices.extend_from_slice(&[3, 8, 7]); // TL -> PentBL -> PentTL

    indices.extend_from_slice(&[0, 4, 8]); 
    
    indices.extend_from_slice(&[0, 1, 4]);
    
    indices.extend_from_slice(&[1, 5, 4]);

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    
    mesh
}

/// Creates a pentagon door mesh that fits in the hole
fn create_pentagon_door(
    bottom_left: Vec3,
    bottom_right: Vec3,
    top_left: Vec3,
    top_right: Vec3,
) -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    );
    
    // Calculate the center of the rectangle
    let center = (bottom_left + bottom_right + top_left + top_right) / 4.0;
    
    // Calculate the width and height
    let width = bottom_left.distance(bottom_right);
    let height = bottom_left.distance(top_left);
    
    // Calculate the normal (same as create_frame_with_hole for consistency)
    let side_vec = bottom_right - bottom_left;
    let up_vec = top_left - bottom_left;
    let normal = side_vec.cross(up_vec).normalize();
    
    // Pentagon parameters (matching the hole)
    let hole_scale = 0.4;
    let pentagon_radius = (width.min(height) * hole_scale) / 2.0;
    
    // Slightly offset the door forward to prevent z-fighting
    let door_center = center + normal * 0.001;
    
    // Local coordinate system (same as create_frame_with_hole)
    let local_right = (bottom_right - bottom_left).normalize();
    let local_up = (top_left - bottom_left).normalize();
    
    // Create pentagon vertices
    let pentagon_points = 5;
    let pentagon_angle_offset = -std::f32::consts::FRAC_PI_2;
    
    let mut positions = Vec::new();
    let mut normals_vec = Vec::new();
    
    // Center vertex
    positions.push(door_center.to_array());
    normals_vec.push(normal.to_array());
    
    // Pentagon vertices
    for i in 0..pentagon_points {
        let angle = (i as f32 * std::f32::consts::TAU / pentagon_points as f32) + pentagon_angle_offset;
        let x_offset = angle.cos() * pentagon_radius;
        let y_offset = angle.sin() * pentagon_radius;
        
        let vertex = door_center + local_right * x_offset + local_up * y_offset;
        positions.push(vertex.to_array());
        normals_vec.push(normal.to_array());
    }
    
    // Create triangles (fan triangulation from center)
    let mut indices = Vec::new();
    for i in 1..=pentagon_points {
        let next = if i == pentagon_points { 1 } else { i + 1 };
        indices.extend_from_slice(&[0, i as u32, next as u32]);
    }
    
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals_vec);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    
    mesh
}

/// Spawns a pyramid composed with three triangular faces.
pub fn spawn_pyramid(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    random_gen: &mut ResMut<RandomGen>,
    game_state: &mut GameState,
) {
    let top_vex = Vec3::new(0.0, game_state.pyramid_height, 0.0);
    // Build the symmetric triangular vertices for the base of the pyramid.
    let mut base_corners: [Vec3; 3] = [Vec3::ZERO; 3];
    let mut prev_xz = Vec2::new(
        game_state.pyramid_base_radius * game_state.pyramid_start_orientation_rad.cos(),
        game_state.pyramid_base_radius * game_state.pyramid_start_orientation_rad.sin(),
    );
    base_corners[0] = Vec3::new(prev_xz.x, GROUND_Y + BASE_HEIGHT, prev_xz.y);
    // Compute constants for the rotation of the pyramid's base vertices.
    let pyramid_angle_increment_cos: f32 = PYRAMID_ANGLE_INCREMENT_RAD.cos();
    let pyramid_angle_increment_sin: f32 = PYRAMID_ANGLE_INCREMENT_RAD.sin();
    for i in 1..3 {
        // Construct a new face by rotating from the previous one on the 2D base-circle of the pyramid in the xz-plane.
        let x = prev_xz.x * pyramid_angle_increment_cos - prev_xz.y * pyramid_angle_increment_sin;
        let z = prev_xz.y * pyramid_angle_increment_cos + prev_xz.x * pyramid_angle_increment_sin;

        prev_xz = Vec2::new(x, z);
        // Save the new vertex.
        base_corners[i] = Vec3::new(prev_xz.x, GROUND_Y + BASE_HEIGHT, prev_xz.y);
    }

    // Generate decoration sets for each face.
    // For Type2 pyramids, we replicate one decoration set (similar to colors).
    let mut decoration_sets: [Option<DecorationSet>; 3] = [None, None, None];

    // Generate decoration sets for faces
    decoration_sets[0] = Some(generate_decoration_set(
        &mut random_gen.random_gen,
        top_vex,
        base_corners[0],
        base_corners[1],
    ));

    decoration_sets[1] = Some(generate_decoration_set(
        &mut random_gen.random_gen,
        top_vex,
        base_corners[1],
        base_corners[2],
    ));

    // For Type2 pyramids, replicate one decoration set to another face
    if game_state.pyramid_type == PyramidType::Type2 {
        decoration_sets[2] = decoration_sets[1].clone();
    } else {
        decoration_sets[2] = Some(generate_decoration_set(
            &mut random_gen.random_gen,
            top_vex,
            base_corners[2],
            base_corners[0],
        ));
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
            top_vex.to_array(), // Top vertex
            base_corners[i].to_array(),
            base_corners[next].to_array(),
        ];

        // Calculate the normal vector on the 2D plane of the face for lighting and shading.
        let v1 = base_corners[i] - top_vex;
        let v2 = base_corners[next] - top_vex;
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
                    double_sided: false ,
                    ..default()
                })),
                Transform::default(),
                Pyramid,
                RotableComponent, // Make it rotatable by camera controls
                GameEntity,
            ))
            .id();

        // Spawn decorations from the decoration set.
        if let Some(ref decoration_set) = decoration_sets[i] {
            spawn_decorations_from_set(
                commands,
                meshes,
                materials,
                face_entity,
                decoration_set,
                top_vex,
                base_corners[i],
                base_corners[next],
                normal,
            );
        }
    }

    // Spawn the base
    spawn_pyramid_base(commands, meshes, materials, game_state);
}


/// Generates a decoration set for a pyramid face using Poisson-like sampling.
/// Decorations are stored using barycentric coordinates relative to the triangle vertices.
fn generate_decoration_set(
    rng: &mut ChaCha8Rng,
    top: Vec3,
    corner1: Vec3,
    corner2: Vec3,
) -> DecorationSet {
    // Determine the number of decorations to generate.
    let decoration_count = rng.random_range(DECORATION_COUNT_MIN..=DECORATION_COUNT_MAX);

    // Store the generated decoration positions (in world space) for overlap checking.
    let mut decorations_world: Vec<(Vec3, f32)> = Vec::new();
    // Store the final decorations with barycentric coordinates.
    let mut decorations: Vec<Decoration> = Vec::new();

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
        rng.random_range(0.2..0.22),
        rng.random_range(0.2..0.22),
        rng.random_range(0.2..0.22),
    );

    while successful_placements < decoration_count
        && total_attempts < decoration_count * MAX_PLACEMENT_ATTEMPTS
    {
        total_attempts += 1;

        // Choose a random size for the decoration.
        let size = rng.random_range(DECORATION_SIZE_MIN..DECORATION_SIZE_MAX);

        // Generate a random position using barycentric coordinates to ensure the point is inside the triangle.
        let (world_position, is_valid) =
            sample_point_in_triangle(rng, top, corner1, corner2, size, &decorations_world);

        // Skip this attempt if the position overlaps with existing decorations or is too close to the edges.
        if !is_valid {
            continue;
        }

        // Convert world position to barycentric coordinates
        // world_position = w0*top + w1*corner1 + w2*corner2
        // where w0 + w1 + w2 = 1
        let v0 = corner1 - top;
        let v1 = corner2 - top;
        let v2 = world_position - top;

        let d00 = v0.dot(v0);
        let d01 = v0.dot(v1);
        let d11 = v1.dot(v1);
        let d20 = v2.dot(v0);
        let d21 = v2.dot(v1);

        let denom = d00 * d11 - d01 * d01;
        let w1 = (d11 * d20 - d01 * d21) / denom;
        let w2 = (d00 * d21 - d01 * d20) / denom;
        let w0 = 1.0 - w1 - w2;

        // Store this decoration with barycentric coordinates
        decorations.push(Decoration {
            barycentric: Vec3::new(w0, w1, w2),
            size,
        });
        decorations_world.push((world_position, size));
        successful_placements += 1;
    }

    DecorationSet {
        shape,
        color,
        decorations,
    }
}

/// Spawns decorations from a decoration set onto a face
/// Reconstructs world positions from barycentric coordinates relative to the given triangle vertices
fn spawn_decorations_from_set(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    parent_face: Entity,
    decoration_set: &DecorationSet,
    top: Vec3,
    corner1: Vec3,
    corner2: Vec3,
    face_normal: Vec3,
) {
    for decoration in &decoration_set.decorations {
        // Reconstruct world position from barycentric coordinates
        let position = decoration.barycentric.x * top
            + decoration.barycentric.y * corner1
            + decoration.barycentric.z * corner2;

        let mesh = create_decoration_mesh(decoration_set.shape, decoration.size);

        // Calculate the rotation to align the decoration with the face plane
        let base_rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let normal_rotation = Quat::from_rotation_arc(Vec3::Y, face_normal);
        let final_rotation = normal_rotation * base_rotation;

        // Offset slightly away from face surface to prevent z-fighting
        let offset_position = position - face_normal* 0.0001;
        

        // Spawn the decoration as a child of the face
        commands.entity(parent_face).with_children(|parent| {
            parent.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: decoration_set.color,
                    reflectance: 0.0,
                    ..default()
                })),
                Transform {
                    translation: offset_position,
                    rotation: final_rotation,
                    scale: Vec3::ONE,
                },
                GameEntity,
            ));
        });
    }
}

/// Samples a random point inside a triangle using barycentric coordinates, with collision checking against existing decorations
fn sample_point_in_triangle(
    rng: &mut ChaCha8Rng,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    size: f32,
    existing_decorations: &[(Vec3, f32)],
) -> (Vec3, bool) {
    // Generate random barycentric coordinates using the square root method for a uniform distribution
    let r1 = rng.random_range(0.0..1.0_f32).sqrt();
    let r2 = rng.random_range(0.0..1.0_f32);

    // The barycentric weights ensure that the point is inside the triangle
    let w0 = 1.0 - r1;
    let w1 = r1 * (1.0 - r2);
    let w2 = r1 * r2;

    // Calculate the 3D position of the point
    let position = v0 * w0 + v1 * w1 + v2 * w2;

    // Set a minimum distance from the edges, proportional to the decoration's size
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

    // Check for overlap with existing decorations (Poisson disk constraint)
    let min_spacing = size * 2.0; // The minimum distance between decoration centers

    for (existing_pos, existing_size) in existing_decorations {
        let distance = position.distance(*existing_pos);
        let required_distance = (size + existing_size) * 1.2; // Add 20% extra spacing.

        if distance < required_distance.max(min_spacing) {
            return (position, false);
        }
    }

    (position, true)
}

/// Calculates the minimum distance from a point to a line segment
fn point_to_line_segment_distance(point: Vec3, line_start: Vec3, line_end: Vec3) -> f32 {
    let line_vec = line_end - line_start;
    let point_vec = point - line_start;
    let line_length_sq = line_vec.length_squared();

    if line_length_sq < 1e-6 {
        return point_vec.length();
    }

    // Project the point onto the line and clamp it to the segment
    let t = (point_vec.dot(line_vec) / line_length_sq).clamp(0.0, 1.0);
    let projection = line_start + line_vec * t;

    point.distance(projection)
}

/// Creates a mesh for a decoration shape
fn create_decoration_mesh(shape: DecorationShape, size: f32) -> Mesh {
    match shape {
        DecorationShape::Circle => Circle::new(size).mesh().resolution(16).build(),
        DecorationShape::Square => Rectangle::new(size * 2.0, size * 2.0).mesh().build(),
        DecorationShape::Star => create_star_mesh(size, 5),
        DecorationShape::Triangle => create_triangle_mesh(size),
    }
}

/// Creates a star-shaped mesh
fn create_star_mesh(size: f32, points: usize) -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    );

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Add the center point of the star
    positions.push([0.0, 0.0, 0.0]);
    normals.push([0.0, 0.0, 1.0]);
    uvs.push([0.5, 0.5]);

    // Create the points of the star
    let angle_step = std::f32::consts::TAU / (points * 2) as f32;
    for i in 0..(points * 2) {
        let angle = i as f32 * angle_step;
        let radius = if i % 2 == 0 { size } else { size * 0.4 };
        let x = angle.cos() * radius;
        let y = angle.sin() * radius;

        positions.push([x, y, 0.0]);
        normals.push([0.0, 0.0, 1.0]);
        uvs.push([x / size * 0.5 + 0.5, y / size * 0.5 + 0.5]);
    }

    // Create the triangles of the star
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

/// Creates a triangle-shaped mesh
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

    let normals = vec![[0.0, 0.0, 1.0]; 3];
    let uvs = vec![[0.5, 1.0], [0.0, 0.0], [1.0, 0.0]];

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);

    mesh
}
