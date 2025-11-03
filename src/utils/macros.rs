/// MACROS
/// Cross-platform logging.
/// This is used to enable different debugging procedures depending on webbrowser or native development.

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {{
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!($($t)*).into());
        #[cfg(not(target_arch = "wasm32"))]
        println!($($t)*);
    }};
}
