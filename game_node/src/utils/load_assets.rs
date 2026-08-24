use bevy::prelude::*;
use bevy::audio::Volume;
use bevy::image::{ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor, ImageAddressMode};
use bevy::math::Affine2;
use crate::{PreloadedTextures, GameConditions, GameStateLocal};
use crate::shared_memory::shared_memory_reader::SharedMemResource;
use crate::utils::objects::{TexturePreloadManifest, TexturePreloadState};
use crate::utils::objects::{GameEntity, LoadingCountdown, LoadingCountdownText};
use crate::utils::pyramid::spawn_pyramid;
use crate::utils::helpers::spawn_blank_screen;
use crate::utils::setup::build_pyramid_config;
use shared::constants::game_constants::LOADING_COUNTDOWN_SECS;
use shared::constants::sound_constants::ENABLE_BACKGROUND_MUSIC;


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

/// Convert an RGBA color mask into the opaque multiplier used by materials.
/// Alpha is mask strength: 0 keeps the authored texture color (white
/// multiplier), while 1 applies the selected RGB fully.
pub fn color_mask_tint(mask: Color) -> Color {
    let mask = mask.to_srgba();
    let strength = mask.alpha.clamp(0.0, 1.0);
    Color::srgb(
        1.0 + (mask.red - 1.0) * strength,
        1.0 + (mask.green - 1.0) * strength,
        1.0 + (mask.blue - 1.0) * strength,
    )
}

#[cfg(test)]
mod color_mask_tests {
    use super::color_mask_tint;
    use bevy::prelude::Color;

    fn assert_rgb(actual: Color, expected: [f32; 3]) {
        let actual = actual.to_srgba();
        for (value, expected) in [actual.red, actual.green, actual.blue]
            .into_iter()
            .zip(expected)
        {
            assert!((value - expected).abs() < 1e-6, "{value} != {expected}");
        }
        assert!((actual.alpha - 1.0).abs() < 1e-6);
    }

    #[test]
    fn zero_strength_keeps_the_texture_untinted() {
        assert_rgb(color_mask_tint(Color::srgba(0.2, 0.4, 0.6, 0.0)), [1.0; 3]);
    }

    #[test]
    fn half_strength_interpolates_from_white_to_mask_rgb() {
        assert_rgb(
            color_mask_tint(Color::srgba(0.2, 0.4, 0.6, 0.5)),
            [0.6, 0.7, 0.8],
        );
    }

    #[test]
    fn full_strength_applies_mask_rgb_without_transparency() {
        assert_rgb(
            color_mask_tint(Color::srgba(0.2, 0.4, 0.6, 1.0)),
            [0.2, 0.4, 0.6],
        );
    }
}

/// Tinted texture whose RGBA tint is interpreted as a color mask.
pub fn tinted_material(tex: &TextureSet, tint: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color_mask_tint(tint),
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

/// Wait for the controller's shared-memory manifest, then load exactly those
/// trial textures plus the game's structural base/lid textures.
pub fn preload_required_textures(
    asset_server: Res<AssetServer>,
    shm_res: Option<Res<SharedMemResource>>,
    mut manifest: ResMut<TexturePreloadManifest>,
    mut preload_state: ResMut<TexturePreloadState>,
    mut preloaded: ResMut<PreloadedTextures>,
) {
    if preload_state.initialized {
        return;
    }

    let Some(shm_res) = shm_res else { return };
    let Some(indices) = shm_res.0.get().texture_manifest.read_indices() else { return };

    *manifest = TexturePreloadManifest::from_indices(indices);
    use shared::Texture;
    use strum::IntoEnumIterator;
    for tex in Texture::iter().filter(|&tex| manifest.includes(tex)) {
        preloaded.0.insert(tex, load_texture_set(&asset_server, &tex.asset_folder()));
    }
    preload_state.initialized = true;
    info!("Preloading {} of {} registered texture sets", preloaded.0.len(), Texture::iter().count());
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
        let textures_loaded = preloaded.0.values().all(|set| set.all_loaded(&images));
        if !(textures_loaded && sounds.all_loaded(&audio_sources)) {
            return;
        }

        countdown.start = Some(time.elapsed());

        // Fake pyramid (a GameEntity, cleaned up at the end) to warm the path.
        let mut config = build_pyramid_config(&local_game_struct.0);
        let warmup_texture = *preloaded.0.keys().next().expect("at least one texture") as u32;
        config.face_textures.fill(warmup_texture);
        config.decoration_textures.fill(warmup_texture);
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
    // Spawn a black BlankScreen first so the freshly built scene never flashes
    // between the loading overlay despawn and the controller issuing its own
    // blank command. The controller's `toggle_blank: !is_blank` logic then sees
    // is_blank=true and leaves the screen black until the trial actually starts.
    spawn_blank_screen(&mut commands);
    if let Some(overlay) = countdown.overlay.take() {
        if let Ok(mut ec) = commands.get_entity(overlay) {
            ec.despawn();
        }
    }
    for e in &game_entities {
        commands.entity(e).try_despawn();
    }

    // Unmute the looping background music now that the countdown is over.
    // Per-level volume is then kept in sync by `update_background_music_volume`.
    if let Some(music) = countdown.music {
        if let Ok(mut sink) = sink_query.get_mut(music) {
            sink.set_volume(Volume::Linear(f32::from_bits(local_game_struct.0.background_music_volume)));
        }
    }

    // Allow game to start
    game_conditions.is_scene_ready = true;
}

/// Keep the looping background-music volume synced to the per-level
/// `background_music_volume` (written by the controller into shared memory).
/// Runs every frame so a new level's value takes effect at its trial reset.
pub fn update_background_music_volume(
    game_conditions: Res<GameConditions>,
    countdown: Res<LoadingCountdown>,
    local_game_struct: Res<GameStateLocal>,
    mut sink_query: Query<&mut AudioSink>,
) {
    // Stay silent during warmup/countdown (the music is intentionally muted then);
    // check_scene_ready unmutes it at handoff. Only track the level value after.
    if !game_conditions.is_scene_ready {
        return;
    }
    let Some(music) = countdown.music else { return };
    if let Ok(mut sink) = sink_query.get_mut(music) {
        sink.set_volume(Volume::Linear(f32::from_bits(local_game_struct.0.background_music_volume)));
    }
}
