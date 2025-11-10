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
use crate::utils::objects::{FaceMarker, GameEntity, GameState, Pyramid, PyramidType};

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
        brightness: 50.0, // Bevy 0.17.0 uses a 0-100 scale here
        affects_lightmapped_meshes: true,
    });

    // Initialize game state
    let game_state = setup_game_state(&mut commands, &time);
    // Spawn Pyramid by borrowing commands, meshes, materials
    spawn_pyramid(&mut commands, &mut meshes, &mut materials, &game_state);

    log!("🎮 Pyramid Game Started!");
}

// Initialize game state resource with random values
pub fn setup_game_state(commands: &mut Commands, time: &Res<Time>) -> GameState {
    // Create Random Structure
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
    let pyramid_target_face_index = random_gen.next_u64() % 3;

    let game_state = GameState {
        random_seed: random_seed,
        random_gen: Some(random_gen),
        pyramid_type: pyramid_type,
        pyramid_base_radius: pyramid_base_radius,
        pyramid_height: pyramid_height,
        pyramid_target_face_index: pyramid_target_face_index as usize,
        pyramid_start_orientation_radius: pyramid_start_orientation_radius,
        pyramid_color_faces: PYRAMID_COLORS,
        
        is_playing: true,
        is_started: false,
        is_won: false,
        is_changed: true,

        start_time: Some(time.elapsed()),
        end_time: None,

        attempts: 0,
        cosine_alignment: None,
    };

    println!("{:?}", game_state);
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
    game_state: &GameState,
) {

    println!("{:?}", game_state);

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
        // Construct new corner by rotating from previous on 2D base-circle of pyramid in xz-plane
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
        commands.spawn((
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
                normal: normal,
            },
            GameEntity,
        ));
    }
}
