//! Declaration of the utils modules for monkey_3d_game.

// Bevy systems idiomatically accept many resources/queries and use deeply
// nested Query types; clippy's defaults flag these on every system.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;


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
        objects::{DoorWinEntities, RoundStartTimestamp, GameStateLocal, GameConditions, PreloadedTextures, RenderTargetImage, FixedFullscreenActive},
        systems_logic::SystemsLogicPlugin,
    },
};

/// Web canvas backing-store resolution: the screen's physical resolution
/// scaled to fit inside `FIXED_RENDER_WIDTH` × `FIXED_RENDER_HEIGHT` (aspect
/// preserved, never upscaled). Falls back to the box itself if the screen
/// can't be queried.
#[cfg(target_arch = "wasm32")]
fn web_capped_resolution() -> (u32, u32) {
    use shared::constants::render_constants::{FIXED_RENDER_HEIGHT, FIXED_RENDER_WIDTH};
    let (max_w, max_h) = (FIXED_RENDER_WIDTH as f64, FIXED_RENDER_HEIGHT as f64);

    let dims = web_sys::window().and_then(|w| {
        let dpr = w.device_pixel_ratio();
        let screen = w.screen().ok()?;
        let sw = screen.width().ok()? as f64 * dpr;
        let sh = screen.height().ok()? as f64 * dpr;
        (sw > 0.0 && sh > 0.0).then_some((sw, sh))
    });

    let Some((sw, sh)) = dims else {
        return (FIXED_RENDER_WIDTH, FIXED_RENDER_HEIGHT);
    };
    let scale = (max_w / sw).min(max_h / sh).min(1.0);
    (
        ((sw * scale).round() as u32).max(1),
        ((sh * scale).round() as u32).max(1),
    )
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

    // Web: the 3D scene renders into #game-canvas. When fixed-resolution
    // rendering is on, cap the canvas backing store to the FIXED_RENDER_* box
    // (aspect-matched to the screen) and let CSS stretch it to fill the
    // viewport — the web analogue of native exclusive fullscreen, so the whole
    // pipeline (scene + UI + present) runs at ~1080p instead of the screen's
    // native resolution. Otherwise track the parent element at native res.
    #[cfg(target_arch = "wasm32")]
    {
        win.canvas = Some("#game-canvas".into());
        if shared::constants::render_constants::RENDER_AT_FIXED_RESOLUTION {
            let (w, h) = web_capped_resolution();
            win.resolution = WindowResolution::new(w, h).with_scale_factor_override(1.0);
        } else {
            win.fit_canvas_to_parent = true;
        }
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
    .insert_resource(crate::utils::warmup::WarmupState::default())
    .insert_resource(crate::utils::objects::CameraShakeState::default())
    .insert_resource(crate::utils::objects::LoadingCountdown::default())
    .insert_resource(RenderTargetImage::default())
    // On web the canvas-backing-store cap above already runs the whole pipeline
    // at the fixed resolution, so the offscreen render path is redundant —
    // mark it active to skip it. On native it starts false and is flipped on
    // by `setup_fixed_fullscreen` once exclusive fullscreen succeeds.
    .insert_resource(FixedFullscreenActive(
        cfg!(target_arch = "wasm32")
            && shared::constants::render_constants::RENDER_AT_FIXED_RESOLUTION,
    ));

    app
}

/// WASM entry point – call this manually from JS after create_shared_memory_wasm()
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn wasm_main() {
    build_app().run();
}