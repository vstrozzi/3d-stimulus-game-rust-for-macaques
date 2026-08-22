//! Web (WASM) shared memory implementation.
//!
//! Exposes SharedMemory to JavaScript through pointers and byte-offset maps
//! so that the JS controller can read/write the same memory regions as the
//! Bevy game running in the same WASM instance.

use crate::{SharedMemory, SharedGameState, RING_BUFFER_SIZE, DecorationShape, Texture};
use wasm_bindgen::prelude::*;
use strum::IntoEnumIterator;
use std::sync::OnceLock;

/// Global static instance of shared memory for WASM
static SHARED_MEMORY: OnceLock<SharedMemory> = OnceLock::new();

/// Allocate the shared memory on Rust side and return pointer.
/// JS will use this pointer to create a view.
#[wasm_bindgen]
pub fn create_shared_memory_wasm() -> *mut SharedMemory {
    let mem_ref = SHARED_MEMORY.get_or_init(|| SharedMemory::new());
    mem_ref as *const SharedMemory as *mut SharedMemory
}

/// Return the byte-size of SharedGameState so JS knows the extent of each region.
#[wasm_bindgen]
pub fn shared_game_state_byte_size() -> u32 {
    std::mem::size_of::<SharedGameState>() as u32
}

/// Single object containing every cross-controller constant — values, field
/// lists, FSM labels. Mirrors the attributes exposed by the Python module so
/// `controller.py` and `controller_main.js` can pull the same source.
#[wasm_bindgen]
pub fn controller_constants() -> JsValue {
    use crate::constants::controller_constants as cc;

    let obj = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        js_sys::Reflect::set(&obj, &JsValue::from_str(k), &v).unwrap();
    };

    set("SHM_NAME", JsValue::from_str(cc::SHM_NAME));
    set("POLLING_RATE_S", JsValue::from_f64(cc::POLLING_RATE_S as f64));
    set(
        "GAME_UNRESPONSIVENESS_THRESHOLD_S",
        JsValue::from_f64(cc::GAME_UNRESPONSIVENESS_THRESHOLD_S as f64),
    );
    set(
        "COLOR_SUGGESTION_COS_SIM",
        JsValue::from_f64(cc::COLOR_SUGGESTION_COS_SIM as f64),
    );
    set("DEFAULT_CAMERA_Y", JsValue::from_f64(cc::DEFAULT_CAMERA_Y as f64));
    set(
        "CAMERA_3D_INITIAL_RADIUS",
        JsValue::from_f64(crate::constants::camera_3d_constants::CAMERA_3D_INITIAL_RADIUS as f64),
    );
    set("N_FACES", JsValue::from_f64(cc::N_FACES as f64));
    set("N_COLOR_CHANNELS", JsValue::from_f64(cc::N_COLOR_CHANNELS as f64));
    set("N_COLOR_FLOATS", JsValue::from_f64(cc::N_COLOR_FLOATS as f64));
    set("N_START_ORIENTS", JsValue::from_f64(cc::N_START_ORIENTS as f64));
    set(
        "MAX_SESSION_DURATION_MIN",
        JsValue::from_f64(cc::MAX_SESSION_DURATION_MIN as f64),
    );

    let to_array = |items: &[&str]| -> JsValue {
        let arr = js_sys::Array::new();
        for s in items {
            arr.push(&JsValue::from_str(s));
        }
        arr.into()
    };
    set("LOGGED_STATE_FIELDS", to_array(cc::LOGGED_STATE_FIELDS));
    set("CONTROLLER_META_FIELDS", to_array(cc::CONTROLLER_META_FIELDS));
    set("FSM_STATES", to_array(cc::FSM_STATES));
    set("PROCEEDING_VALUES", to_array(cc::PROCEEDING_VALUES));

    obj.into()
}

/// Constants and defaults consumed by the trial editor (trial_editor.html).
/// Lets the editor import the same source of truth as the game and the
/// controllers, instead of hand-mirroring values from shared/src/lib.rs.
#[wasm_bindgen]
pub fn editor_constants() -> JsValue {
    use crate::constants::{
        camera_3d_constants as cam, controller_constants as cc, fog_constants as fog,
        game_constants as gc, lighting_constants as light, pyramid_constants as pyr,
        sound_constants as snd,
    };

    let obj = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        js_sys::Reflect::set(&obj, &JsValue::from_str(k), &v).unwrap();
    };

    // Enum variant names — single source of truth for the editor's dropdowns.
    let texture_names = js_sys::Array::new();
    for t in Texture::iter() {
        texture_names.push(&JsValue::from_str(&t.to_string()));
    }
    set("TEXTURE_NAMES", texture_names.into());

    let shape_names = js_sys::Array::new();
    for s in DecorationShape::iter() {
        shape_names.push(&JsValue::from_str(&s.to_string()));
    }
    set("SHAPE_NAMES", shape_names.into());

    // Pyramid + lighting + camera defaults. Editor's "+ Level" button
    // populates these into a new level.
    set("PYRAMID_BASE_RADIUS", JsValue::from_f64(pyr::PYRAMID_BASE_RADIUS as f64));
    set("PYRAMID_HEIGHT", JsValue::from_f64(pyr::PYRAMID_HEIGHT as f64));
    set(
        "PYRAMID_TARGET_DOOR_INDEX",
        JsValue::from_f64(pyr::PYRAMID_TARGET_DOOR_INDEX as f64),
    );
    set("DOOR_ANIM_FADE_OUT", JsValue::from_f64(pyr::DOOR_ANIM_FADE_OUT as f64));
    set("DOOR_ANIM_STAY_OPEN", JsValue::from_f64(pyr::DOOR_ANIM_STAY_OPEN as f64));
    set("DOOR_ANIM_FADE_IN", JsValue::from_f64(pyr::DOOR_ANIM_FADE_IN as f64));
    set("COSINE_ALIGNMENT_TO_WIN", JsValue::from_f64(gc::COSINE_ALIGNMENT_TO_WIN as f64));
    set(
        "SPOTLIGHT_LIGHT_INTENSITY",
        JsValue::from_f64(light::SPOTLIGHT_LIGHT_INTENSITY as f64),
    );
    set(
        "GLOBAL_AMBIENT_LIGHT_INTENSITY",
        JsValue::from_f64(light::GLOBAL_AMBIENT_LIGHT_INTENSITY as f64),
    );
    set(
        "MAX_SPOTLIGHT_INTENSITY",
        JsValue::from_f64(light::MAX_SPOTLIGHT_INTENSITY as f64),
    );
    set("CAMERA_3D_INITIAL_RADIUS", JsValue::from_f64(cam::CAMERA_3D_INITIAL_RADIUS as f64));
    set("CAMERA_3D_SPEED_ROTATE", JsValue::from_f64(cam::CAMERA_3D_SPEED_ROTATE as f64));
    set("DEFAULT_CAMERA_Y", JsValue::from_f64(cc::DEFAULT_CAMERA_Y as f64));
    set(
        "CAMERA_ROTATION_SENSE_DEFAULT",
        JsValue::from_f64(cam::CAMERA_ROTATION_SENSE_DEFAULT as f64),
    );
    set("N_FACES", JsValue::from_f64(cc::N_FACES as f64));
    set("N_START_ORIENTS", JsValue::from_f64(cc::N_START_ORIENTS as f64));

    // Audio + atmosphere per-level defaults (mirrors SharedGameState::new()).
    set("SOUND_EFFECTS_VOLUME", JsValue::from_f64(snd::SOUND_EFFECTS_VOLUME as f64));
    set("BACKGROUND_MUSIC_VOLUME", JsValue::from_f64(snd::BACKGROUND_MUSIC_VOLUME as f64));
    set("FOG_ENABLED", JsValue::from_bool(fog::FOG_ENABLED));
    set("FOG_THICKNESS_BASE", JsValue::from_f64(fog::FOG_THICKNESS_BASE as f64));
    set("FIREFLY_COUNT", JsValue::from_f64(fog::FIREFLY_COUNT as f64));
    set("FIREFLY_SIZE", JsValue::from_f64(fog::FIREFLY_SIZE as f64));
    set("FIREFLY_EXPAND_SECS", JsValue::from_f64(fog::FIREFLY_EXPAND_SECS as f64));

    // Per-face arrays (length N_FACES). Editor uses these to populate a new
    // object's default shape/size/count/color/seed values.
    let to_f64_arr = |xs: &[f32]| -> JsValue {
        let a = js_sys::Array::new();
        for &v in xs { a.push(&JsValue::from_f64(v as f64)); }
        a.into()
    };
    let to_u32_arr = |xs: &[u32]| -> JsValue {
        let a = js_sys::Array::new();
        for &v in xs { a.push(&JsValue::from_f64(v as f64)); }
        a.into()
    };
    let to_u64_arr = |xs: &[u64]| -> JsValue {
        let a = js_sys::Array::new();
        for &v in xs { a.push(&JsValue::from_f64(v as f64)); }
        a.into()
    };
    let to_i32_arr = |xs: &[i32]| -> JsValue {
        let a = js_sys::Array::new();
        for &v in xs { a.push(&JsValue::from_f64(v as f64)); }
        a.into()
    };
    let to_color_arr = |xs: &[[f32; 4]]| -> JsValue {
        let outer = js_sys::Array::new();
        for face in xs {
            let inner = js_sys::Array::new();
            for &c in face { inner.push(&JsValue::from_f64(c as f64)); }
            outer.push(&inner);
        }
        outer.into()
    };
    let to_tex_idx_arr = |xs: &[Texture]| -> JsValue {
        let a = js_sys::Array::new();
        for &t in xs { a.push(&JsValue::from_f64(t as u32 as f64)); }
        a.into()
    };
    let to_shape_idx_arr = |xs: &[DecorationShape]| -> JsValue {
        let a = js_sys::Array::new();
        for &s in xs { a.push(&JsValue::from_f64(s as u32 as f64)); }
        a.into()
    };

    set("PYRAMID_COLORS", to_color_arr(&pyr::PYRAMID_COLORS));
    set("PYRAMID_TEXTURES", to_tex_idx_arr(&pyr::PYRAMID_TEXTURES));
    set("PYRAMID_DECORATIONS_COUNT", to_u32_arr(&pyr::PYRAMID_DECORATIONS_COUNT));
    set("PYRAMID_DECORATIONS_SIZE", to_f64_arr(&pyr::PYRAMID_DECORATIONS_SIZE));
    set("PYRAMID_DECORATIONS_SHAPE", to_shape_idx_arr(&pyr::PYRAMID_DECORATIONS_SHAPE));
    set("PYRAMID_DECORATIONS_COLOR", to_color_arr(&pyr::PYRAMID_DECORATIONS_COLOR));
    set("PYRAMID_DECORATIONS_TEXTURE", to_tex_idx_arr(&pyr::PYRAMID_DECORATIONS_TEXTURE));
    set("PYRAMID_DECORATIONS_THICKNESS", to_f64_arr(&pyr::PYRAMID_DECORATIONS_THICKNESS));
    set("PYRAMID_DECORATIONS_ROTATION", to_i32_arr(&pyr::PYRAMID_DECORATIONS_ROTATION));
    set("DECORATIONS_SEEDS", to_u64_arr(&gc::DECORATIONS_SEEDS));

    obj.into()
}

// Helper wrapper for WASM side
#[wasm_bindgen]
pub struct WebSharedMemory {
    ptr: *mut SharedMemory,
}

#[wasm_bindgen]
impl WebSharedMemory {
    #[wasm_bindgen(constructor)]
    pub fn new(ptr: usize) -> Self {
        Self { ptr: ptr as *mut SharedMemory }
    }

    /// Get base pointer to SharedMemory
    pub fn get_ptr(&self) -> usize {
        self.ptr as usize
    }

    /// Pointer to command_seq (AtomicU64, Controller writes)
    pub fn get_command_seq_ptr(&self) -> usize {
        unsafe { &(*self.ptr).command_seq as *const _ as usize }
    }

    /// Pointer to command_ack (AtomicU64, Game writes)
    pub fn get_command_ack_ptr(&self) -> usize {
        unsafe { &(*self.ptr).command_ack as *const _ as usize }
    }

    /// Pointer to SharedCommands (Controller → Game)
    pub fn get_commands_ptr(&self) -> usize {
        unsafe { &(*self.ptr).commands as *const _ as usize }
    }

    /// Pointer to game_structure_game (Game → Controller: game writes, controller reads)
    pub fn get_game_structure_game_ptr(&self) -> usize {
        unsafe { &(*self.ptr).game_structure_game as *const _ as usize }
    }

    /// Pointer to game_structure_control (Controller → Game: controller writes, game reads on reset)
    pub fn get_game_structure_control_ptr(&self) -> usize {
        unsafe { &(*self.ptr).game_structure_control as *const _ as usize }
    }

    /// Pointer to frame_ring_buffer.write_head (AtomicU64)
    pub fn get_frame_buffer_write_head_ptr(&self) -> usize {
        unsafe { &(*self.ptr).frame_ring_buffer.write_head as *const _ as usize }
    }

    /// Pointer to frame_ring_buffer.entries[0] (first SharedGameState slot)
    pub fn get_frame_buffer_entries_ptr(&self) -> usize {
        unsafe { &(*self.ptr).frame_ring_buffer.entries[0] as *const _ as usize }
    }

    /// Byte size of one ring buffer entry (= SharedGameState).
    pub fn get_frame_buffer_entry_stride(&self) -> u32 {
        std::mem::size_of::<SharedGameState>() as u32
    }

    /// Number of slots in the ring buffer.
    pub fn get_frame_buffer_size(&self) -> u32 {
        RING_BUFFER_SIZE as u32
    }

    /// Byte offsets of every field inside SharedCommands (relative to its start).
    pub fn get_commands_offsets(&self) -> JsValue {
        let base = unsafe { &(*self.ptr).commands as *const _ as usize };
        let cmd  = unsafe { &(*self.ptr).commands };

        macro_rules! off {
            ($field:expr) => {
                ((&$field as *const _ as *const u8 as usize) - base) as u32
            };
        }

        let obj = js_sys::Object::new();
        let set = |k: &str, v: u32| {
            js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from_f64(v as f64)).unwrap();
        };

        set("rotate_left",      off!(cmd.rotate_left));
        set("rotate_right",     off!(cmd.rotate_right));
        set("zoom_in",          off!(cmd.zoom_in));
        set("zoom_out",         off!(cmd.zoom_out));
        set("check",            off!(cmd.check_alignment)); // exposed as "check" to match Python interface
        set("reset",            off!(cmd.reset));
        set("toggle_blank",     off!(cmd.toggle_blank));
        set("toggle_stop_rendering", off!(cmd.toggle_stop_rendering));
        set("animation_door",   off!(cmd.animation_door));
        set("animation_all_door", off!(cmd.animation_all_door));
        set("animation_colored", off!(cmd.animation_colored));
        set("shake",            off!(cmd.shake));

        obj.into()
    }

    /// Byte offsets of every field inside SharedGameState (works for both
    /// game_structure_game and game_structure_control since they have identical layout).
    pub fn get_game_state_offsets(&self) -> JsValue {
        let base = unsafe { &(*self.ptr).game_structure_game as *const _ as usize };
        let gs   = unsafe { &(*self.ptr).game_structure_game };

        // Cast via *const u8 so generic pointer inference doesn't conflict across types
        macro_rules! off {
            ($field:expr) => {
                ((&$field as *const _ as *const u8 as usize) - base) as u32
            };
        }

        let obj = js_sys::Object::new();
        let set = |k: &str, v: u32| {
            js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from_f64(v as f64)).unwrap();
        };

        // Fixed trial fields
        set("base_radius",    off!(gs.base_radius));
        set("height",         off!(gs.height));
        set("start_orient",   off!(gs.start_orient));
        set("target_door",    off!(gs.target_door));
        set("colors",         off!(gs.colors));
        set("decorations_count", off!(gs.decorations_count));
        set("decorations_size",  off!(gs.decorations_size));
        set("decorations_seeds", off!(gs.decorations_seeds));
        set("decorations_shape", off!(gs.decorations_shape));
        set("decorations_texture", off!(gs.decorations_texture));
        set("decorations_thickness", off!(gs.decorations_thickness));
        set("decorations_rotation", off!(gs.decorations_rotation));
        set("decorations_color", off!(gs.decorations_color));
        set("textures", off!(gs.textures));
        set("camera_speed_rotate", off!(gs.camera_speed_rotate));
        set("camera_rotation_sense", off!(gs.camera_rotation_sense));

        set("cosine_alignment_threshold", off!(gs.cosine_alignment_threshold));

        // Animation durations
        set("door_anim_fade_out",  off!(gs.door_anim_fade_out));
        set("door_anim_stay_open", off!(gs.door_anim_stay_open));
        set("door_anim_fade_in",   off!(gs.door_anim_fade_in));

        // Lighting
        set("main_spotlight_intensity", off!(gs.main_spotlight_intensity));
        set("ambient_brightness",       off!(gs.ambient_brightness));
        set("max_spotlight_intensity",  off!(gs.max_spotlight_intensity));

        // Progress bar state
        set("progress_bar_size", off!(gs.progress_bar_size));
        set("progress_bar_cur_size", off!(gs.progress_bar_cur_size));
        set("score_bar_value", off!(gs.score_bar_value));
        set("score_bar_max", off!(gs.score_bar_max));
        set("session_time_left", off!(gs.session_time_left));
        set("correct_streak", off!(gs.correct_streak));
        set("shake_amplitude", off!(gs.shake_amplitude));
        set("shake_duration", off!(gs.shake_duration));
        set("sound_effects_volume", off!(gs.sound_effects_volume));
        set("background_music_volume", off!(gs.background_music_volume));
        set("fog_enabled", off!(gs.fog_enabled));
        set("fog_thickness_base", off!(gs.fog_thickness_base));
        set("firefly_count", off!(gs.firefly_count));
        set("firefly_size", off!(gs.firefly_size));
        set("firefly_expand_secs", off!(gs.firefly_expand_secs));

        // Dynamic fields
        set("frame_number",       off!(gs.frame_number));
        set("render_frame_number", off!(gs.render_frame_number));
        set("elapsed_secs",       off!(gs.elapsed_secs));
        set("render_elapsed_secs", off!(gs.render_elapsed_secs));
        set("present_elapsed_secs", off!(gs.present_elapsed_secs));
        set("photodiode_white",   off!(gs.photodiode_white));
        set("camera_radius",      off!(gs.camera_radius));
        set("camera_x",           off!(gs.camera_x));
        set("camera_y",           off!(gs.camera_y));
        set("camera_z",           off!(gs.camera_z));
        set("attempts",           off!(gs.attempts));
        set("current_alignment",  off!(gs.current_alignment));
        set("current_angle",      off!(gs.current_angle));
        set("is_animating",       off!(gs.is_animating));
        set("is_blank",           off!(gs.is_blank));
        set("is_rendering_stopped", off!(gs.is_rendering_stopped));
        set("is_scene_ready",     off!(gs.is_scene_ready));
        set("win_time",           off!(gs.win_time));

        

        obj.into()
    }

    /// Return default values of SharedGameState::new() as a JS object.
    /// Equivalent to Python's `read_default_game_state()`.
    pub fn get_default_game_state(&self) -> JsValue {
        let def = SharedGameState::new();
        let obj = js_sys::Object::new();
        let set_u32 = |k: &str, v: u32| {
            js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from_f64(v as f64)).unwrap();
        };
        let set_f64 = |k: &str, v: f64| {
            js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from_f64(v)).unwrap();
        };
        let set_bool = |k: &str, v: bool| {
            js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from_bool(v)).unwrap();
        };

        use std::sync::atomic::Ordering::Relaxed;

        set_u32("base_radius",    def.base_radius.load(Relaxed));
        set_u32("height",         def.height.load(Relaxed));
        set_u32("start_orient",   def.start_orient.load(Relaxed));
        set_u32("target_door",    def.target_door.load(Relaxed));

        // Colors as flat array of 12 u32
        let colors = js_sys::Array::new();
        for i in 0..12 { colors.push(&JsValue::from_f64(def.colors[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("colors"), &colors).unwrap();

        let textures = js_sys::Array::new();
        for i in 0..3 { textures.push(&JsValue::from_f64(def.textures[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("textures"), &textures).unwrap();

        let dec_count = js_sys::Array::new();
        for i in 0..3 { dec_count.push(&JsValue::from_f64(def.decorations_count[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("decorations_count"), &dec_count).unwrap();

        let dec_size = js_sys::Array::new();
        for i in 0..3 { dec_size.push(&JsValue::from_f64(def.decorations_size[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("decorations_size"), &dec_size).unwrap();

        let dec_seeds = js_sys::Array::new();
        for i in 0..3 { dec_seeds.push(&JsValue::from_f64(def.decorations_seeds[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("decorations_seeds"), &dec_seeds).unwrap();

        let dec_shape = js_sys::Array::new();
        for i in 0..3 { dec_shape.push(&JsValue::from_f64(def.decorations_shape[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("decorations_shape"), &dec_shape).unwrap();

        let dec_texture = js_sys::Array::new();
        for i in 0..3 { dec_texture.push(&JsValue::from_f64(def.decorations_texture[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("decorations_texture"), &dec_texture).unwrap();

        let dec_thickness = js_sys::Array::new();
        for i in 0..3 { dec_thickness.push(&JsValue::from_f64(def.decorations_thickness[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("decorations_thickness"), &dec_thickness).unwrap();

        let dec_rotation = js_sys::Array::new();
        for i in 0..3 { dec_rotation.push(&JsValue::from_f64(def.decorations_rotation[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("decorations_rotation"), &dec_rotation).unwrap();

        let dec_color = js_sys::Array::new();
        for i in 0..12 { dec_color.push(&JsValue::from_f64(def.decorations_color[i].load(Relaxed) as f64)); }
        js_sys::Reflect::set(&obj, &JsValue::from_str("decorations_color"), &dec_color).unwrap();

        set_u32("cosine_alignment_threshold", def.cosine_alignment_threshold.load(Relaxed));
        set_u32("door_anim_fade_out",  def.door_anim_fade_out.load(Relaxed));
        set_u32("door_anim_stay_open", def.door_anim_stay_open.load(Relaxed));
        set_u32("door_anim_fade_in",   def.door_anim_fade_in.load(Relaxed));

        set_u32("main_spotlight_intensity", def.main_spotlight_intensity.load(Relaxed));
        set_u32("ambient_brightness",       def.ambient_brightness.load(Relaxed));
        set_u32("max_spotlight_intensity",  def.max_spotlight_intensity.load(Relaxed));

        set_u32("progress_bar_size", def.progress_bar_size.load(Relaxed));
        set_u32("progress_bar_cur_size", def.progress_bar_cur_size.load(Relaxed));
        set_u32("score_bar_value", def.score_bar_value.load(Relaxed));
        set_u32("score_bar_max", def.score_bar_max.load(Relaxed));
        set_u32("session_time_left", def.session_time_left.load(Relaxed));
        set_u32("correct_streak", def.correct_streak.load(Relaxed));
        set_u32("shake_amplitude", def.shake_amplitude.load(Relaxed));
        set_u32("shake_duration", def.shake_duration.load(Relaxed));
        set_u32("sound_effects_volume", def.sound_effects_volume.load(Relaxed));
        set_u32("background_music_volume", def.background_music_volume.load(Relaxed));
        set_bool("fog_enabled", def.fog_enabled.load(Relaxed));
        set_u32("fog_thickness_base", def.fog_thickness_base.load(Relaxed));
        set_u32("firefly_count", def.firefly_count.load(Relaxed));
        set_u32("firefly_size", def.firefly_size.load(Relaxed));
        set_u32("firefly_expand_secs", def.firefly_expand_secs.load(Relaxed));

        set_f64("frame_number", def.frame_number.load(Relaxed) as f64);
        set_f64("render_frame_number", def.render_frame_number.load(Relaxed) as f64);
        set_u32("elapsed_secs", def.elapsed_secs.load(Relaxed));
        set_u32("render_elapsed_secs", def.render_elapsed_secs.load(Relaxed));
        set_u32("present_elapsed_secs", def.present_elapsed_secs.load(Relaxed));
        set_bool("photodiode_white", def.photodiode_white.load(Relaxed));
        set_u32("camera_radius", def.camera_radius.load(Relaxed));
        set_u32("camera_x", def.camera_x.load(Relaxed));
        set_u32("camera_y", def.camera_y.load(Relaxed));
        set_u32("camera_z", def.camera_z.load(Relaxed));
        set_u32("camera_speed_rotate", def.camera_speed_rotate.load(Relaxed));
        set_u32("camera_rotation_sense", def.camera_rotation_sense.load(Relaxed));
        set_u32("attempts", def.attempts.load(Relaxed));
        set_u32("current_alignment", def.current_alignment.load(Relaxed));
        set_u32("current_angle", def.current_angle.load(Relaxed));
        set_bool("is_animating", def.is_animating.load(Relaxed));
        set_bool("is_blank", def.is_blank.load(Relaxed));
        set_bool("is_rendering_stopped", def.is_rendering_stopped.load(Relaxed));
        set_bool("is_scene_ready", def.is_scene_ready.load(Relaxed));
        set_u32("win_time", def.win_time.load(Relaxed));

        obj.into()
    }
}

// SharedMemoryHandle, wrapper for consistency with the native API
#[derive(Clone, Copy)]
pub struct SharedMemoryHandle(&'static SharedMemory);

impl SharedMemoryHandle {
    pub fn get(&self) -> &'static SharedMemory {
        self.0
    }
}

pub fn open_shared_memory(_name: &str) -> std::io::Result<SharedMemoryHandle> {
    let mem = SHARED_MEMORY.get().ok_or(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Shared memory not initialized in WASM",
    ))?;
    Ok(SharedMemoryHandle(mem))
}
