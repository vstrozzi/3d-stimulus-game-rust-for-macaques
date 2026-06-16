//! Camera handler
use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::{PrimaryWindow, WindowResized};
#[cfg(not(target_arch = "wasm32"))]
use bevy::window::{Monitor, MonitorSelection, PrimaryMonitor, VideoModeSelection, WindowMode};
use bevy::camera::RenderTarget;
use crate::shared_memory::shared_memory_reader::PendingCommands;
use crate::utils::objects::{
    CameraShakeState, FixedFullscreenActive, GameStateLocal, PersistentCamera,
    RenderBackdrop, RenderTargetImage, UpscaleCamera,
};
use shared::constants::render_constants::{
    FIXED_RENDER_HEIGHT, FIXED_RENDER_WIDTH, RENDER_AT_FIXED_RESOLUTION,
};

/// Game camera, persistent across levels and trials
pub fn spawn_persistent_camera(mut commands: Commands, local_game_struct: Res<GameStateLocal>) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(
            f32::from_bits(local_game_struct.0.camera_x),
            f32::from_bits(local_game_struct.0.camera_y),
            f32::from_bits(local_game_struct.0.camera_z),
        )
        .looking_at(Vec3::ZERO, Vec3::Y),
        PersistentCamera,
    ));
}

/// Internal render size: native aspect ratio, scaled to fit inside the
/// `FIXED_RENDER_WIDTH` × `FIXED_RENDER_HEIGHT` box. Bounding both dimensions
/// keeps the pixel count in check on wide displays (height-only capping let
/// the width balloon). Scale is clamped to <= 1 so we never exceed native.
/// `None` until the window reports a real size.
fn fixed_target_size(window: &Window) -> Option<(u32, u32)> {
    let w = window.physical_width();
    let h = window.physical_height();
    if w == 0 || h == 0 {
        return None;
    }
    let scale = (FIXED_RENDER_WIDTH as f32 / w as f32)
        .min(FIXED_RENDER_HEIGHT as f32 / h as f32)
        .min(1.0);
    let width = ((w as f32 * scale).round() as u32).max(1);
    let height = ((h as f32 * scale).round() as u32).max(1);
    Some((width, height))
}

/// On native, switch to exclusive fullscreen at a video mode whose height is
/// capped at `FIXED_RENDER_HEIGHT`, matches the monitor's aspect ratio, and
/// keeps its refresh rate. This makes the whole pipeline (3D scene + UI +
/// present) run at ~1080p — the cheap path mirroring gave us for free — while
/// staying adaptive to the monitor's form factor (height fixed, width follows
/// aspect). Runs once, as soon as the monitor reports its video modes. If no
/// suitable lower mode exists it leaves `BorderlessFullscreen` and the
/// offscreen render path active as a fallback.
#[cfg(not(target_arch = "wasm32"))]
pub fn setup_fixed_fullscreen(
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut active: ResMut<FixedFullscreenActive>,
    mut done: Local<bool>,
) {
    if !RENDER_AT_FIXED_RESOLUTION || *done {
        return;
    }
    let Ok(monitor) = monitors.single() else { return };
    if monitor.video_modes.is_empty() {
        return; // modes not enumerated yet
    }
    let (nat_w, nat_h) = (monitor.physical_width, monitor.physical_height);
    if nat_w == 0 || nat_h == 0 {
        return;
    }
    let native_aspect = nat_w as f32 / nat_h as f32;

    // Height-capped, aspect-matched, same refresh rate (so vsync timing is
    // unchanged). Pick the tallest such mode — closest to the cap.
    let best = monitor
        .video_modes
        .iter()
        .copied()
        .filter(|m| {
            let (mw, mh) = (m.physical_size.x, m.physical_size.y);
            mh <= FIXED_RENDER_HEIGHT
                && (mw as f32 / mh as f32 - native_aspect).abs() < 0.02
                && monitor
                    .refresh_rate_millihertz
                    .map_or(true, |hz| m.refresh_rate_millihertz.abs_diff(hz) < 1000)
        })
        .max_by_key(|m| m.physical_size.y);

    *done = true;
    let Some(mode) = best else { return }; // keep borderless + offscreen fallback

    if let Ok(mut window) = windows.single_mut() {
        window.mode =
            WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Specific(mode));
        active.0 = true;
    }
}

/// Build the offscreen image the 3D scene renders into.
fn make_render_image(width: u32, height: u32) -> Image {
    let size = Extent3d { width, height, depth_or_array_layers: 1 };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 0],
        // Rgba8UnormSrgb is render-attachment-capable on both native wgpu and
        // WebGPU; Bgra8UnormSrgb is not a portable render target on the web.
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image
}

/// When `RENDER_AT_FIXED_RESOLUTION` is on, retarget the 3D camera to a
/// fixed-resolution offscreen image and spawn a native-res 2D camera that
/// upscales it (with the UI on top). Runs once, as soon as the window has a
/// real size (borderless-fullscreen)
pub fn setup_fixed_resolution(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut target: ResMut<RenderTargetImage>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera_q: Query<&mut RenderTarget, With<PersistentCamera>>,
    mut done: Local<bool>,
) {
    if !RENDER_AT_FIXED_RESOLUTION || *done {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some((width, height)) = fixed_target_size(window) else { return };
    let Ok(mut render_target) = camera_q.single_mut() else { return };

    let handle = images.add(make_render_image(width, height));

    // 3D scene now renders into the offscreen image instead of the window.
    *render_target = handle.clone().into();

    // 2D camera draws to the window: upscaled image as backdrop + UI on top.
    commands.spawn((
        Camera2d,
        Camera { order: 1, ..default() },
        IsDefaultUiCamera,
        UpscaleCamera,
    ));

    // Full-window backdrop showing the upscaled image, behind all UI
    // (blank screen = 1000, photodiode = 1001).
    commands.spawn((
        ImageNode::new(handle.clone()),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(-1),
        RenderBackdrop,
    ));

    target.handle = Some(handle);
    target.width = width;
    target.height = height;
    *done = true;
}

/// Resize the offscreen image when the window aspect changes (web canvas
/// resize). The backdrop node fills 100% of the window, so pure scale changes
/// need no work — only an aspect change reallocates the texture.
pub fn on_window_resized(
    mut resize_events: MessageReader<WindowResized>,
    mut images: ResMut<Assets<Image>>,
    mut target: ResMut<RenderTargetImage>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if !RENDER_AT_FIXED_RESOLUTION || resize_events.is_empty() {
        return;
    }
    resize_events.clear();

    let Some(handle) = target.handle.clone() else { return };
    let Ok(window) = windows.single() else { return };
    let Some((width, height)) = fixed_target_size(window) else { return };

    if width == target.width && height == target.height {
        return; // aspect unchanged; backdrop restretches for free
    }
    if let Some(image) = images.get_mut(&handle) {
        image.resize(Extent3d { width, height, depth_or_array_layers: 1 });
        target.width = width;
        target.height = height;
    }
}

pub fn handle_camera_shake(
    pending: Res<PendingCommands>,
    local_game_struct: Res<GameStateLocal>,
    time: Res<Time>,
    mut shake: ResMut<CameraShakeState>,
    mut camera_query: Query<&mut Transform, With<PersistentCamera>>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };

    if pending.shake {
        let amplitude = f32::from_bits(local_game_struct.0.shake_amplitude);
        let duration = f32::from_bits(local_game_struct.0.shake_duration);
        if amplitude > 0.0 && duration > 0.0 {
            shake.start = Some(time.elapsed());
            shake.amplitude = amplitude;
            shake.duration = duration;
        }
    }

    let Some(start) = shake.start else { return };
    let elapsed = (time.elapsed() - start).as_secs_f32();
    if elapsed >= shake.duration {
        shake.start = None;
        return;
    }

    // Pitch + roll jitter only — yaw would change handle_zoom's extracted
    // euler-yaw next frame and drift the orbit position. look_at resets
    // rotation each frame, so no accumulation.
    let decay = (-4.0 * elapsed / shake.duration).exp();
    let t = time.elapsed_secs();
    let pitch = shake.amplitude * decay * (t * 37.0).sin() * 0.05;
    let roll  = shake.amplitude * decay * (t * 53.0 + 1.3).sin() * 0.05;
    transform.rotate_local_x(pitch);
    transform.rotate_local_z(roll);
}