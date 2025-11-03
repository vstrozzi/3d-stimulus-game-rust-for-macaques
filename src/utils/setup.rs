use bevy::prelude::*;

use crate::log;
use crate::utils::objects::{FaceMarker, GameState, Pyramid, RotationSpeed};

/// Systems
pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        // Start at (0, 2.5, 8.0) looking at the origin
        Transform::from_xyz(0.0, 2.5, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Light
    commands.spawn((
        PointLight {
            intensity: 2_000_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    // Ambient light
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 100.0, // Bevy 0.17.0 uses a 0-100 scale here
        affects_lightmapped_meshes: true,
    });

    // Create pyramid - 3 triangular faces
    let pyramid_height = 2.0;
    let base_size = 3.0;

    // Define vertices for pyramid
    let top = Vec3::new(0.0, pyramid_height, 0.0);
    let base_corners = [
        Vec3::new(-base_size / 3.0, 0.0, -base_size / 3.0),
        Vec3::new(base_size / 3.0, 0.0, -base_size / 3.0),
        Vec3::new(base_size / 3.0, 0.0, base_size / 3.0),
    ];

    // Face colors - one will be the target (red with marker)
    let face_colors = [
        Color::srgb(1.0, 0.2, 0.2), // Red - TARGET FACE
        Color::srgb(0.2, 0.5, 1.0), // Blue
        Color::srgb(0.2, 1.0, 0.3), // Green
    ];

    let target_face = 0; // Red face is the target

    // Create 3 triangular faces
    for i in 0..3 {
        let next = (i + 1) % 3;

        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            Default::default(),
        );

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

        let mut material_color = face_colors[i];

        // Add a small square marker to the target face
        if i == target_face {
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
                color: face_colors[i],
                normal: normal, // <-- Store the calculated normal
            },
        ));
    }

    // Add small cube marker on target face
    let face_center = (top + base_corners[target_face] + base_corners[1]) / 3.0;
    let marker_offset = (face_center - Vec3::ZERO).normalize() * 0.1;

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.2, 0.2, 0.2))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::rgb(2.0, 2.0, 2.0),
            ..default()
        })),
        Transform::from_translation(face_center + marker_offset),
        Pyramid,
    ));

    // Initialize game state
    commands.insert_resource(GameState {
        start_time: time.elapsed(),
        is_playing: true,
        target_face_index: target_face,
        attempts: 0,
    });

    commands.insert_resource(RotationSpeed(1.0));

    log!("🎮 Pyramid Game Started!");
    log!("🎯 Find and center the RED face with the white marker");
    log!("⌨️  Use Arrow Keys or WASD to rotate");
    log!("␣  Press SPACE when the target face is centered");
}
