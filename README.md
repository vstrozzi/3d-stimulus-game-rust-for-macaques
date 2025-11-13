# Monkey 3D Game

A 3D puzzle game built with Bevy where the player must find the **RED face** of a randomly generated 3-sided pyramid using an orbital camera system.

**Objective**: Locate the red face of the pyramid by orbiting around it, then press SPACE when aligned correctly.

## Controls

-   **Arrow Keys / WASD**: Rotate camera around pyramid (orbit left/right, zoom in/out)
-   **SPACE**: Check alignment / Start game
-   **R**: Restart game (when game is over)
-   **ESC**: Toggle fullscreen/windowed mode and cursor lock
-   **V**: Toggle VSync on/off

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
    # Build and run with wasm-server-runner
    cargo run --target wasm32-unknown-unknown
    ```
    This will build the wasm binary and start a local server.

3.  **Manual Wasm Build and Serve:**
    If you want to build the wasm file and serve it manually:

    a. **Build the wasm binary:**
    You will need a tool like `wasm-pack` or `wasm-bindgen-cli` to process the output. For example with `wasm-pack`:
    ```bash
    wasm-pack build --target web --out-dir ./out
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

## 🏗️ Architecture (Bevy ECS)

### Plugin System

The game is organized into modular plugins:

```rust
App::new()
    .add_plugins(DefaultPlugins)           // Bevy's core plugins
    .add_plugins(SetupPlugin)              // Scene initialization
    .add_plugins(GameFunctionsPlugin)      // Game logic & UI
    .add_plugins(Camera3dFpovPlugin)       // Camera controls
    .add_plugins(InputsPlugin)             // Input handling
    .add_plugins(DebugFunctionsPlugin)     // Debug tools
```

### Key Resources

- **`GameState`**: Central game state resource containing:
  - Random seed & generator
  - Pyramid parameters (type, size, colors)
  - Game flags (is_playing, is_started, is_won, is_changed)
  - Timing data (start_time, end_time)
  - Metrics (attempts, cosine_alignment)

### Key Components

- **`Pyramid`**: Marker component for pyramid entity
- **`FaceMarker`**: Face metadata (index, color, normal vector)
- **`GameEntity`**: Marker for entities that get cleared on restart
- **`UIEntity`**: Marker for UI elements that get updated

### Key Systems

1. **Setup System** (`setup.rs`):
   - Runs once at startup
   - Spawns camera, ground plane, lights
   - Generates random pyramid with decorations
   - Initializes game state

2. **Camera System** (`camera.rs`):
   - Orbital camera that rotates around origin
   - Maintains fixed height (Y-axis)
   - Zoom constrained between min/max radius

3. **Game Logic System** (`game_functions.rs`):
   - `check_face_alignment`: Detects SPACE press, calculates alignment
   - `game_ui`: State machine for UI (start screen, in-game, win screen)

4. **Input System** (`inputs.rs`):
   - Handles ESC key for fullscreen/cursor toggle

## 🎨 Pyramid Generation

### Two Pyramid Types

- **Type1**: All 3 faces have different colors
- **Type2**: 2 faces share the same color (harder!)

### Face Decoration System

Each pyramid face gets random decorations:
- **Shapes**: Circle, Square, Star, Triangle
- **Count**: 10-100 per face
- **Size**: 0.05-0.15 units
- Placed randomly within triangle bounds with collision avoidance

### Color Scheme

```rust
PYRAMID_COLORS = [
    RED (target face),
    BLUE,
    GREEN
]
```

## 🔧 Key Bevy 0.17.2 APIs Used

### Entity Spawning (New in 0.14+)

```rust
// New way (0.14+)
commands.spawn((
    Mesh3d(mesh),
    MeshMaterial3d(material),
    Transform::default(),
));
```

### Component Access

```rust
// Queries
fn system(query: Query<&Transform, With<Camera3d>>) { }
fn system(mut query: Query<&mut Transform>) { }

// Resources
fn system(res: Res<GameState>) { }
fn system(mut res: ResMut<GameState>) { }
```

### UI (New Text API in 0.15+)

```rust
// Spawn text
commands.spawn((
    Text::new("Hello"),
    TextFont {
        font_size: 24.0,
        ..default()
    },
    TextColor(Color::WHITE),
    Node { /* positioning */ },
));
```

### Transform Operations

```rust
// Position
Transform::from_xyz(x, y, z)

// Look at
transform.looking_at(Vec3::ZERO, Vec3::Y)

// Rotation (orbital camera)
let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
transform.translation = Vec3::new(
    radius * yaw.sin(),
    height,
    radius * yaw.cos(),
);
```

### Input Handling

```rust
fn system(keyboard: Res<ButtonInput<KeyCode>>) {
    if keyboard.just_pressed(KeyCode::Space) { /* ... */ }
    if keyboard.pressed(KeyCode::KeyW) { /* ... */ }
}
```

## 🎲 Game State Machine

```
┌─────────────────┐
│  Not Started    │  Press SPACE
│  (Tutorial)     ├──────────────┐
└─────────────────┘              │
                                 ▼
                    ┌─────────────────────┐
                    │    Playing          │
                    │  (Orbiting camera)  │
                    └──────────┬──────────┘
                               │ SPACE (correct)
                               ▼
                    ┌─────────────────────┐
                    │   Game Over         │  Press R
                    │  (Show stats)       ├─────────┐
                    └─────────────────────┘         │
                               ▲                     │
                               └─────────────────────┘
```

## 📊 Alignment Detection

The game uses **dot product** to determine if the camera is aligned with a face:

```rust
// Get camera forward direction
let camera_forward = camera_transform.local_z();

// Get face normal (world space)
let face_normal = face_transform.rotation * face_marker.normal;

// Project to XZ plane (horizontal alignment)
let face_normal_xz = Vec3::new(face_normal.x, 0.0, face_normal.z).normalize();

// Calculate alignment (dot product)
let alignment = face_normal_xz.dot(*camera_forward);

// Check threshold (< -0.9 means facing camera)
if alignment < COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD {
    // Face is aligned!
}
```

## 🎨 Constants Reference

### Camera

```rust
CAMERA_3D_INITIAL_Y: 0.5
CAMERA_3D_SPEED_X: 2.0        // Rotation speed
CAMERA_3D_SPEED_Z: 4.0        // Zoom speed
CAMERA_3D_MIN_RADIUS: 5.0
CAMERA_3D_MAX_RADIUS: 50.0
```

### Pyramid

```rust
PYRAMID_BASE_RADIUS: 1.0 - 5.0
PYRAMID_HEIGHT: 2.0 - 7.0
PYRAMID_ANGLE_OFFSET: 0° - 360° (random start rotation)
PYRAMID_TARGET_FACE_INDEX: 0 (red face)
```

### Game

```rust
REFRESH_RATE_HZ: 60.0
COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD: -0.9
```

## 🚀 Building & Running

### Native Build

```bash
# Development
cargo run

# Release (optimized)
cargo run --release
```

### WASM Build

```bash
# Install target
rustup target add wasm32-unknown-unknown

# Build
cargo build --release --target wasm32-unknown-unknown

# Run with wasm-server-runner (configured in Cargo.toml)
cargo run --target wasm32-unknown-unknown

# Or use Python server
python3 -m http.server 8000
# Open http://localhost:8000
```

## 🔨 How to Modify

### Change Pyramid Colors

Edit `src/utils/constants.rs`:

```rust
pub const PYRAMID_COLORS: [Color; 3] = [
    Color::srgb(1.0, 0.0, 0.0),  // Face 0 (target)
    Color::srgb(0.0, 1.0, 0.0),  // Face 1
    Color::srgb(0.0, 0.0, 1.0),  // Face 2
];
```

### Add New Decoration Shape

1. Add variant to `DecorationShape` enum in `objects.rs`
2. Implement mesh creation in `setup.rs` (see `create_star_mesh`)
3. Add case in `create_decoration_mesh` function

### Change Camera Behavior

Edit `src/utils/camera.rs`:
- Modify `CAMERA_3D_SPEED_X/Z` for different speeds
- Change `CAMERA_3D_MIN/MAX_RADIUS` for zoom limits
- Add pitch control by modifying rotation calculation

### Add New Game Mode

1. Add field to `GameState` in `objects.rs`
2. Modify state machine in `game_ui` function in `game_functions.rs`
3. Add new key handler in `inputs.rs`

## 🐛 Common Issues

### Camera not moving
- Check `game_state.is_playing` and `game_state.is_started` flags
- Camera only responds when game is active

### Face alignment not detecting
- Verify `COSINE_ALIGNMENT_CAMERA_FACE_THRESHOLD` value
- Check face normals are calculated correctly in world space

### WASM not loading
- Ensure `wasm32-unknown-unknown` target is installed
- Check browser console for errors
- Verify `index.html` points to correct WASM file

## 📚 Key Bevy Concepts Used

- **ECS (Entity Component System)**: Core architecture pattern
- **Plugins**: Modular game logic organization
- **Queries**: Efficient entity filtering and access
- **Resources**: Global game state management
- **Systems**: Functions that run each frame
- **Commands**: Deferred entity/component operations
- **Fixed Timestep**: For consistent physics (60Hz)
- **Markers**: Zero-size components for entity tagging

## 🔗 Useful Links

- [Bevy 0.17 Documentation](https://docs.rs/bevy/0.17.2/bevy/)
- [Bevy Official Examples](https://bevyengine.org/examples/)
- [Bevy Cheat Book](https://bevy-cheatbook.github.io/)
- [Bevy Assets](https://bevyengine.org/assets/)

## 💡 Quick Tips

1. **Use markers**: Tag entities with zero-size components for easy queries
2. **State changes**: Set `is_changed` flag when game state updates
3. **Restart logic**: Despawn all `GameEntity` components, call `setup` again
4. **Camera math**: Use polar coordinates for smooth orbital motion
5. **Performance**: Most setup uses `ResMut` to reduce cloning

## 🎯 Next Steps for Development

- [ ] Add scoring system based on attempts and time
- [ ] Multiple difficulty levels (pyramid complexity)
- [ ] Sound effects and music
- [ ] Particle effects on correct alignment
- [ ] Multiplayer race mode
- [ ] Leaderboard (local/online)
- [ ] More pyramid shapes (4-sided, 5-sided)
- [ ] Tutorial mode with hints
- [ ] Save/load game progress
