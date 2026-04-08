//! Python bindings for shared memroy of native.rs
use crate::{SharedMemoryHandle, open_shared_memory, SharedGameState, RING_BUFFER_SIZE};
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
    /// Attach to an existing shared memory segment created by the game node.
    fn new(name: &str) -> PyResult<Self> {
        open_shared_memory(name)
            .map(|handle| SharedMemoryWrapper { inner: handle })
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))
    }

    /// Return the current write head of the frame buffer.
    fn frame_write_head(&self) -> u64 {
        let shm = self.inner.get();
        shm.frame_ring_buffer.write_head.load(Ordering::Acquire)
    }

    /// Read the latest game state snapshot (from game_structure_game).
    fn read_game_state(&self) -> PyResult<Py<PyAny>> {
        let shm = self.inner.get();
        read_game_state(&shm.game_structure_game)
    }

    /// Read all game states written since `last_head`.
    /// Returns `(new_head, list_of_state_dicts)`.
    /// If the caller has fallen behind by more than the buffer capacity,
    /// only the most recent entries are returned.
    fn read_game_state_since(&self, last_head: u64) -> PyResult<Py<PyAny>> {
        let shm = self.inner.get();
        let ring = &shm.frame_ring_buffer;
        let current_head = ring.write_head.load(Ordering::Acquire);

        Python::attach(|py| {
            let result_list = pyo3::types::PyList::empty(py);

            if current_head <= last_head {
                let tup = pyo3::types::PyTuple::new(py, &[current_head.into_pyobject(py)?.into_any(), result_list.into_any()])?;
                return Ok(tup.into());
            }

            let start = if current_head - last_head > RING_BUFFER_SIZE as u64 {
                current_head - RING_BUFFER_SIZE as u64
            } else {
                last_head
            };

            for i in start..current_head {
                let idx = (i as usize) % RING_BUFFER_SIZE;
                let entry = &ring.entries[idx];
                let dict = read_game_state(entry)?;
                result_list.append(dict)?;
            }

            let tup = pyo3::types::PyTuple::new(py, &[current_head.into_pyobject(py)?.into_any(), result_list.into_any()])?;
            Ok(tup.into())
        })
    }

    fn read_default_game_state(&self) -> Result<pyo3::Py<pyo3::PyAny>, pyo3::PyErr>{
        read_game_state(&SharedGameState::new())
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
        animation_door: bool,
        animation_all_door: bool,
        animation_colored: bool,
    ) {
        let shm = self.inner.get();
        let cmd = &shm.commands;

        cmd.rotate_left.store(rotate_left, Ordering::Relaxed);
        cmd.rotate_right.store(rotate_right, Ordering::Relaxed);
        cmd.zoom_in.store(zoom_in, Ordering::Relaxed);
        cmd.zoom_out.store(zoom_out, Ordering::Relaxed);    
        cmd.check_alignment.store(check, Ordering::Relaxed);
        // Release ensures all preceding Relaxed stores are visible to the game before it sees reset=true.
        cmd.reset.store(reset, Ordering::Release);
        cmd.blank_screen.store(blank_screen, Ordering::Relaxed);
        cmd.stop_rendering.store(stop_rendering, Ordering::Relaxed);
        cmd.animation_door.store(animation_door, Ordering::Relaxed);
        cmd.animation_all_door.store(animation_all_door, Ordering::Relaxed);
        cmd.animation_colored.store(animation_colored, Ordering::Relaxed);
        
    }

    /// Write game structure config fields to controller shared memory.
    fn write_game_state(
        &mut self,
        base_radius: f32,
        height: f32,
        start_orient: f32,
        target_door: u32,
        colors: Vec<Vec<f32>>,
        textures: [u32; 3],
        decorations_count: [u32; 3],
        decorations_size: [f32; 3],
        decorations_color: Vec<Vec<f32>>,
        decorations_seeds: [u64; 3],
        decorations_shape: [u32; 3],
        decorations_texture: [u32; 3],
        decorations_thickness: [f32; 3],
        cosine_alignment_threshold: f32,
        door_anim_fade_out: f32,
        door_anim_stay_open: f32,
        door_anim_fade_in: f32,
        main_spotlight_intensity: f32,
        ambient_brightness: f32,
        max_spotlight_intensity: f32,
        progress_bar_size: u32,
        progress_bar_cur_size: u32,
        frame_number: u64,
        elapsed_secs: f32,
        camera_radius: f32,
        camera_position: [f32; 3],
        nr_attempts: u32,
        cosine_alignment: f32,
        current_angle: f32,
        is_animating: bool,
        is_blank: bool,
        is_rendering_stopped: bool,
        is_scene_ready: bool,
        win_elapsed_secs: f32,
    ) -> PyResult<()> {
        if colors.len() != 3 || colors.iter().any(|face| face.len() != 4) {
            return Err(PyErr::new::<PyValueError, _>(format!(
                "expected colors to be a 3x4 matrix, got {:?}",
                colors.iter().map(|face| face.len()).collect::<Vec<_>>()
            )));
        }
        if decorations_color.len() != 3 || decorations_color.iter().any(|face| face.len() != 4) {
            return Err(PyErr::new::<PyValueError, _>(format!(
                "expected decorations_color to be a 3x4 matrix, got {:?}",
                decorations_color.iter().map(|face| face.len()).collect::<Vec<_>>()
            )));
        }

        let shm = self.inner.get();
        let gs = &shm.game_structure_control;

        // Fixed vars in trial
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

        // Textures and decorations
        for i in 0..3 {
            gs.textures[i].store(textures[i], Ordering::Relaxed);
            gs.decorations_count[i].store(decorations_count[i], Ordering::Relaxed);
            gs.decorations_size[i].store(decorations_size[i].to_bits(), Ordering::Relaxed);
            gs.decorations_seeds[i].store(decorations_seeds[i], Ordering::Relaxed);
            gs.decorations_shape[i].store(decorations_shape[i], Ordering::Relaxed);
            gs.decorations_texture[i].store(decorations_texture[i], Ordering::Relaxed);
            gs.decorations_thickness[i].store(decorations_thickness[i].to_bits(), Ordering::Relaxed);
        }
        for (face_idx, face) in decorations_color.iter().enumerate() {
            for (channel_idx, value) in face.iter().enumerate() {
                gs.decorations_color[face_idx * 4 + channel_idx].store(value.to_bits(), Ordering::Relaxed);
            }
        }

        gs.cosine_alignment_threshold.store(cosine_alignment_threshold.to_bits(), Ordering::Relaxed);

        // Dynamic vars in trial
        gs.door_anim_fade_out.store(door_anim_fade_out.to_bits(), Ordering::Relaxed);
        gs.door_anim_stay_open.store(door_anim_stay_open.to_bits(), Ordering::Relaxed);
        gs.door_anim_fade_in.store(door_anim_fade_in.to_bits(), Ordering::Relaxed);

        gs.main_spotlight_intensity.store(main_spotlight_intensity.to_bits(), Ordering::Relaxed);
        gs.ambient_brightness.store(ambient_brightness.to_bits(), Ordering::Relaxed);
        gs.max_spotlight_intensity.store(max_spotlight_intensity.to_bits(), Ordering::Relaxed);

        gs.progress_bar_size.store(progress_bar_size, Ordering::Relaxed);
        gs.progress_bar_cur_size.store(progress_bar_cur_size, Ordering::Relaxed);

        gs.frame_number.store(frame_number, Ordering::Relaxed);
        gs.elapsed_secs.store(elapsed_secs.to_bits(), Ordering::Relaxed);
        gs.camera_radius.store(camera_radius.to_bits(), Ordering::Relaxed);
        gs.camera_x.store(camera_position[0].to_bits(), Ordering::Relaxed);
        gs.camera_y.store(camera_position[1].to_bits(), Ordering::Relaxed);
        gs.camera_z.store(camera_position[2].to_bits(), Ordering::Relaxed);
        gs.attempts.store(nr_attempts, Ordering::Relaxed);
        gs.current_alignment.store(cosine_alignment.to_bits(), Ordering::Relaxed);
        gs.current_angle.store(current_angle.to_bits(), Ordering::Relaxed);
        gs.is_animating.store(is_animating, Ordering::Relaxed);
        gs.is_blank.store(is_blank, Ordering::Relaxed);
        gs.is_rendering_stopped.store(is_rendering_stopped, Ordering::Relaxed);
        gs.is_scene_ready.store(is_scene_ready, Ordering::Relaxed);
        gs.win_time.store(win_elapsed_secs.to_bits(), Ordering::Relaxed);

        Ok(())
    }
}

// Read a game state as a python dict
    fn read_game_state(gs: &SharedGameState) -> PyResult<Py<PyAny>>{
        Python::attach(|py| {
            let dict = pyo3::types::PyDict::new(py);

            // Fixed trials fields
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

            dict.set_item("textures", [
                gs.textures[0].load(Ordering::Relaxed),
                gs.textures[1].load(Ordering::Relaxed),
                gs.textures[2].load(Ordering::Relaxed),
            ])?;
            dict.set_item("decorations_count", [
                gs.decorations_count[0].load(Ordering::Relaxed),
                gs.decorations_count[1].load(Ordering::Relaxed),
                gs.decorations_count[2].load(Ordering::Relaxed)
            ])?;
            dict.set_item("decorations_size", [
                f32::from_bits(gs.decorations_size[0].load(Ordering::Relaxed)),
                f32::from_bits(gs.decorations_size[1].load(Ordering::Relaxed)),
                f32::from_bits(gs.decorations_size[2].load(Ordering::Relaxed)),
            ])?;
            let decorations_color: Vec<Vec<f32>> = (0..3).map(|face| {
                (0..4).map(|ch| f32::from_bits(gs.decorations_color[face * 4 + ch].load(Ordering::Relaxed))).collect()
            }).collect();
            dict.set_item("decorations_color", decorations_color)?;
            dict.set_item("decorations_seeds", [
                gs.decorations_seeds[0].load(Ordering::Relaxed),
                gs.decorations_seeds[1].load(Ordering::Relaxed),
                gs.decorations_seeds[2].load(Ordering::Relaxed),
            ])?;
            dict.set_item("decorations_shape", [
                gs.decorations_shape[0].load(Ordering::Relaxed),
                gs.decorations_shape[1].load(Ordering::Relaxed),
                gs.decorations_shape[2].load(Ordering::Relaxed)
            ])?;
            dict.set_item("decorations_texture", [
                gs.decorations_texture[0].load(Ordering::Relaxed),
                gs.decorations_texture[1].load(Ordering::Relaxed),
                gs.decorations_texture[2].load(Ordering::Relaxed),
            ])?;
            dict.set_item("decorations_thickness", [
                f32::from_bits(gs.decorations_thickness[0].load(Ordering::Relaxed)),
                f32::from_bits(gs.decorations_thickness[1].load(Ordering::Relaxed)),
                f32::from_bits(gs.decorations_thickness[2].load(Ordering::Relaxed)),
            ])?;

            dict.set_item("cosine_alignment_threshold", f32::from_bits(gs.cosine_alignment_threshold.load(Ordering::Relaxed)))?;

            // Animation Durations
            dict.set_item("door_anim_fade_out", f32::from_bits(gs.door_anim_fade_out.load(Ordering::Relaxed)))?;
            dict.set_item("door_anim_stay_open", f32::from_bits(gs.door_anim_stay_open.load(Ordering::Relaxed)))?;
            dict.set_item("door_anim_fade_in", f32::from_bits(gs.door_anim_fade_in.load(Ordering::Relaxed)))?;

            // Lighting
            dict.set_item("main_spotlight_intensity", f32::from_bits(gs.main_spotlight_intensity.load(Ordering::Relaxed)))?;
            dict.set_item("ambient_brightness", f32::from_bits(gs.ambient_brightness.load(Ordering::Relaxed)))?;
            dict.set_item("max_spotlight_intensity", f32::from_bits(gs.max_spotlight_intensity.load(Ordering::Relaxed)))?;

            // Level bar
            dict.set_item("progress_bar_size", gs.progress_bar_size.load(Ordering::Relaxed))?;
            dict.set_item("progress_bar_cur_size", gs.progress_bar_cur_size.load(Ordering::Relaxed))?;

            // Dynamic trials fields
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
            dict.set_item("is_blank", gs.is_blank.load(Ordering::Relaxed))?;
            dict.set_item("is_rendering_stopped", gs.is_rendering_stopped.load(Ordering::Relaxed))?;
            dict.set_item("is_scene_ready", gs.is_scene_ready.load(Ordering::Relaxed))?;
            dict.set_item("win_elapsed_secs", f32::from_bits(gs.win_time.load(Ordering::Relaxed)))?;

            Ok(dict.into())
        })
    }

    #[pymodule]
    #[pyo3(name = "monkey_shared")]
    fn monkey_shared(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_class::<SharedMemoryWrapper>()?;

        // Export constants from constants.rs so Python can import them directly.
        use crate::constants::game_constants;
        m.add("REFRESH_RATE_HZ", game_constants::REFRESH_RATE_HZ)?;
        m.add("DECORATIONS_SEEDS", game_constants::DECORATIONS_SEEDS.to_vec())?;
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
        m.add("PYRAMID_DECORATIONS_SHAPE", pyramid_constants::PYRAMID_DECORATIONS_SHAPE.iter().map(|s| *s as u32).collect::<Vec<u32>>())?;
        m.add("DOOR_ANIM_FADE_OUT", pyramid_constants::DOOR_ANIM_FADE_OUT)?;
        m.add("DOOR_ANIM_STAY_OPEN", pyramid_constants::DOOR_ANIM_STAY_OPEN)?;
        m.add("DOOR_ANIM_FADE_IN", pyramid_constants::DOOR_ANIM_FADE_IN)?;

        // lighting_constants
        use crate::constants::lighting_constants;
        m.add("SPOTLIGHT_LIGHT_INTENSITY", lighting_constants::SPOTLIGHT_LIGHT_INTENSITY)?;
        m.add("GLOBAL_AMBIENT_LIGHT_INTENSITY", lighting_constants::GLOBAL_AMBIENT_LIGHT_INTENSITY)?;
        m.add("MAX_SPOTLIGHT_INTENSITY", lighting_constants::MAX_SPOTLIGHT_INTENSITY)?;

        m.add("PROGRESS_BAR_WRAP_AROUND_SIZE", game_constants::PROGRESS_BAR_WRAP_AROUND_SIZE)?;
        // timing
        use crate::constants::timing;
        m.add("WIN_BLANK_DURATION_FRAMES", timing::WIN_BLANK_DURATION_FRAMES)?;

        // camera_3d_constants
        use crate::constants::camera_3d_constants;
        m.add("CAMERA_3D_INITIAL_RADIUS", camera_3d_constants::CAMERA_3D_INITIAL_RADIUS)?;

        Ok(())
    }

