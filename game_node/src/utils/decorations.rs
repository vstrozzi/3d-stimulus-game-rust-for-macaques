//! Function to spawn decorations on a pyramid face.
/// Creates a star-shaped mesh.

use crate::utils::utils::build_mesh;
use crate::utils::objects::{Decoration, DecorationSet, GameEntity, PreloadedTextures};
use crate::utils::load_textures::tinted_material_tiled;
use shared::{DecorationShape, Texture};
use bevy::prelude::*;
use bevy::prelude::ops::sqrt;
use bevy::mesh::{VertexAttributeValues, Indices};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

/// Creates a star-shaped mesh
fn create_star_mesh(size: f32, points: usize) -> Mesh {
    let mut positions = vec![[0.0f32, 0.0, 0.0]];
    let mut normals = vec![[0.0f32, 0.0, 1.0]];
    let mut uvs = vec![[0.5f32, 0.5]];
    let mut indices = Vec::new();

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

    for i in 1..=(points * 2) {
        let next = if i == points * 2 { 1 } else { i + 1 };
        indices.extend_from_slice(&[0, i as u32, next as u32]);
    }

    build_mesh(positions, normals, uvs, indices)
}

/// Creates a triangle-shaped mesh
fn create_triangle_mesh(size: f32) -> Mesh {
    let height = size * sqrt(3.0);
    build_mesh(
        vec![
            [0.0, height * 0.666, 0.0],
            [-size, -height * 0.333, 0.0],
            [size, -height * 0.333, 0.0],
        ],
        vec![[0.0, 0.0, 1.0]; 3],
        vec![[0.5, 1.0], [0.0, 0.0], [1.0, 0.0]],
        vec![],
    )
}

/// Generates a decoration set for a pyramid face using Poisson-like sampling.
/// Decorations are stored using barycentric coordinates relative to the triangle vertices.
pub fn generate_decoration_set(
    rng: &mut ChaCha8Rng,
    top: Vec3,
    corner1: Vec3,
    corner2: Vec3,
    count: u32,
    size: f32,
    decoration_shape: DecorationShape,
    color: Color,
    thickness: f32,
) -> DecorationSet {
    const MAX_PLACEMENT_ATTEMPTS: usize = 30;

    let mut decorations_world: Vec<(Vec3, f32)> = Vec::new();
    let mut decorations: Vec<Decoration> = Vec::new();
    let decoration_count = count as usize;

    let mut successful_placements = 0;
    let mut total_attempts = 0;

    while successful_placements < decoration_count
        && total_attempts < decoration_count * MAX_PLACEMENT_ATTEMPTS
    {
        total_attempts += 1;

        let (world_position, is_valid) =
            sample_point_in_triangle(rng, top, corner1, corner2, size, &decorations_world);
        if !is_valid {
            continue;
        }

        // Convert world position to barycentric coordinates
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

        decorations.push(Decoration {
            barycentric: Vec3::new(w0, w2, w1),
            size,
            thickness,
        });
        decorations_world.push((world_position, size));
        successful_placements += 1;
    }

    DecorationSet {
        shape: decoration_shape,
        color,
        decorations,
    }
}

/// Spawns decorations from a decoration set onto a face.
/// Reconstructs world positions from barycentric coordinates relative to the given triangle.
pub fn spawn_decorations_from_set(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    parent_face: Entity,
    decoration_set: &DecorationSet,
    preloaded: &PreloadedTextures,
    top: Vec3,
    corner1: Vec3,
    corner2: Vec3,
    face_normal: Vec3,
    texture_id: u32,
) {
    let dec_tex = preloaded.get(Texture::from_u32(texture_id));

    for decoration in &decoration_set.decorations {
        let position = decoration.barycentric.x * top
            + decoration.barycentric.y * corner1
            + decoration.barycentric.z * corner2;

        let mesh = create_decoration_mesh(decoration_set.shape, decoration.size, decoration.thickness);

        let base_rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let normal_rotation = Quat::from_rotation_arc(Vec3::Y, -face_normal);
        let final_rotation = normal_rotation * base_rotation;

        let offset_position = position + face_normal * 0.001;

        commands.entity(parent_face).with_children(|parent| {
            parent.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    reflectance: 0.1,
                    ..tinted_material_tiled(&dec_tex, decoration_set.color, 0.05)
                })),
                Transform {
                    translation: offset_position,
                    rotation: -final_rotation,
                    scale: Vec3::ONE,
                },
                GameEntity,
            ));
        });
    }
}

/// Samples a random point inside a triangle with edge-margin and Poisson-disk constraints.
fn sample_point_in_triangle(
    rng: &mut ChaCha8Rng,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    size: f32,
    existing_decorations: &[(Vec3, f32)],
) -> (Vec3, bool) {
    let r1 = rng.random_range(0.0..1.0_f32).sqrt();
    let r2 = rng.random_range(0.0..1.0_f32);
    let w0 = 1.0 - r1;
    let w1 = r1 * (1.0 - r2);
    let w2 = r1 * r2;
    let position = v0 * w0 + v1 * w1 + v2 * w2;

    let edge_margin = size * 1.5;
    if point_to_line_segment_distance(position, v0, v1) < edge_margin
        || point_to_line_segment_distance(position, v1, v2) < edge_margin
        || point_to_line_segment_distance(position, v2, v0) < edge_margin
    {
        return (position, false);
    }

    let min_spacing = size * 2.0;
    for (existing_pos, existing_size) in existing_decorations {
        let required_distance = (size + existing_size) * 1.2;
        if position.distance(*existing_pos) < required_distance.max(min_spacing) {
            return (position, false);
        }
    }

    (position, true)
}

/// Minimum distance from a point to a line segment.
fn point_to_line_segment_distance(point: Vec3, line_start: Vec3, line_end: Vec3) -> f32 {
    let line_vec = line_end - line_start;
    let point_vec = point - line_start;
    let line_length_sq = line_vec.length_squared();
    if line_length_sq < 1e-6 {
        return point_vec.length();
    }
    let t = (point_vec.dot(line_vec) / line_length_sq).clamp(0.0, 1.0);
    point.distance(line_start + line_vec * t)
}

/// Creates a mesh for a decoration shape, extruded to `thickness`.
fn create_decoration_mesh(shape: DecorationShape, size: f32, thickness: f32) -> Mesh {
    let flat = match shape {
        DecorationShape::Circle => Circle::new(size).mesh().resolution(16).build(),
        DecorationShape::Square => Rectangle::new(size * 2.0, size * 2.0).mesh().build(),
        DecorationShape::Star => create_star_mesh(size, 5),
        DecorationShape::Triangle => create_triangle_mesh(size),
    };
    extrude_mesh(flat, thickness)
}

/// Extrudes a flat 2-D mesh into a 3-D solid along the Z axis.
fn extrude_mesh(flat_mesh: Mesh, thickness: f32) -> Mesh {
    if thickness <= 0.0 {
        return flat_mesh;
    }

    let positions: Vec<[f32; 3]> = match flat_mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
        VertexAttributeValues::Float32x3(v) => v.clone(),
        _ => panic!("extrude_mesh: expected Float32x3 positions"),
    };
    let orig_uvs: Vec<[f32; 2]> = match flat_mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap() {
        VertexAttributeValues::Float32x2(v) => v.clone(),
        _ => panic!("extrude_mesh: expected Float32x2 UVs"),
    };
    let orig_indices: Vec<u32> = match flat_mesh.indices() {
        Some(Indices::U32(v)) => v.clone(),
        Some(Indices::U16(v)) => v.iter().map(|&i| i as u32).collect(),
        None => (0..positions.len() as u32).collect(),
    };

    let n = positions.len();
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut nrm: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();

    // Front face (z = thickness, normal = +Z)
    for i in 0..n {
        let [x, y, _] = positions[i];
        pos.push([x, y, thickness]);
        nrm.push([0.0, 0.0, 1.0]);
        uvs.push(orig_uvs[i]);
    }
    idx.extend_from_slice(&orig_indices);

    // Back face (z = 0, normal = −Z, winding flipped)
    let back_base = n as u32;
    for i in 0..n {
        let [x, y, _] = positions[i];
        pos.push([x, y, 0.0]);
        nrm.push([0.0, 0.0, -1.0]);
        uvs.push(orig_uvs[i]);
    }
    for tri in orig_indices.chunks(3) {
        idx.extend_from_slice(&[back_base + tri[0], back_base + tri[2], back_base + tri[1]]);
    }

    // Side walls: boundary edges appear in exactly one triangle
    let mut edge_seen: std::collections::HashMap<(u32, u32), (bool, u32, u32)> =
        std::collections::HashMap::new();
    for tri in orig_indices.chunks(3) {
        for &(a, b) in &[(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let key = if a < b { (a, b) } else { (b, a) };
            edge_seen
                .entry(key)
                .and_modify(|e| e.0 = false)
                .or_insert((true, a, b));
        }
    }
    for (_key, (is_boundary, e0, e1)) in &edge_seen {
        if !is_boundary {
            continue;
        }
        let [x0, y0, _] = positions[*e0 as usize];
        let [x1, y1, _] = positions[*e1 as usize];
        let edge = Vec2::new(x1 - x0, y1 - y0);
        let side_n = Vec2::new(edge.y, -edge.x).normalize();
        let base = pos.len() as u32;
        for &(x, y, z) in &[
            (x0, y0, 0.0_f32),
            (x1, y1, 0.0),
            (x1, y1, thickness),
            (x0, y0, thickness),
        ] {
            pos.push([x, y, z]);
            nrm.push([side_n.x, side_n.y, 0.0]);
            uvs.push([z / thickness, 0.0]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    build_mesh(pos, nrm, uvs, idx)
}
