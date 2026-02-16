//! Declaration of the utils modules for monkey_3d_game.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    window::*,
};

// Shared memory helpers
pub mod shared_memory{
    pub mod shared_memory_reader;
    pub mod shared_memory_writer;
    pub mod shared_memory_web_extension;
}
/// Game functions
pub mod utils {
    pub mod camera;
    pub mod utils;
    pub mod ui;
    pub mod handle_commands;
    pub mod debug_functions;
    pub mod game_functions;
    pub mod macros;
    pub mod objects;
    pub mod pyramid;
    pub mod setup;
    pub mod systems_logic;
}