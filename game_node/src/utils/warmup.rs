//! Startup warmup phase for GPU.
//! Spawn a sub-pixel "warmup" scene that loads every
//! texture + every decoration shape + the emissive pipeline. After enough
//! frames have rendered to cache everything, the warmup entities are
//! despawned and `WarmupState.complete` is set. `check_scene_ready` is
//! gated on that flag so the controller stays in WAITING_FOR_START until
//! the GPU is hot.
//!

use bevy::prelude::*;
use shared::{DecorationShape, Texture};
use strum::IntoEnumIterator;

use crate::shared_memory::shared_memory_writer::RenderFrameCounterResource;
use crate::utils::decorations::create_decoration_mesh;
use crate::utils::load_assets::{natural_material, tinted_material_tiled};
use crate::utils::objects::PreloadedTextures;

/// Marker for entities spawned during warmup; despawned once warmup
/// completes. Distinct from `GameEntity` so the normal trial reset path
/// never touches these.
#[derive(Component)]
pub struct WarmupEntity;

/// Tracks warmup progress. `complete` is what `check_scene_ready` gates on.
#[derive(Resource, Default)]
pub struct WarmupState {
    /// Render frame at which every preloaded texture first reported `all_loaded`.
    /// We render the warmup scene for `WARMUP_FRAMES_AFTER_LOAD` render frames
    /// after this so the GPU pipeline cache is populated. Stored in the
    /// render-frame domain (`RenderFrameCounterResource`) so that warmup
    /// progresses even when the controller has set `stop_rendering=true`,
    /// which pauses `FrameCounterResource`.
    pub all_loaded_at_frame: Option<u64>,
    pub complete: bool,
}

/// How many render frames to render the warmup scene after every texture is
/// decoded. Counted in render frames (vsync ticks) so warmup completion is
/// independent of game-logic pausing.
const WARMUP_FRAMES_AFTER_LOAD: u64 = 20;

/// Spawn a tiny, near-invisible scene that touches every texture +
/// shape + material variant we'll ever use in a trial.
pub fn spawn_warmup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    preloaded: Res<PreloadedTextures>,
) {
    let tiny_scale = Vec3::splat(0.001);
    let origin = Vec3::new(0.0, 1.0, 0.0);

    // One small cube per texture,  forces GPU upload of every `Texture`
    // variant via natural_material's full PBR slots.
    for (i, tex) in Texture::iter().enumerate() {
        let texset = preloaded.get(tex);
        let mat = materials.add(natural_material(texset));
        let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0).mesh().build());
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(origin + Vec3::new(i as f32 * 0.0001, 0.0, 0.0))
                .with_scale(tiny_scale),
            WarmupEntity,
        ));
    }

    // One small mesh per DecorationShape, exercises every shape's
    // vertex layout and the tinted+tiled material pipeline.
    let dec_tex = preloaded.get(Texture::iter().next().expect("at least one texture"));
    let shapes = [
        DecorationShape::Circle,
        DecorationShape::Square,
        DecorationShape::Star,
        DecorationShape::Triangle,
        DecorationShape::Rectangle,
        DecorationShape::Oval,
        DecorationShape::Pentagon,
        DecorationShape::Kite,
        DecorationShape::Rhombus,
        DecorationShape::Trapezoid,
        DecorationShape::Semicircle,
    ];
    for (i, shape) in shapes.into_iter().enumerate() {
        let mesh = meshes.add(create_decoration_mesh(shape, 1.0, 0.5));
        let mat = materials.add(tinted_material_tiled(dec_tex, Color::WHITE, 1.0));
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(origin + Vec3::new(0.0, i as f32 * 0.0001, 0.0))
                .with_scale(tiny_scale),
            WarmupEntity,
        ));
    }

    // Emissive double-sided panel — covers the StandardMaterial variant
    //    used by HoleEmissive (emissive + cull_mode: None).
    let emissive_mat = materials.add(StandardMaterial {
        emissive: LinearRgba::new(0.5, 0.5, 0.5, 1.0),
        cull_mode: None,
        ..default()
    });
    let pent_mesh = meshes.add(create_decoration_mesh(DecorationShape::Pentagon, 1.0, 0.0));
    commands.spawn((
        Mesh3d(pent_mesh),
        MeshMaterial3d(emissive_mat),
        Transform::from_translation(origin + Vec3::new(0.0, 0.0, 0.0001))
            .with_scale(tiny_scale),
        WarmupEntity,
    ));

    info!("Warmup scene spawned ({} textures + {} shapes + 1 emissive)",
          Texture::iter().count(), shapes.len());
}

/// Each frame, advance the warmup state machine:
///   - Wait until every preloaded texture is decoded.
///   - From that frame, render for `WARMUP_FRAMES_AFTER_LOAD` more frames.
///   - Then despawn all `WarmupEntity` entities and mark complete.
pub fn tick_warmup(
    mut warmup: ResMut<WarmupState>,
    images: Res<Assets<Image>>,
    preloaded: Res<PreloadedTextures>,
    render_frame_counter: Res<RenderFrameCounterResource>,
    warmup_entities: Query<Entity, With<WarmupEntity>>,
    mut commands: Commands,
) {
    if warmup.complete {
        return;
    }

    let all_loaded = preloaded.0.values().all(|set| set.all_loaded(&images));
    if !all_loaded {
        return;
    }

    let start = match warmup.all_loaded_at_frame {
        Some(f) => f,
        None => {
            warmup.all_loaded_at_frame = Some(render_frame_counter.0);
            info!("Warmup: all textures decoded at render frame {}; rendering for {} more frames",
                  render_frame_counter.0, WARMUP_FRAMES_AFTER_LOAD);
            return;
        }
    };

    if render_frame_counter.0.saturating_sub(start) < WARMUP_FRAMES_AFTER_LOAD {
        return;
    }

    let mut despawned = 0usize;
    for e in warmup_entities.iter() {
        commands.entity(e).despawn();
        despawned += 1;
    }
    warmup.complete = true;
    info!("Warmup complete: despawned {} entities; GPU pipelines hot", despawned);
}
