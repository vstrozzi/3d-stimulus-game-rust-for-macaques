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
