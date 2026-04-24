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

/// Bilinear interpolation inside the (tl, tr, bl, br) quad.
/// u=0 -> left edge, u=1 -> right edge, v=0 -> top edge, v=1 -> bottom edge.
fn bilerp_quad(tl: Vec3, tr: Vec3, bl: Vec3, br: Vec3, u: f32, v: f32) -> Vec3 {
    let top = tl.lerp(tr, u);
    let bot = bl.lerp(br, u);
    top.lerp(bot, v)
}

/// Deterministic "max-space" decoration layout: items are placed at the
/// centers of a uniform grid over the face in UV space. Grid dimensions are
/// `cols = ceil(sqrt(n))`, `rows = ceil(n / cols)`, which biases toward
/// horizontal splits (e.g. n=2 → 2×1, n=4 → 2×2). A partial last row is
/// horizontally centered.
///
/// Used when the decoration seed equals 0 (sentinel).
pub fn generate_grid_decoration_set(
    count: u32,
    size: f32,
    decoration_shape: DecorationShape,
    color: Color,
    thickness: f32,
) -> DecorationSet {
    let n = count as usize;
    let mut decorations: Vec<Decoration> = Vec::with_capacity(n);
    if n == 0 {
        return DecorationSet { shape: decoration_shape, color, decorations };
    }

    let cols = (n as f32).sqrt().ceil() as usize;
    let rows = ((n as f32) / (cols as f32)).ceil() as usize;

    let mut placed = 0usize;
    for r in 0..rows {
        let items_this_row = if r + 1 == rows { n - placed } else { cols };
        let row_offset = (cols - items_this_row) as f32 * 0.5;
        for c in 0..items_this_row {
            let u = (row_offset + c as f32 + 0.5) / cols as f32;
            let v = (r as f32 + 0.5) / rows as f32;
            decorations.push(Decoration {
                uv: Vec2::new(u, v),
                size,
                thickness,
            });
            placed += 1;
        }
    }

    DecorationSet { shape: decoration_shape, color, decorations }
}

/// Generates a decoration set for a pyramid face using Poisson-like sampling
/// over the whole quadrilateral face. Decorations are stored as bilinear
/// (u, v) coordinates in the (tl, tr, bl, br) quad.
pub fn generate_decoration_set(
    rng: &mut ChaCha8Rng,
    tl: Vec3,
    tr: Vec3,
    bl: Vec3,
    br: Vec3,
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

        let (u, v, world_position, is_valid) =
            sample_point_in_quad(rng, tl, tr, bl, br, size, &decorations_world);
        if !is_valid {
            continue;
        }

        decorations.push(Decoration {
            uv: Vec2::new(u, v),
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
/// Reconstructs world positions via bilinear interpolation over the quad.
pub fn spawn_decorations_from_set(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    parent_face: Entity,
    decoration_set: &DecorationSet,
    preloaded: &PreloadedTextures,
    tl: Vec3,
    tr: Vec3,
    bl: Vec3,
    br: Vec3,
    face_normal: Vec3,
    texture_id: u32,
) {
    let dec_tex = preloaded.get(Texture::from_u32(texture_id));

    // Build a consistent face basis so every decoration on the face shares the
    // same orientation
    let raw_up = ((tl - bl) + (tr - br)) * 0.5;
    let face_right = raw_up.cross(face_normal).normalize();
    let face_up = face_normal.cross(face_right).normalize();
    let rotation = Quat::from_mat3(&Mat3::from_cols(face_right, face_up, face_normal));

    for decoration in &decoration_set.decorations {
        let position = bilerp_quad(tl, tr, bl, br, decoration.uv.x, decoration.uv.y);

        let mesh = create_decoration_mesh(decoration_set.shape, decoration.size, decoration.thickness);

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
                    rotation,
                    scale: Vec3::ONE,
                },
                GameEntity,
            ));
        });
    }
}

/// Samples a random point inside the quad with edge-margin and Poisson-disk constraints.
/// Returns `(u, v, world_position, is_valid)`.
fn sample_point_in_quad(
    rng: &mut ChaCha8Rng,
    tl: Vec3,
    tr: Vec3,
    bl: Vec3,
    br: Vec3,
    size: f32,
    existing_decorations: &[(Vec3, f32)],
) -> (f32, f32, Vec3, bool) {
    let u = rng.random_range(0.0..1.0_f32);
    let v = rng.random_range(0.0..1.0_f32);
    let position = bilerp_quad(tl, tr, bl, br, u, v);

    let edge_margin = size * 1.5;
    if point_to_line_segment_distance(position, tl, tr) < edge_margin
        || point_to_line_segment_distance(position, tr, br) < edge_margin
        || point_to_line_segment_distance(position, br, bl) < edge_margin
        || point_to_line_segment_distance(position, bl, tl) < edge_margin
    {
        return (u, v, position, false);
    }

    let min_spacing = size * 2.0;
    for (existing_pos, existing_size) in existing_decorations {
        let required_distance = (size + existing_size) * 1.2;
        if position.distance(*existing_pos) < required_distance.max(min_spacing) {
            return (u, v, position, false);
        }
    }

    (u, v, position, true)
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
