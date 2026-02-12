//! Python bindings for shared memroy of native.rs
use crate::{SharedMemoryHandle, create_shared_memory};
use std::sync::atomic::Ordering;
use pyo3::exceptions::PyValueError;
use pyo3::{prelude::*};

// Python class wrapper of SharedMemoryHandle implementation
#[pyclass]
struct SharedMemoryWrapper {
    inner: SharedMemoryHandle,
}

// Python wrapper around methods for SharedMemoryHandle
#[pymethods]
impl SharedMemoryWrapper {
    #[new]
    #[pyo3(signature = (name))]
    /// Create (with file name) or open shared memory segment
    fn new(name: &str) -> PyResult<Self> {
        let res = create_shared_memory(name);

        match res {
            Ok(handle) => Ok(SharedMemoryWrapper { inner: handle }),
            Err(e) => Err(PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string())),
        }
    }

    /// Read the full game structure from shared memory as a dictionary.
    /// It reads one written by the game.
    /// Some values need to be read as f32 from bits
    fn read_game_structure(&self) -> PyResult<Py<PyAny>> {
        let shm = self.inner.get();
        let gs= &shm.game_structure_game;

        Python::attach(|py| {
            let dict = pyo3::types::PyDict::new(py);

            // Fixed vars in trial
            dict.set_item("decoration_seeds", [
                gs.decoration_seeds[0].load(Ordering::Relaxed),
                gs.decoration_seeds[1].load(Ordering::Relaxed),
                gs.decoration_seeds[2].load(Ordering::Relaxed),
            ])?;
            dict.set_item("base_radius", f32::from_bits(gs.base_radius.load(Ordering::Relaxed)))?;
            dict.set_item("height", f32::from_bits(gs.height.load(Ordering::Relaxed)))?;
            dict.set_item("start_orient", f32::from_bits(gs.start_orient.load(Ordering::Relaxed)))?;
            dict.set_item("target_door", gs.target_door.load(Ordering::Relaxed))?;
            let mut colors: Vec<Vec<f32>> = Vec::with_capacity(3);  // Colors as 3x4 list
            for face_idx in 0..3 {
                let mut face_colors: Vec<f32> = Vec::with_capacity(4);
                for channel_idx in 0..4 {
                    let index = face_idx * 4 + channel_idx;
                    face_colors.push(f32::from_bits(gs.colors[index].load(Ordering::Relaxed)));
                }
                colors.push(face_colors);
            }
            dict.set_item("colors", colors)?;

            dict.set_item("main_spotlight_intensity", f32::from_bits(gs.main_spotlight_intensity.load(Ordering::Relaxed)))?;
            dict.set_item("ambient_brightness", f32::from_bits(gs.ambient_brightness.load(Ordering::Relaxed)))?;
            dict.set_item("max_spotlight_intensity", f32::from_bits(gs.max_spotlight_intensity.load(Ordering::Relaxed)))?;
            dict.set_item("decoration_count", [
                gs.decorations_count[0].load(Ordering::Relaxed),
                gs.decorations_count[1].load(Ordering::Relaxed),
                gs.decorations_count[2].load(Ordering::Relaxed)
            ])?;
            dict.set_item("decoration_size", [
                f32::from_bits(gs.decorations_size[0].load(Ordering::Relaxed)),
                f32::from_bits(gs.decorations_size[1].load(Ordering::Relaxed)),
                f32::from_bits(gs.decorations_size[2].load(Ordering::Relaxed))
            ])?;

            // Dynamic vars in trial
            dict.set_item("cosine_alignment_threshold", f32::from_bits(gs.cosine_alignment_threshold.load(Ordering::Relaxed)))?;
            dict.set_item("door_anim_fade_out", f32::from_bits(gs.door_anim_fade_out.load(Ordering::Relaxed)))?;
            dict.set_item("door_anim_stay_open", f32::from_bits(gs.door_anim_stay_open.load(Ordering::Relaxed)))?;
            dict.set_item("door_anim_fade_in", f32::from_bits(gs.door_anim_fade_in.load(Ordering::Relaxed)))?;
            dict.set_item("frame_number", gs.frame_number.load(Ordering::Relaxed))?;
            dict.set_item("elapsed_secs", f32::from_bits(gs.elapsed_secs.load(Ordering::Relaxed)))?;
            dict.set_item("camera_radius", f32::from_bits(gs.camera_radius.load(Ordering::Relaxed)))?;
            dict.set_item("camera_position", vec![
                f32::from_bits(gs.camera_x.load(Ordering::Relaxed)),
                f32::from_bits(gs.camera_y.load(Ordering::Relaxed)),
                f32::from_bits(gs.camera_z.load(Ordering::Relaxed)),
            ])?;
            dict.set_item("nr_attempts", gs.attempts.load(Ordering::Relaxed))?;
            dict.set_item("cosine_alignment", f32::from_bits(gs.current_alignment.load(Ordering::Relaxed)))?;
            dict.set_item("current_angle", f32::from_bits(gs.current_angle.load(Ordering::Relaxed)))?;
            dict.set_item("is_animating", gs.is_animating.load(Ordering::Relaxed))?;
            dict.set_item("win_elapsed_secs", f32::from_bits(gs.win_time.load(Ordering::Relaxed)))?;

            Ok(dict.into())
        })
    }

    /// Write commands to shared memory.
    fn write_commands(
        &mut self,
        rotate_left: bool,
        rotate_right: bool,
        zoom_in: bool,
        zoom_out: bool,
        check: bool,
        reset: bool,
        blank_screen: bool,
        stop_rendering: bool,
        resume_rendering: bool,
        animation_door: bool,
    ) {
        let shm = self.inner.get();
        let cmd = &shm.commands;

        cmd.rotate_left.store(rotate_left, Ordering::Relaxed);
        cmd.rotate_right.store(rotate_right, Ordering::Relaxed);
        cmd.zoom_in.store(zoom_in, Ordering::Relaxed);
        cmd.zoom_out.store(zoom_out, Ordering::Relaxed);    
        cmd.check_alignment.store(check, Ordering::Relaxed);
        cmd.reset.store(reset, Ordering::Release);
        cmd.blank_screen.store(blank_screen, Ordering::Relaxed);
        cmd.stop_rendering.store(stop_rendering, Ordering::Relaxed);
        cmd.resume_rendering.store(resume_rendering, Ordering::Relaxed);
        cmd.animation_door.store(animation_door, Ordering::Relaxed);
        
    }

    /// Write game structure config fields to shared memory.
    /// Write in controller region
    fn write_game_structure(
        &mut self,
        decoration_seeds: [u64; 3],
        base_radius: f32,
        height: f32,
        start_orient: f32,
        target_door: u32,
        colors: Vec<Vec<f32>>,
        decorations_count: [u32; 3],
        decorations_size: [f32; 3],
        cosine_alignment_threshold: f32,
        door_anim_fade_out: f32,
        door_anim_stay_open: f32,
        door_anim_fade_in: f32,
        main_spotlight_intensity: f32,
        ambient_brightness: f32,
        max_spotlight_intensity: f32,
    ) -> PyResult<()> {
        if colors.len() != 3 || colors.iter().any(|face| face.len() != 4) {
            return Err(PyErr::new::<PyValueError, _>(format!(
                "expected colors to be a 3x4 matrix, got {:?}",
                colors.iter().map(|face| face.len()).collect::<Vec<_>>()
            )));
        }

        let shm = self.inner.get();
        let gs = &shm.game_structure_control;

        for i in 0..3 {
            gs.decoration_seeds[i].store(decoration_seeds[i], Ordering::Relaxed);
        }
        gs.base_radius.store(base_radius.to_bits(), Ordering::Relaxed);
        gs.height.store(height.to_bits(), Ordering::Relaxed);
        gs.start_orient.store(start_orient.to_bits(), Ordering::Relaxed);
        gs.target_door.store(target_door, Ordering::Relaxed);

        for (face_idx, face) in colors.iter().enumerate() {
            for (channel_idx, value) in face.iter().enumerate() {
                let index = face_idx * 4 + channel_idx;
                gs.colors[index].store(value.to_bits(), Ordering::Relaxed);
            }
        }
        
        // Store decorations
        for i in 0..3 {
            gs.decorations_count[i].store(decorations_count[i], Ordering::Relaxed);
            gs.decorations_size[i].store(decorations_size[i].to_bits(), Ordering::Relaxed);
        }
        gs.cosine_alignment_threshold.store(cosine_alignment_threshold.to_bits(), Ordering::Relaxed);
        gs.door_anim_fade_out.store(door_anim_fade_out.to_bits(), Ordering::Relaxed);
        gs.door_anim_stay_open.store(door_anim_stay_open.to_bits(), Ordering::Relaxed);
        gs.door_anim_fade_in.store(door_anim_fade_in.to_bits(), Ordering::Relaxed);
        gs.main_spotlight_intensity.store(main_spotlight_intensity.to_bits(), Ordering::Relaxed);
        gs.ambient_brightness.store(ambient_brightness.to_bits(), Ordering::Relaxed);
        gs.max_spotlight_intensity.store(max_spotlight_intensity.to_bits(), Ordering::Relaxed);
        Ok(())
    }


}

#[pymodule]
#[pyo3(name = "monkey_shared")]
fn monkey_shared(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SharedMemoryWrapper>()?;

    // Export constants from constants.rs so Python can import them directly.
    use crate::constants::game_constants;
    m.add("REFRESH_RATE_HZ", game_constants::REFRESH_RATE_HZ)?;
    m.add("DECORATION_SEEDS", game_constants::DECORATION_SEEDS.to_vec())?;
    m.add("COSINE_ALIGNMENT_TO_WIN", game_constants::COSINE_ALIGNMENT_TO_WIN)?;

    // pyramid_constants
    use crate::constants::pyramid_constants;
    m.add("PYRAMID_BASE_RADIUS", pyramid_constants::PYRAMID_BASE_RADIUS)?;
    m.add("PYRAMID_HEIGHT", pyramid_constants::PYRAMID_HEIGHT)?;
    m.add("PYRAMID_START_ANGLE_OFFSET_RAD", pyramid_constants::PYRAMID_START_ANGLE_OFFSET_RAD)?;
    m.add("PYRAMID_TARGET_DOOR_INDEX", pyramid_constants::PYRAMID_TARGET_DOOR_INDEX)?;
    m.add("PYRAMID_COLORS", pyramid_constants::PYRAMID_COLORS.iter().map(|f| f.to_vec()).collect::<Vec<Vec<f32>>>())?;
    m.add("PYRAMID_DECORATIONS_COUNT", pyramid_constants::PYRAMID_DECORATIONS_COUNT.to_vec())?;
    m.add("PYRAMID_DECORATIONS_SIZE", pyramid_constants::PYRAMID_DECORATIONS_SIZE.to_vec())?;
    m.add("DOOR_ANIM_FADE_OUT", pyramid_constants::DOOR_ANIM_FADE_OUT)?;
    m.add("DOOR_ANIM_STAY_OPEN", pyramid_constants::DOOR_ANIM_STAY_OPEN)?;
    m.add("DOOR_ANIM_FADE_IN", pyramid_constants::DOOR_ANIM_FADE_IN)?;

    // lighting_constants
    use crate::constants::lighting_constants;
    m.add("SPOTLIGHT_LIGHT_INTENSITY", lighting_constants::SPOTLIGHT_LIGHT_INTENSITY)?;
    m.add("GLOBAL_AMBIENT_LIGHT_INTENSITY", lighting_constants::GLOBAL_AMBIENT_LIGHT_INTENSITY)?;
    m.add("MAX_SPOTLIGHT_INTENSITY", lighting_constants::MAX_SPOTLIGHT_INTENSITY)?;

    // timing
    use crate::constants::timing;
    m.add("WIN_BLANK_DURATION_FRAMES", timing::WIN_BLANK_DURATION_FRAMES)?;

    // camera_3d_constants
    use crate::constants::camera_3d_constants;
    m.add("CAMERA_3D_INITIAL_RADIUS", camera_3d_constants::CAMERA_3D_INITIAL_RADIUS)?;

    Ok(())
}
