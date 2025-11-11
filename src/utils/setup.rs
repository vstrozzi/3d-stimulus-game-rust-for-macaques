use bevy::prelude::*;

use crate::log;
use crate::utils::constants::{
    camera_3d_constants::{CAMERA_3D_INITIAL_X, CAMERA_3D_INITIAL_Y, CAMERA_3D_INITIAL_Z},
    object_constants::GROUND_Y,
    pyramid_constants::*,
    game_constants::{
        SEED
    },
};
use crate::utils::objects::{FaceMarker, GameEntity, GameState, Pyramid, PyramidType, DecorationShape};

use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;


/// Plugin for handling setup
pub struct SetupPlugin;

impl Plugin for SetupPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, crate::utils::setup::setup);
    }
}

/// Setup the scene
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        // Start at fixed position looking at the origin
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

    // Light
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

    // Initialize game state
    let mut game_state = setup_game_state(&mut commands, &time);
    // Spawn Pyramid by borrowing commands, meshes, materials
    spawn_pyramid(&mut commands, &mut meshes, &mut materials, &mut game_state);

    log!("🎮 Pyramid Game Started!");
}

// Initialize game state resource with random values
pub fn setup_game_state(commands: &mut Commands, time: &Res<Time>) -> GameState {
    // Create Random Structure
    #[allow(unused_assignments)]
    let mut random_seed = 0;
    unsafe{ 
        SEED += 1;
        random_seed = SEED;
    }
    let mut random_gen = ChaCha8Rng::seed_from_u64(random_seed);

    // Define the random pyramid parameters
    let pyramid_type = if random_gen.next_u64() % 2 == 0 { PyramidType::Type1 } else { PyramidType::Type2 };
    let pyramid_base_radius = random_gen.random_range(
        PYRAMID_BASE_RADIUS_MIN..=PYRAMID_BASE_RADIUS_MAX,
    );
    let pyramid_height = random_gen.random_range(
        PYRAMID_HEIGHT_MIN..=PYRAMID_HEIGHT_MAX,
    );

    let pyramid_start_orientation_radius = random_gen.random_range(PYRAMID_ANGLE_OFFSET_RAD_MIN..PYRAMID_ANGLE_OFFSET_RAD_MAX);
    let pyramid_target_face_index = 0;

    let mut pyramid_colors =  PYRAMID_COLORS;
    // Set same colors for two sides if Type2
    if pyramid_type == PyramidType::Type2 {
        if random_gen.next_u64() % 2 == 0 {
            pyramid_colors[1] = pyramid_colors[2];
        } else {
            pyramid_colors[2] = pyramid_colors[1];
        }
    }
    let game_state = GameState {
        random_seed: random_seed,
        random_gen: Some(random_gen),
        pyramid_type: pyramid_type,
        pyramid_base_radius: pyramid_base_radius,
        pyramid_height: pyramid_height,
        pyramid_target_face_index: pyramid_target_face_index as usize,
        pyramid_start_orientation_radius: pyramid_start_orientation_radius,
        pyramid_color_faces: pyramid_colors,
        
        is_playing: true,
        is_started: false,
        is_won: false,
        is_changed: true,

        start_time: Some(time.elapsed()),
        end_time: None,

        attempts: 0,
        cosine_alignment: None,
    };


    // Initialize game state
    let cloned_game_state = game_state.clone();
    commands.insert_resource(game_state);

    return cloned_game_state;
    
}

/// Spawn a pyramid composed of 3 triangular faces
pub fn spawn_pyramid(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    game_state: &mut GameState,
) {
    // Define top vertex for pyramid
    let top = Vec3::new(0.0, game_state.pyramid_height, 0.0);
    // Build symmetric triangular vertices for base
    let mut base_corners: [Vec3; 3] = [Vec3::ZERO; 3];
    let mut prev_xz = Vec2::new(
        game_state.pyramid_base_radius * game_state.pyramid_start_orientation_radius.cos(),
        game_state.pyramid_base_radius * game_state.pyramid_start_orientation_radius.sin(),
    );
    base_corners[0] = Vec3::new(prev_xz.x, GROUND_Y, prev_xz.y);
    // Compute constants
    let pyramid_angle_increment_cos: f32 = PYRAMID_ANGLE_INCREMENT_RAD.cos();
    let pyramid_angle_increment_sin: f32 = PYRAMID_ANGLE_INCREMENT_RAD.sin();
    for i in 1..3 {
        // Construct new face by rotating from previous on 2D base-circle of pyramid in xz-plane
        let x = prev_xz.x * pyramid_angle_increment_cos - prev_xz.y * pyramid_angle_increment_sin;
        let z = prev_xz.y * pyramid_angle_increment_cos + prev_xz.x * pyramid_angle_increment_sin;

        prev_xz = Vec2::new(x, z);
        // Save new vertex
        base_corners[i] = Vec3::new(prev_xz.x, GROUND_Y, prev_xz.y);
    }

    // Create triangular faces meshes independently
    for i in 0..3 {
        let next = (i + 1) % 3;

        // Create triangular Mesh for face
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            Default::default(),
        );

        // Define positions of the face-triangle's vertices
        let positions = vec![
            top.to_array(), // Top vertex
            base_corners[i].to_array(),
            base_corners[next].to_array(),
        ];

        // Calculate normal vector on the 2-D plane on the face for the lighting and shading
        let v1 = base_corners[i] - top;
        let v2 = base_corners[next] - top;
        let normal = v1.cross(v2).normalize();

        // Save the normal of each vertex (same)
        let normals = vec![normal.to_array(); 3];

        // Insert the positions, normals, and UVs for each vertex into the mesh
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.5, 0.0], [0.0, 1.0], [1.0, 1.0]], // How to derive the texture to put into the triangular shape (flipped vertically)
        );

        // Spawn the face entity with mesh, material, transform, and components
        let face_entity = commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: game_state.pyramid_color_faces[i],
            cull_mode: None, // Disable backface culling - render both sides
            double_sided: true,
            ..default()
        })),
        Transform::default(),
        Pyramid,
        FaceMarker {
            face_index: i,
            color: game_state.pyramid_color_faces[i],
            normal: if game_state.pyramid_type == PyramidType::Type1 {normal} else {-normal},
        },
        GameEntity,
        )).id();

        // Spawn decorations on this face
        spawn_face_decorations(
            commands, 
            meshes, 
            materials, 
            game_state.random_gen.as_mut().unwrap(),
            face_entity,
            top,
            base_corners[i],
            base_corners[next],
            normal,
        );
    }
}


/// Spawn decorative shapes on a pyramid face using Poisson-like sampling
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
    
    // Determine number of decorations
    let decoration_count = rng.random_range(DECORATION_COUNT_MIN..=DECORATION_COUNT_MAX);
    
    // Store generated decoration positions and sizes for overlap checking
    let mut decorations: Vec<(Vec3, f32)> = Vec::new();
    
    // Maximum attempts to place each decoration before giving up
    const MAX_PLACEMENT_ATTEMPTS: usize = 30;
    
    // Try to place the desired number of decorations
    let mut successful_placements = 0;
    let mut total_attempts = 0;
    
    // Random shape type, same for all decorations on this face
    let shape = match rng.next_u64() % 4 {
        0 => DecorationShape::Circle,
        1 => DecorationShape::Square,
        2 => DecorationShape::Star,
        _ => DecorationShape::Triangle,
    };

    // Random color (vibrant colors), same for all decorations on this face
    let color = Color::srgb(
        rng.random_range(0.2..1.0),
        rng.random_range(0.2..1.0),
        rng.random_range(0.2..1.0),
    );
        

    while successful_placements < decoration_count && total_attempts < decoration_count * MAX_PLACEMENT_ATTEMPTS {
        total_attempts += 1;
        

        // Random size
        let size = rng.random_range(DECORATION_SIZE_MIN..DECORATION_SIZE_MAX);
        
        // Generate random position using barycentric coordinates (ensures point is inside triangle)
        let (position, is_valid) = sample_point_in_triangle(
            rng,
            top,
            corner1,
            corner2,
            size,
            &decorations,
        );
        
        // Skip if position overlaps with existing decorations or is too close to edges
        if !is_valid {
            continue;
        }
        
        // Create mesh based on shape
        let mesh = create_decoration_mesh(shape, size);
        
        // Calculate rotation to align with face plane
        // First rotate from Z-up (mesh default) to Y-up, then align Y-up to face normal
        let base_rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2); // Rotate 90° to make mesh face up in Y
        let face_rotation = Quat::from_rotation_arc(Vec3::Y, face_normal);
        let final_rotation = face_rotation * base_rotation;
        
        // Optional: add small random rotation around the normal for variety
        let random_spin = Quat::from_axis_angle(face_normal, rng.random_range(0.0..std::f32::consts::TAU));
        let rotation = random_spin * final_rotation;
        
        // Offset position slightly along normal to prevent z-fighting with face
        let offset_position = position - face_normal * 0.001;
        
        // Spawn decoration as child of face
        commands.entity(parent_face).with_children(|parent| {
            parent.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color,
                    emissive: color.to_linear() * 0.3, // Slight glow
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
        
        // Store this decoration's position and size for future collision checks
        decorations.push((position, size));
        successful_placements += 1;
    }
}

/// Sample a random point inside a triangle using barycentric coordinates
/// with collision checking against existing decorations
fn sample_point_in_triangle(
    rng: &mut ChaCha8Rng,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    size: f32,
    existing_decorations: &[(Vec3, f32)],
) -> (Vec3, bool) {
    // Generate random barycentric coordinates
    // Using the square root method for uniform distribution
    let r1 = rng.random_range(0.0..1.0_f32).sqrt();
    let r2 = rng.random_range(0.0..1.0_f32);
    
    // Barycentric weights that ensure point is inside triangle
    let w0 = 1.0 - r1;
    let w1 = r1 * (1.0 - r2);
    let w2 = r1 * r2;
    
    // Calculate 3D position
    let position = v0 * w0 + v1 * w1 + v2 * w2;
    
    // Minimum distance from edges (proportional to decoration size)
    let edge_margin = size * 1.5;
    
    // Check if too close to triangle edges
    let dist_to_edge_01 = point_to_line_segment_distance(position, v0, v1);
    let dist_to_edge_12 = point_to_line_segment_distance(position, v1, v2);
    let dist_to_edge_20 = point_to_line_segment_distance(position, v2, v0);
    
    if dist_to_edge_01 < edge_margin || dist_to_edge_12 < edge_margin || dist_to_edge_20 < edge_margin {
        return (position, false);
    }
    
    // Check for overlap with existing decorations (Poisson disk constraint)
    let min_spacing = size * 2.0; // Minimum distance between decoration centers
    
    for (existing_pos, existing_size) in existing_decorations {
        let distance = position.distance(*existing_pos);
        let required_distance = (size + existing_size) * 1.2; // 20% extra spacing
        
        if distance < required_distance.max(min_spacing) {
            return (position, false);
        }
    }
    
    (position, true)
}

/// Calculate minimum distance from a point to a line segment
fn point_to_line_segment_distance(point: Vec3, line_start: Vec3, line_end: Vec3) -> f32 {
    let line_vec = line_end - line_start;
    let point_vec = point - line_start;
    let line_length_sq = line_vec.length_squared();
    
    if line_length_sq < 1e-6 {
        return point_vec.length();
    }
    
    // Project point onto line and clamp to segment
    let t = (point_vec.dot(line_vec) / line_length_sq).clamp(0.0, 1.0);
    let projection = line_start + line_vec * t;
    
    point.distance(projection)
}

/// Create a mesh for a decoration shape
fn create_decoration_mesh(shape: DecorationShape, size: f32) -> Mesh {
    match shape {
        DecorationShape::Circle => {
            Circle::new(size).mesh().resolution(16).build()
        }
        DecorationShape::Square => {
            Rectangle::new(size * 2.0, size * 2.0).mesh().build()
        }
        DecorationShape::Star => {
            create_star_mesh(size, 5)
        }
        DecorationShape::Triangle => {
            create_triangle_mesh(size)
        }
    }
}

/// Create a star-shaped mesh
fn create_star_mesh(size: f32, points: usize) -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    );
    
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    
    // Center point
    positions.push([0.0, 0.0, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    uvs.push([0.5, 0.5]);
    
    // Create star points
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
    
    // Create triangles
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

/// Create a triangle mesh
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
