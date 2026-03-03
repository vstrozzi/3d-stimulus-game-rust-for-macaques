//! Start-up for the monkey_3d_game, with window, plugins, and resources.
//! This is the Game Node. It receives commands from the Controller and emits state via Shared Memory.
//! On native, this `main()` function builds and runs the Bevy App.
//! On WASM, the JS controller calls `wasm_main()` (exported from lib.rs) instead.

fn main() {
    game_node::build_app().run();
}