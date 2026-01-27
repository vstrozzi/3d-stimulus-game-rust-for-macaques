//! Native Controller - Windowed (minifb)
//!
//! This controller opens a small window to handle input reliably.
//! It mimics a standard game loop: poll window events -> update state -> render.

use minifb::{Key, Window, WindowOptions};
use serde::Deserialize;
use shared::{open_shared_memory, SharedMemoryHandle};
use std::{
    error::Error,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    thread,
    time::{Duration, Instant},
};

const WIDTH: usize = 600;
const HEIGHT: usize = 200;

/// Trial configuration from JSONL file
#[derive(Debug, Clone, Deserialize)]
struct TrialConfig {
    seed: u64,
    pyramid_type: u32,
    base_radius: f32,
    height: f32,
    start_orient: f32,
    target_door: u32,
    colors: [[f32; 4]; 3],
}

impl Default for TrialConfig {
    fn default() -> Self {
        Self {
            seed: 69,
            pyramid_type: 0,
            base_radius: 2.5,
            height: 4.0,
            start_orient: 0.0,
            target_door: 5,
            colors: [
                [1.0, 0.2, 0.2, 1.0], // Red
                [0.2, 0.5, 1.0, 1.0], // Blue
                [0.2, 1.0, 0.3, 1.0], // Green
            ],
        }
    }
}

/// Load trials from JSONL file
fn load_trials() -> Vec<TrialConfig> {
    // Try relative to executable first, then parent directory
    let paths = [
        Path::new("trials.jsonl").to_path_buf(),
        Path::new("../trials.jsonl").to_path_buf(),
    ];

    for path in &paths {
        if let Ok(file) = File::open(path) {
            let reader = BufReader::new(file);
            let mut trials = Vec::new();

            for line in reader.lines().filter_map(Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    if let Ok(trial) = serde_json::from_str::<TrialConfig>(line) {
                        trials.push(trial);
                    }
                }
            }

            if !trials.is_empty() {
                println!("Loaded {} trials from {:?}", trials.len(), path);
                return trials;
            }
        }
    }

    println!("Failed to load trials.jsonl, using default config");
    vec![TrialConfig::default()]
}

/// Write trial configuration to shared memory (game_structure)
fn write_trial_config(shm: &shared::SharedMemory, trial: &TrialConfig) {
    use std::sync::atomic::Ordering;

    let gs = &shm.game_structure;
    gs.seed.store(trial.seed, Ordering::Relaxed);
    gs.pyramid_type.store(trial.pyramid_type, Ordering::Relaxed);
    gs.base_radius.store(trial.base_radius.to_bits(), Ordering::Relaxed);
    gs.height.store(trial.height.to_bits(), Ordering::Relaxed);
    gs.start_orient.store(trial.start_orient.to_bits(), Ordering::Relaxed);
    gs.target_door.store(trial.target_door, Ordering::Relaxed);

    // Write colors (3 faces * 4 channels = 12 values)
    for (face_idx, face_colors) in trial.colors.iter().enumerate() {
        for (chan_idx, &channel) in face_colors.iter().enumerate() {
            let idx = face_idx * 4 + chan_idx;
            gs.colors[idx].store(channel.to_bits(), Ordering::Relaxed);
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("Starting Native Controller...");

    // Load trials
    let trials = load_trials();
    let mut current_trial_index = 0;
    let mut previous_has_won = false;
    let mut win_time: Option<Instant> = None;
    const WIN_DELAY: Duration = Duration::from_secs(2);

    // Connect to shared memory
    let mut shm_handle: Option<SharedMemoryHandle> = None;

    println!("Waiting for Game Node to initialize Shared Memory...");
    // Simple retry loop
    for _ in 0..10 {
        match open_shared_memory("monkey_game") {
            Ok(h) => {
                shm_handle = Some(h);
                break;
            }
            Err(_) => {
                thread::sleep(Duration::from_secs(1));
                println!("Retrying...");
            }
        }
    }

    let shm_handle = shm_handle.ok_or("Could not connect to Shared Memory. Is game_node running?")?;
    let shm = shm_handle.get();

    println!("Connected! Starting Window...");
    thread::sleep(Duration::from_secs(1));

    // Create Window
    let mut window = Window::new(
        "Monkey Game Controller - Press ESC to exit",
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    )?;

    println!("=== Native Controller Window Open ===");
    println!("Focus the WINDOW to control the game.");
    println!("Controls: Arrows (Rotate/Zoom), Space (Check), R (Reset), B (Blank), P (Pause), O (Resume)");

    // Toggle debounce for one-shot triggers
    let mut space_was_pressed = false;
    let mut r_was_pressed = false;
    let mut b_was_pressed = false;
    let mut p_was_pressed = false;
    let mut o_was_pressed = false;

    // Framebuffer (black)
    let buffer: Vec<u32> = vec![0; WIDTH * HEIGHT];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // 1. Poll Input from Window
        let rotate_left = window.is_key_down(Key::Left);
        let rotate_right = window.is_key_down(Key::Right);
        let zoom_in = window.is_key_down(Key::Up);
        let zoom_out = window.is_key_down(Key::Down);
        let space = window.is_key_down(Key::Space);
        let r = window.is_key_down(Key::R);
        let b = window.is_key_down(Key::B);
        let p = window.is_key_down(Key::P);
        let o = window.is_key_down(Key::O);

        // 2. Read game state and check for win
        let has_won = shm.game_structure.has_won.load(std::sync::atomic::Ordering::Relaxed);

        // Detect rising edge of win
        if has_won && !previous_has_won {
            println!("Trial {} won!", current_trial_index + 1);
            win_time = Some(Instant::now());
        }
        previous_has_won = has_won;

        // Auto-advance after delay
        if let Some(wt) = win_time {
            if wt.elapsed() >= WIN_DELAY {
                current_trial_index = (current_trial_index + 1) % trials.len();
                println!("Advancing to trial {}/{}", current_trial_index + 1, trials.len());
                write_trial_config(shm, &trials[current_trial_index]);
                shm.commands.reset.store(true, std::sync::atomic::Ordering::Release);
                win_time = None;
            }
        }

        // 3. Write Commands

        // Continuous controls (rotate, zoom) - direct mapping
        shm.commands.rotate_left.store(rotate_left, std::sync::atomic::Ordering::Relaxed);
        shm.commands.rotate_right.store(rotate_right, std::sync::atomic::Ordering::Relaxed);
        shm.commands.zoom_in.store(zoom_in, std::sync::atomic::Ordering::Relaxed);
        shm.commands.zoom_out.store(zoom_out, std::sync::atomic::Ordering::Relaxed);

        // One-shot triggers (with debounce)
        if space {
            if !space_was_pressed {
                shm.commands.check_alignment.store(true, std::sync::atomic::Ordering::Relaxed);
                space_was_pressed = true;
                println!("Check Alignment triggered");
            }
        } else {
            space_was_pressed = false;
        }

        if r {
            if !r_was_pressed {
                // Write current trial config before reset
                write_trial_config(shm, &trials[current_trial_index]);
                shm.commands.reset.store(true, std::sync::atomic::Ordering::Release);
                r_was_pressed = true;
                println!("Reset triggered (trial {} config written)", current_trial_index + 1);
            }
        } else {
            r_was_pressed = false;
        }

        if b {
            if !b_was_pressed {
                shm.commands.blank_screen.store(true, std::sync::atomic::Ordering::Relaxed);
                b_was_pressed = true;
                println!("Blank screen toggled");
            }
        } else {
            b_was_pressed = false;
        }

        if p {
            if !p_was_pressed {
                shm.commands.stop_rendering.store(true, std::sync::atomic::Ordering::Relaxed);
                p_was_pressed = true;
                println!("Rendering paused");
            }
        } else {
            p_was_pressed = false;
        }

        if o {
            if !o_was_pressed {
                shm.commands.resume_rendering.store(true, std::sync::atomic::Ordering::Relaxed);
                o_was_pressed = true;
                println!("Rendering resumed");
            }
        } else {
            o_was_pressed = false;
        }

        // 4. Read Game State (Telemetry)
        let frame = shm.game_structure.frame_number.load(std::sync::atomic::Ordering::Relaxed);

        // Update Title with Telemetry
        let title = format!(
            "Trial {}/{} | Frame: {} | Controls: ←→ Rotate, ↑↓ Zoom, Space Check, R Reset, B Blank, P Pause, O Resume",
            current_trial_index + 1, trials.len(), frame
        );
        window.set_title(&title);

        // 5. Update Window
        // Push black buffer to keep window alive
        window.update_with_buffer(&buffer, WIDTH, HEIGHT)?;
    }

    println!("Controller window closed.");
    Ok(())
}
