//! Web (WASM) shared memory implementation using SharedArrayBuffer.

use crate::SharedMemory;
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

/// Helper wrapper for WASM side
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

    /// Get pointer to SharedCommands (for writing commands from JS)
    pub fn get_commands_ptr(&self) -> usize {
        unsafe { &(*self.ptr).commands as *const _ as usize }
    }

    /// Get pointer to SharedGameStructure (for reading/writing game state from JS)
    pub fn get_game_structure_ptr(&self) -> usize {
        unsafe { &(*self.ptr).game_structure as *const _ as usize }
    }

    /// Get offsets of fields within SharedGameStructure
    /// Returns a JS Object { "frame_number": offset, ... }
    pub fn get_game_structure_offsets(&self) -> JsValue {
        let base = unsafe { &(*self.ptr).game_structure as *const _ as usize };
        let gs = unsafe { &(*self.ptr).game_structure };

        let make_offset = |field_ptr: *const _| -> u32 {
             (field_ptr as usize - base) as u32
        };

        let offsets = js_sys::Object::new();
        let set = |key: &str, val: u32| {
            js_sys::Reflect::set(&offsets, &JsValue::from_str(key), &JsValue::from_f64(val as f64)).unwrap();
        };

        set("decoration_seeds", make_offset(&gs.decoration_seeds as *const _));
        set("base_radius", make_offset(&gs.base_radius as *const _));
        set("height", make_offset(&gs.height as *const _));
        set("start_orient", make_offset(&gs.start_orient as *const _));
        set("target_door", make_offset(&gs.target_door as *const _));
        set("colors", make_offset(&gs.colors as *const _));

        // Dynamic Constants
        set("decoration_count_min", make_offset(&gs.decoration_count_min as *const _));
        set("decoration_count_max", make_offset(&gs.decoration_count_max as *const _));
        set("decoration_size_min", make_offset(&gs.decoration_size_min as *const _));
        set("decoration_size_max", make_offset(&gs.decoration_size_max as *const _));

        set("cosine_alignment_threshold", make_offset(&gs.cosine_alignment_threshold as *const _));

        set("door_anim_fade_out", make_offset(&gs.door_anim_fade_out as *const _));
        set("door_anim_stay_open", make_offset(&gs.door_anim_stay_open as *const _));
        set("door_anim_fade_in", make_offset(&gs.door_anim_fade_in as *const _));

        set("main_spotlight_intensity", make_offset(&gs.main_spotlight_intensity as *const _));
        set("max_spotlight_intensity", make_offset(&gs.max_spotlight_intensity as *const _));
        set("ambient_brightness", make_offset(&gs.ambient_brightness as *const _));

        set("frame_number", make_offset(&gs.frame_number as *const _));
        set("elapsed_secs", make_offset(&gs.elapsed_secs as *const _));
        set("camera_radius", make_offset(&gs.camera_radius as *const _));
        set("camera_x", make_offset(&gs.camera_x as *const _));
        set("camera_y", make_offset(&gs.camera_y as *const _));
        set("camera_z", make_offset(&gs.camera_z as *const _));
        set("pyramid_yaw", make_offset(&gs.pyramid_yaw as *const _));
        set("attempts", make_offset(&gs.attempts as *const _));
        set("alignment", make_offset(&gs.alignment as *const _));
        set("current_angle", make_offset(&gs.current_angle as *const _));
        set("is_animating", make_offset(&gs.is_animating as *const _));
        set("win_time", make_offset(&gs.win_time as *const _));
        
        offsets.into()
    }
}

/// Handle to shared memory (wrapper for consistency with native API).
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
        "Shared memory not initialized in WASM"
    ))?;
    Ok(SharedMemoryHandle(mem))
}
