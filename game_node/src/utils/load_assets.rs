use bevy::prelude::*;
use bevy::audio::Volume;
use bevy::image::{ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor, ImageAddressMode};
use bevy::math::Affine2;
use crate::{PreloadedTextures, GameConditions, GameStateLocal};
use crate::utils::objects::{GameEntity, LoadingCountdown, LoadingCountdownText};
use crate::utils::pyramid::spawn_pyramid;
use crate::utils::setup::build_pyramid_config;
use shared::constants::game_constants::LOADING_COUNTDOWN_SECS;
use shared::constants::sound_constants::{ENABLE_BACKGROUND_MUSIC, BACKGROUND_MUSIC_VOLUME};


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

/// Handles for the game's sounds, kept alive as a resource.
#[derive(Resource)]
pub struct SoundSet {
    pub win: Handle<AudioSource>,
    pub hint: Handle<AudioSource>,
    pub background: Handle<AudioSource>,
}

impl SoundSet {
    /// Returns true once every sound is present in the Assets<AudioSource> store
    pub fn all_loaded(&self, audio: &Assets<AudioSource>) -> bool {
        [&self.win, &self.hint, &self.background]
            .iter()
            .all(|h| audio.get(h.id()).is_some())
    }
}

/// Load all sounds into a resource so the handles stay alive.
pub fn load_sounds(asset_server: Res<AssetServer>, mut commands: Commands) {
    commands.insert_resource(SoundSet {
        win: asset_server.load("sounds/win_sound.ogg"),
        hint: asset_server.load("sounds/audio_earthquake.ogg"),
        background: asset_server.load("sounds/wind_sound.ogg"),
    });
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
        parallax_depth_scale: 1.0,
        parallax_mapping_method: ParallaxMappingMethod::Occlusion,
        max_parallax_layer_count: 32.0,

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

/// Gates `is_scene_ready` behind a black pre-start countdown.
///
/// Phase 1: wait until every texture (GPU-uploaded) and sound is loaded and
/// the GPU warmup pass has finished, then start the countdown — spawn a black
/// overlay with a `3/2/1` number, a fake pyramid (warms the spawn/render path
/// while the controller is still idle)
/// Phase 2: tick the countdown, updating the number each frame.
/// Phase 3: at the end, tear the loading scene down, unmute the music, reset
/// the timing counters, and finally set `is_scene_ready`.
pub fn check_scene_ready(
    mut game_conditions: ResMut<GameConditions>,
    preloaded: Res<PreloadedTextures>,
    images: Res<Assets<Image>>,
    sounds: Res<SoundSet>,
    audio_sources: Res<Assets<AudioSource>>,
    local_game_struct: Res<GameStateLocal>,
    warmup: Res<crate::utils::warmup::WarmupState>,
    mut countdown: ResMut<LoadingCountdown>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut sink_query: Query<&mut AudioSink>,
    mut text_query: Query<&mut Text, With<LoadingCountdownText>>,
    game_entities: Query<Entity, With<GameEntity>>,
) {
    if game_conditions.is_scene_ready {
        return;
    }

    // Black full-screen overlay, spawned once on the first frame so the canvas
    // is covered (with text) during warmup + texture upload — before the
    // countdown. The same text entity then shows the 3/2/1 numbers once the
    // countdown starts (Phase 2), so the screen stays black throughout loading.
    if countdown.overlay.is_none() {
        let overlay = commands.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(2000),
        )).with_children(|parent| {
            parent.spawn((
                Text::new("Loading textures"),
                TextFont { font_size: 64.0, ..default() },
                TextColor(Color::WHITE),
                LoadingCountdownText,
            ));
        }).id();
        countdown.overlay = Some(overlay);
    }

    if !warmup.complete {
        return;
    }

    // Phase 1: wait for all assets, then start the countdown. The overlay
    // already exists; Phase 2 swaps its "Loading textures…" text to the number.
    if countdown.start.is_none() {
        use shared::Texture;
        let gs = &local_game_struct.0;
        // All six texture slots (face + decoration) used by the current trial config must be loaded
        let textures_loaded = gs.textures.iter()
            .chain(gs.decorations_texture.iter())
            .all(|&t| {
                preloaded.0.get(&Texture::from_u32(t))
                    .map(|set| set.all_loaded(&images))
                    .unwrap_or(false)
            });
        if !(textures_loaded && sounds.all_loaded(&audio_sources)) {
            return;
        }

        countdown.start = Some(time.elapsed());

        // Fake pyramid (a GameEntity, cleaned up at the end) to warm the path.
        let config = build_pyramid_config(&local_game_struct.0);
        spawn_pyramid(&mut commands, &mut meshes, &mut materials, &preloaded, &config);

        // Muted looping background music to warm the audio graph; unmuted at
        // the end of the countdown. Skipped entirely when disabled.
        if ENABLE_BACKGROUND_MUSIC {
            let music = commands.spawn((
                AudioPlayer::new(sounds.background.clone()),
                PlaybackSettings { volume: Volume::Linear(0.0), ..PlaybackSettings::LOOP },
            )).id();
            countdown.music = Some(music);
        }

        return;
    }

    // Phase 2: countdown running — update the number (3, 2, 1).
    let elapsed = (time.elapsed() - countdown.start.unwrap()).as_secs_f32();
    if let Ok(mut text) = text_query.single_mut() {
        let n = (LOADING_COUNTDOWN_SECS - elapsed).max(0.0).ceil() as i32;
        let s = n.to_string();
        if text.0 != s {
            text.0 = s;
        }
    }
    if elapsed < LOADING_COUNTDOWN_SECS {
        return;
    }

    // Phase 3: tear down the loading scene and hand off to the controller.
    if let Some(overlay) = countdown.overlay.take() {
        if let Ok(mut ec) = commands.get_entity(overlay) {
            ec.despawn();
        }
    }
    for e in &game_entities {
        commands.entity(e).try_despawn();
    }

    // Unmute the looping background music now that the countdown is over.
    if let Some(music) = countdown.music {
        if let Ok(mut sink) = sink_query.get_mut(music) {
            sink.set_volume(Volume::Linear(BACKGROUND_MUSIC_VOLUME));
        }
    }

    // Allow game to start
    game_conditions.is_scene_ready = true;
}