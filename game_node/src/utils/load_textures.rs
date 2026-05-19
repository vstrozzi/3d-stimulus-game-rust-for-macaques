use bevy::prelude::*;
use bevy::image::{ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor, ImageAddressMode};
use bevy::math::Affine2;
use crate::{PreloadedTextures, GameConditions, GameStateLocal};
use crate::shared_memory::shared_memory_writer::{FrameCounterResource, RenderFrameCounterResource, StagedRenderSample, StagedFrame}; 

/// Holds all loaded handles for one PBR texture set
/// Store this as a resource so handles stay alive
#[derive(Resource)]
pub struct TextureSet {
    color:              Option<Handle<Image>>,
    color_tintable:     Option<Handle<Image>>,
    normal:             Option<Handle<Image>>,
    metallic_roughness: Option<Handle<Image>>,
    occlusion:          Option<Handle<Image>>,
    depth:              Option<Handle<Image>>,
}

impl TextureSet {
    /// Returns true once every handle in this set is present in the Assets<Image> store
    pub fn all_loaded(&self, images: &Assets<Image>) -> bool {
        [
            self.color.as_ref(),
            self.color_tintable.as_ref(),
            self.normal.as_ref(),
            self.metallic_roughness.as_ref(),
            self.occlusion.as_ref(),
            self.depth.as_ref(),
        ]
        .iter()
        .all(|h| match h {
            Some(h) => images.get(h.id()).is_some(),
            None => true,
        })
    }
}

// Load the whole texture set for one material
pub fn load_texture_set(
    asset_server: &AssetServer,
    folder: &str,
) -> TextureSet {
 
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(folder);

    println!("Loading texture set from: {}", base.display());

    let linear = |name: &str| -> Option<Handle<Image>> {
        // Check path existence only native, assetserver handles web request
        #[cfg(not(target_arch = "wasm32"))]
        {
            let full = base.join(name);
            if !full.exists() {
                println!("  ✗ missing {}", name);
                return None;
            }
        }
        
        Some(asset_server.load_with_settings(
            format!("{}/{}", folder, name),
            |s: &mut ImageLoaderSettings| {
                s.is_srgb = false;
                s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..default()
                });
            },
        ))
    };

    let color = |name: &str| -> Option<Handle<Image>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let full = base.join(name);
            if !full.exists() {
                println!("  ✗ missing {}", name);
                return None;
            }
            println!("  ✓ found {}", name);
        }
        Some(asset_server.load_with_settings(
            format!("{}/{}", folder, name),
            |s: &mut ImageLoaderSettings| {
                s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..default()
                });
            },
        ))
    };

    TextureSet {
        color:              color("color.png"),
        color_tintable:     color("color_tintable.png"),
        normal:             linear("normal_gl.png"),
        metallic_roughness: linear("metallic_roughness.png"),
        occlusion:          linear("occlusion.png"),
        depth:              linear("displacement_inv.png"),
    }
}

// Create a natural material form loaded textures
pub fn natural_material(tex: &TextureSet) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: tex.color.clone(),

        normal_map_texture: tex.normal.clone(),
        flip_normal_map_y: false,

        metallic_roughness_texture: tex.metallic_roughness.clone(),
        metallic: 1.0,
        perceptual_roughness: 1.0,

        occlusion_texture: tex.occlusion.clone(),

        depth_map: tex.depth.clone(),
        // PERF: parallax fragment cost is the dominant fragment-shader cost
        // in the scene (every surface uses this material). 32 layers ×
        // 80+ surfaces was producing the "missed one vsync" pattern in
        // data_perf/. See documentation_performance.md §F1 — revert to
        // (0.05, 32.0) if a regression in surface depth-detail appears.
        parallax_depth_scale: 0.03,
        parallax_mapping_method: ParallaxMappingMethod::Occlusion,
        max_parallax_layer_count: 8.0,

        ..default()
    }
}

// Tile a texture across the surface, using the uv_transform to repeat it
pub fn natural_material_tiled(tex: &TextureSet, tile: f32) -> StandardMaterial {
    StandardMaterial {
        uv_transform: Affine2::from_scale(Vec2::splat(tile)),
        ..natural_material(tex)
    }
}

/// Tinted look grain detail from texture, hue from base_color
pub fn tinted_material(tex: &TextureSet, tint: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: tint,
        base_color_texture: tex.color_tintable.clone(),
        ..natural_material(tex)
    }
}

// Tint and tile a texture
pub fn tinted_material_tiled(tex: &TextureSet, tint: Color, tile: f32) -> StandardMaterial {
    StandardMaterial {
        uv_transform: Affine2::from_scale(Vec2::splat(tile)),
        ..tinted_material(tex, tint)
    }
}

/// Load every texture set at startup and keep the handles in a resource
/// This prevents Bevy from GC-ing images between resets, eliminating latency at WASM trials's start
pub fn preload_all_textures(asset_server: Res<AssetServer>, mut preloaded: ResMut<PreloadedTextures>) {
    use shared::Texture;
    use strum::IntoEnumIterator;
    for tex in Texture::iter() {
        preloaded.0.insert(tex, load_texture_set(&asset_server, &tex.asset_folder()));
    }
}

/// Each frame while `is_scene_ready` is false, check whether all textures used by the
/// current trial are fully loaded (including GPU upload) **and** the GPU warmup
/// pass has finished. Once both are true, set the flag so the controller
/// knows it is safe to remove the blank screen.
///
/// Gating on warmup ensures the first trial doesn't pay the
/// pipeline-compilation / GPU-upload cost that previously produced
/// multi-hundred-millisecond Δt spikes in trial 0. See `warmup.rs`.
pub fn check_scene_ready(
    mut game_conditions: ResMut<GameConditions>,
    preloaded: Res<PreloadedTextures>,
    images: Res<Assets<Image>>,
    local_game_struct: Res<GameStateLocal>,
    warmup: Res<crate::utils::warmup::WarmupState>,
    mut counters: (ResMut<FrameCounterResource>, ResMut<RenderFrameCounterResource>, ResMut<StagedRenderSample>),
) {
    if game_conditions.is_scene_ready {
        return;
    }

    if !warmup.complete {
        return;
    }

    use shared::Texture;
    let gs = &local_game_struct.0;

    // All six texture slots (face + decoration) used by the current trial config must be loaded
    let all_loaded = gs.textures.iter()
        .chain(gs.decorations_texture.iter())
        .all(|&t| {
            let tex = Texture::from_u32(t);
            let res =
            preloaded.0.get(&tex)
                .map(|set| set.all_loaded(&images))
                .unwrap_or(false);
            res
        });

    if all_loaded {
        // Reset time counters
        counters.0.0 = 0;
        counters.1.0 = 0;

        counters.2.pending = Some(StagedFrame {
            render_frame_number: 0,
            render_elapsed_secs_bits: 0,
            photodiode_white: false,
        });

        game_conditions.is_scene_ready = true;
    }
}