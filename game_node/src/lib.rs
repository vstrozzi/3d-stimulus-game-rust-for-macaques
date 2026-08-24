//! Declaration of the utils modules for monkey_3d_game.

// Bevy systems idiomatically accept many resources/queries and use deeply
// nested Query types; clippy's defaults flag these on every system.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;


use bevy::{
    asset::AssetMetaCheck,
    diagnostic::{
        EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin,
    },
    prelude::*,
    window::*,
};

use bevy::winit::WinitSettings;

// Re-export shared memory functions so wasm-bindgen keeps them in the cdylib
#[cfg(target_arch = "wasm32")]
pub use shared::{create_shared_memory_wasm, WebSharedMemory};

// Shared memory helpers
pub mod shared_memory {
    pub mod shared_memory_reader;
    pub mod shared_memory_writer;
    pub mod shared_memory_web_extension;
}
/// Game functions
pub mod utils {
    pub mod camera;
    pub mod helpers;
    pub mod ui;
    pub mod handle_commands;
    pub mod debug_functions;
    pub mod game_functions;
    pub mod macros;
    pub mod objects;
    pub mod pyramid;
    pub mod decorations;
    pub mod setup;
    pub mod fog;
    pub mod systems_logic;
    pub mod load_assets;
    pub mod warmup;
}

use crate::{
    shared_memory::{
        shared_memory_reader::SharedMemoryReaderPlugin,
        shared_memory_writer::StateEmitterPlugin,
        shared_memory_web_extension::WebAdapterPlugin,
    },
    utils::{
        debug_functions::DebugFunctionsPlugin,
        objects::{DoorWinEntities, RoundStartTimestamp, GameStateLocal, GameConditions, PreloadedTextures, RenderTargetImage, FixedFullscreenActive, TexturePreloadManifest},
        systems_logic::SystemsLogicPlugin,
    },
};

#[cfg(target_arch = "wasm32")]
thread_local! {
    static WEB_REQUIRED_TEXTURE_INDICES: RefCell<Option<Vec<u32>>> = const { RefCell::new(None) };
}

/// Supply the complete set of texture indices referenced by the loaded trial
/// configuration. This must be called by the web controller before
/// `wasm_main()` starts Bevy.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_required_texture_indices(indices: Vec<u32>) {
    WEB_REQUIRED_TEXTURE_INDICES.with(|required| {
        *required.borrow_mut() = Some(indices);
    });
}

fn texture_preload_manifest() -> TexturePreloadManifest {
    #[cfg(target_arch = "wasm32")]
    {
        return WEB_REQUIRED_TEXTURE_INDICES.with(|required| {
            required
                .borrow()
                .as_ref()
                .map(|indices| TexturePreloadManifest::from_indices(indices.iter().copied()))
                .unwrap_or_default()
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    TexturePreloadManifest::default()
}

/// Build the Bevy App with all plugins and resources.
/// Shared between native (`main()`) and WASM (`wasm_main()`).
pub fn build_app() -> App {
    #[allow(unused_mut)]
    let mut win = Window {
        title: "Monkey 3D Game".into(),
        prevent_default_event_handling: true,
        #[cfg(not(target_arch = "wasm32"))]
        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
        present_mode: PresentMode::Fifo,
        ..default()
    };

    // Web: the canvas tracks the parent at native resolution
    #[cfg(target_arch = "wasm32")]
    {
        win.canvas = Some("#game-canvas".into());
        win.fit_canvas_to_parent = true;
    }

    let window = Some(win);

    let cursor = Some(CursorOptions {
        grab_mode: CursorGrabMode::None,
        visible: false,
        ..default()
    });

    let mut app = App::new();
    // Add continous window
    app.insert_resource(WinitSettings::continuous());

    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: window,
            primary_cursor_options: cursor,
            ..default()
        })
        .set(AssetPlugin {
            file_path: if cfg!(target_arch = "wasm32") {
                "game_node/assets".to_string()
            } else {
                "assets".to_string()
            },
            meta_check: AssetMetaCheck::Never, // Disable .meta file checking for faster load times
            ..default()
        }),

        LogDiagnosticsPlugin::default(),
        FrameTimeDiagnosticsPlugin::default(),
        EntityCountDiagnosticsPlugin::default(),
        SharedMemoryReaderPlugin,
        SystemsLogicPlugin,
        DebugFunctionsPlugin,
        StateEmitterPlugin,
        WebAdapterPlugin,
    ))
    .insert_resource(DoorWinEntities::default())
    .insert_resource(RoundStartTimestamp::default())
    .insert_resource(GameConditions::default())
    .insert_resource(GameStateLocal::default())
    .insert_resource(PreloadedTextures::default())
    .insert_resource(texture_preload_manifest())
    .insert_resource(crate::utils::warmup::WarmupState::default())
    .insert_resource(crate::utils::objects::CameraShakeState::default())
    .insert_resource(crate::utils::objects::LoadingCountdown::default())
    .insert_resource(RenderTargetImage::default())
    // Starts false: the offscreen render path runs on web and on native until
    // `setup_fixed_fullscreen` switches to exclusive fullscreen (native only),
    // at which point it flips true to skip the now-redundant offscreen path.
    .insert_resource(FixedFullscreenActive::default());

    app
}

/// WASM entry point – call this manually from JS after create_shared_memory_wasm()
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn wasm_main() {
    build_app().run();
}
