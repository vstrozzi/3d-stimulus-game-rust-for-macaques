// This file is the root of the `monkey_3d_game` library.
// It declares the `utils` module and its submodules, making them available to other parts of the crate.

/// The `utils` module contains various utility functions, constants, and objects used throughout the game.
pub mod utils {
    // The `camera` module contains the implementation of the 3D camera.
    pub mod camera;
    // The `constants` module contains all the constants used in the game.
    pub mod constants;
    // The `debug_functions` module contains functions for debugging purposes.
    pub mod debug_functions;
    // The `game_functions` module contains the core game logic.
    pub mod game_functions;
    // The `inputs` module handles player input.
    pub mod inputs;
    // The `macros` module defines macros used in the game.
    pub mod macros;
    // The `objects` module defines the various objects, resources, and components used in the game.
    pub mod objects;
    // The `pyramid` module contains the logic for spawning the pyramid and its decorations.
    pub mod pyramid;
    // The `setup` module contains the setup logic for the game.
    pub mod setup;
}
