//! Web (WASM) shared memory implementation.
//!
//! Exposes SharedMemory to JavaScript through pointers and byte-offset maps
//! so that the JS controller can read/write the same memory regions as the
//! Bevy game running in the same WASM instance.

use crate::{SharedMemory, SharedGameState};
use wasm_bindgen::prelude::*;
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

// ---------------------------------------------------------------------------
// Helper wrapper for WASM side
// ---------------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Pointers to the three top-level regions
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Offset maps – returned as JS Objects { fieldName: byteOffset, … }
    // -----------------------------------------------------------------------

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
        set("check_alignment",  off!(cmd.check_alignment));
        set("reset",            off!(cmd.reset));
        set("blank_screen",     off!(cmd.blank_screen));
        set("stop_rendering",   off!(cmd.stop_rendering));
        set("animation_door",   off!(cmd.animation_door));
        set("animation_all_door", off!(cmd.animation_all_door));
        set("animation_colored", off!(cmd.animation_colored));

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
        set("decorations_color", off!(gs.decorations_color));
        set("textures", off!(gs.textures));

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

        // Dynamic fields
        set("frame_number",       off!(gs.frame_number));
        set("elapsed_secs",       off!(gs.elapsed_secs));
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

        set_f64("frame_number", def.frame_number.load(Relaxed) as f64);
        set_u32("elapsed_secs", def.elapsed_secs.load(Relaxed));
        set_u32("camera_radius", def.camera_radius.load(Relaxed));
        set_u32("camera_x", def.camera_x.load(Relaxed));
        set_u32("camera_y", def.camera_y.load(Relaxed));
        set_u32("camera_z", def.camera_z.load(Relaxed));
        set_u32("attempts", def.attempts.load(Relaxed));
        set_u32("current_alignment", def.current_alignment.load(Relaxed));
        set_u32("current_angle", def.current_angle.load(Relaxed));
        set_bool("is_animating", def.is_animating.load(Relaxed));
        set_bool("is_blank", def.is_blank.load(Relaxed));
        set_bool("is_rendering_stopped", def.is_rendering_stopped.load(Relaxed));
        set_u32("win_time", def.win_time.load(Relaxed));

        obj.into()
    }
}

// ---------------------------------------------------------------------------
// SharedMemoryHandle – thin wrapper for consistency with the native API
// ---------------------------------------------------------------------------

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
