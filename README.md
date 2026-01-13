# Monkey 3D Game

A 3D puzzle game built with Bevy where the player must find the correct orientation of the given randomly generated 3-sided pyramid using an orbital camera system.

**Objective**: Locate the red face of the pyramid by orbiting around it, then press SPACE when aligned correctly.

## Controls

### Keyboard
-   **Arrow Keys / WASD**: Rotate camera around pyramid (orbit left/right, zoom in/out)
-   **SPACE**: Check alignment / Start game
-   **R**: Restart game (when game is over)
-   **ESC**: Toggle fullscreen/windowed mode and cursor lock
-   **V**: Toggle VSync on/off

### Touch (Mobile/Touchscreen)
-   **Horizontal Swipe**: Rotate camera around pyramid (left/right)
-   **Vertical Swipe**: Zoom in/out (swipe up to zoom in, swipe down to zoom out)
-   **Tap**: Check alignment / Start game / Restart game

## Dependencies

-   `bevy = "0.17.2"`
-   `rand = "0.9.2"`
-   `rand_chacha = "0.9.0"`
-   `web-sys = "0.3.82"` (for wasm logging)

## Building and Running

### Native

```bash
# Run in development mode
cargo run

# Run in release mode (optimized)
cargo run --release
```

### WebAssembly (WASM)

1.  **Install the wasm target:**
    ```bash
    rustup target add wasm32-unknown-unknown
    ```

2.  **Build for wasm:**
    This project is configured to use `wasm-server-runner` which simplifies development.

    ```bash
    # Build
    cargo build --release --target wasm32-unknown-unknown

    ```
    This will build the wasm binary.

3.  **Manual Wasm Build and Serve:**
    If you want to build the wasm file and serve it manually:

    a. **Build the wasm binary:**
    You will need a tool like `wasm-bindgen` to get the js bindings.
    ```bash
    wasm-bindgen --out-dir ./out/ --target web ./target/wasm32-unknown-unknown/release/monkey_3d_game.wasm
    ```

    b. **Serve the `index.html` file:**
    The `index.html` file expects the wasm javascript module to be in the `./out/` directory.

    A simple way to serve the files is to use a local http server. For example, with Python:
    ```bash
    # Make sure you are in the root of the project directory
    python3 -m http.server
    ```
    Then open your browser to `http://localhost:8000`.

    Or using `http-server` via npx:
    ```bash
    npx http-server .
    ```

## 🔗 Useful Links

- [Bevy 0.17 Documentation](https://docs.rs/bevy/0.17.2/bevy/)
- [Bevy Official Examples](https://bevyengine.org/examples/)
- [Bevy Cheat Book](https://bevy-cheatbook.github.io/)
- [Bevy Assets](https://bevyengine.org/assets/)