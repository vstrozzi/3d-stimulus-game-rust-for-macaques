use bevy::prelude::*;

use crate::log;
use crate::utils::objects::{FaceMarker, GameState, Pyramid};
use crate::utils::constants::{
    camera_3d_constants::{
        CAMERA_3D_INITIAL_X,
        CAMERA_3D_INITIAL_Y,
        CAMERA_3D_INITIAL_Z,
    },
    object_constants::GROUND_Y,
    pyramid_constants::{
        PYRAMID_BASE_RADIUS,
        PYRAMID_HEIGHT,
        PYRAMID_COLORS,
        PYRAMID_TARGET_FACE_INDEX,
        PYRAMID_ANGLE_OFFSET_RAD,
        PYRAMID_ANGLE_INCREMENT_RAD
    },
};

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
        Transform::from_xyz(CAMERA_3D_INITIAL_X, CAMERA_3D_INITIAL_Y, CAMERA_3D_INITIAL_Z).looking_at(Vec3::ZERO, Vec3::Y),
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
    ));

    // Light
    commands.spawn((
        PointLight {
            intensity: 2_000_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(2.0, 2.0, -2.0),
    ));

    // Ambient light
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 50.0, // Bevy 0.17.0 uses a 0-100 scale here
        affects_lightmapped_meshes: true,
    });

    // Spawn Pyramid by borrowing commands, meshes, materials
    spawn_pyramid(&mut commands, &mut meshes, &mut materials);

    let target_face = 0; // Red face is the target

    // Initialize game state
    commands.insert_resource(GameState {
        start_time: time.elapsed(),
        is_playing: true,
        target_face_index: target_face,
        attempts: 0,
    });

    log!("🎮 Pyramid Game Started!");
    log!("🎯 Find and center the RED face with the white marker");
    log!("⌨️  Use Arrow Keys or WASD to rotate");
    log!("␣  Press SPACE when the target face is centered");
}


/// Spawn a pyramid composed of 3 triangular faces
pub fn spawn_pyramid(commands: &mut Commands, meshes: &mut ResMut<Assets<Mesh>>, materials: &mut ResMut<Assets<StandardMaterial>>){

    // Define top vertex for pyramid
    let top = Vec3::new(0.0, PYRAMID_HEIGHT, 0.0);
    // Build symmetric triangular vertices for base
    let mut base_corners: [Vec3; 3] = [Vec3::ZERO; 3];
    let mut prev_xz = Vec2::new(PYRAMID_BASE_RADIUS* PYRAMID_ANGLE_OFFSET_RAD.cos(), PYRAMID_BASE_RADIUS * PYRAMID_ANGLE_OFFSET_RAD.sin());
    base_corners[0] = Vec3::new(prev_xz.x, GROUND_Y, prev_xz.y);
    // Compute constants
    let pyramid_angle_increment_cos: f32 = PYRAMID_ANGLE_INCREMENT_RAD.cos();
    let pyramid_angle_increment_sin: f32 = PYRAMID_ANGLE_INCREMENT_RAD.sin();
    for i in 1..3{
        // Construct new corner by rotating from previous on 2D base-circle of pyramid in xz-plane
        let x = prev_xz.x * pyramid_angle_increment_cos - prev_xz.y * pyramid_angle_increment_sin;
        let z = prev_xz.y * pyramid_angle_increment_cos + prev_xz.x * pyramid_angle_increment_sin;

        prev_xz = Vec2::new(x, z);
        // Save new vertex
        base_corners[i] = Vec3::new(prev_xz.x, GROUND_Y, prev_xz.y);
    }

    // Create triangular faces independently
    for i in 0..3 {
        let next = (i + 1) % 3;

        // Create triangular mesh for face
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            Default::default(),
        );

        // Define positions
        let positions = vec![
            top.to_array(),
            base_corners[i].to_array(),
            base_corners[next].to_array(),
        ];

        // Calculate normal
        let v1 = base_corners[i] - top;
        let v2 = base_corners[next] - top;
        let normal = v1.cross(v2).normalize(); // <-- This is the face's normal
        let normals = vec![normal.to_array(); 3];

        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.5, 0.0], [0.0, 1.0], [1.0, 1.0]],
        );

        let mut material_color = PYRAMID_COLORS[i];

        // Add a small square marker to the target face
        if i == PYRAMID_TARGET_FACE_INDEX {
            material_color = Color::srgb(1.0, 0.3, 0.3);
        }

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: material_color,
                cull_mode: None, // Disable backface culling - render both sides
                double_sided: true,
                ..default()
            })),
            Transform::default(),
            Pyramid,
            FaceMarker {
                face_index: i,
                color: PYRAMID_COLORS[i],
                normal: normal, // <-- Store the calculated normal
            },
        ));
    }

}