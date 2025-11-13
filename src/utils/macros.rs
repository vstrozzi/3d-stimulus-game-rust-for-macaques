/// This file contains macros for the game, such as for cross-platform logging.

/// A macro for cross-platform logging.
/// This macro allows for logging messages that will work on both native and web platforms.
/// It uses conditional compilation to switch between `println!` for native targets
/// and `web_sys::console::log_1` for the `wasm32` target.
#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {{
        // If the target architecture is wasm32, use the web console for logging.
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!($($t)*).into());
        // Otherwise, use the standard println! macro for logging.
        #[cfg(not(target_arch = "wasm32"))]
        println!($($t)*);
    }};
}
