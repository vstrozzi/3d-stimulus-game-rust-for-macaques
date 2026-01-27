# Monkey 3D Game - Twin Engine Architecture (Shared Memory)

A 3D puzzle game built with Bevy demonstrating the **Twin-Engine Architecture**, where the Game Logic (Game Node) is decoupled from the Controller and communicates via **Atomic Shared Memory**.

This architecture allows for extremely low-latency, lock-free communication between the game and external controllers, supporting multiple languages and platforms.

## Architecture

*   **Shared Library (`shared`)**: Defines the atomic data structures (`SharedCommands`, `SharedGameState`) and handles platform-specific shared memory creation (mmap on Native, SharedArrayBuffer on Web).
*   **Game Node (`game_node`)**: The Bevy application. It reads commands from shared memory and writes game state to shared memory every frame.
*   **Controllers**:
    *   **Native (`controller_native`)**: Rust TUI application. Spawns the game process and maps keyboard input to atomic flags.
    *   **Python (`controller_python`)**: Tkinter + transitions GUI built on the `monkey_shared` PyO3 bindings for interactive control.
    *   **Web (`controller_web`)**: HTML/JS interface. Loads the WASM game and interacts via shared memory buffers.

## Prerequisites

1.  **Rust**: Stable toolchain installed (`rustup`).
2.  **OS**: Linux/macOS (Windows support is experimental).

### For Web Build
*   `wasm-pack`: `cargo install wasm-pack`

### For Python Controller
*   Python 3.10+
*   `pip install transitions`
*   (Linux) `sudo apt install python3-tk` if Tkinter is missing

## How to Run

**Important**: You must run the `game_node` and the `controller` in separate terminals.

### 1. Start the Game Node
Terminal 1:
```bash
cargo run -p game_node
```

### 2. Start a Controller (Terminal 2)

#### Native Controller (Rust)
```bash
cargo run -p controller_native
```

#### Python Controller
```bash
# Build shared library with Python bindings
cargo build --release -p shared --features python

# Copy the module next to the controller (adjust extension for your OS if needed)
cp target/release/libshared.so controller_python/monkey_shared.so

# Run the GUI controller
python controller_python/controller.py
```

#### Web Controller
1. Build WASM (`wasm-pack build game_node --target web --out-dir pkg`)
2. Launch
