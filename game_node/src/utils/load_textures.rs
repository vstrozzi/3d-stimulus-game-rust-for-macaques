use bevy::prelude::*;
use bevy::image::{ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor, ImageAddressMode};
use bevy::math::Affine2;

/// Holds all loaded handles for one PBR texture set.
/// Store this as a resource or component so handles stay alive.
#[derive(Resource)]
pub struct TextureSet {
    color:              Option<Handle<Image>>,
    color_tintable:     Option<Handle<Image>>,
    normal:             Option<Handle<Image>>,
    metallic_roughness: Option<Handle<Image>>,
    occlusion:          Option<Handle<Image>>,
    depth:              Option<Handle<Image>>,
}

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
        parallax_depth_scale: 0.05,
        parallax_mapping_method: ParallaxMappingMethod::Occlusion,
        max_parallax_layer_count: 32.0,

        ..default()
    }
}

// Tile a texture across the surface, using the uv_transform to repeat it
pub fn natural_material_tiled(tex: &TextureSet, tile: f32) -> StandardMaterial {
    StandardMaterial {
        // Disable parallax — it breaks with uv_transform
        uv_transform: Affine2::from_scale(Vec2::splat(tile)),
        ..natural_material(tex)
    }
}

/// Tinted look — grain detail from texture, hue from base_color
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
