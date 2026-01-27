use crate::{SharedMemoryHandle, create_shared_memory, open_shared_memory};
use pyo3::{prelude::*, exceptions::PyValueError};
use core::sync::atomic::Ordering;

// Python class wrapper of SharedMemoryHandle implementation
#[pyclass]
struct SharedMemoryWrapper {
    inner: SharedMemoryHandle,
}

// Python wrapper around methods for SharedMemoryHandle
#[pymethods]
impl SharedMemoryWrapper {
    #[new]
    #[pyo3(signature = (name, create=None))]
    fn new(name: &str, create: Option<bool>) -> PyResult<Self> {
        let create = create.unwrap_or(false);
        let res = if create {
            create_shared_memory(name)
        } else {
            open_shared_memory(name)
        };

        match res {
            Ok(handle) => Ok(SharedMemoryWrapper { inner: handle }),
            Err(e) => Err(PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string())),
        }
    }

    /// Read the full game structure from shared memory as a dictionary.
    /// This includes both config fields and state fields.
    fn read_game_structure(&self) -> PyResult<PyObject> {
        let shm = self.inner.get();
        let gs = &shm.game_structure;

        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);

            // Config fields
            dict.set_item("seed", gs.seed.load(Ordering::Relaxed))?;
            dict.set_item("pyramid_type", gs.pyramid_type.load(Ordering::Relaxed))?;
            dict.set_item("base_radius", f32::from_bits(gs.base_radius.load(Ordering::Relaxed)))?;
            dict.set_item("height", f32::from_bits(gs.height.load(Ordering::Relaxed)))?;
            dict.set_item("start_orient", f32::from_bits(gs.start_orient.load(Ordering::Relaxed)))?;
            dict.set_item("target_door", gs.target_door.load(Ordering::Relaxed))?;

            // Colors as 3x4 list
            let mut colors: Vec<Vec<f32>> = Vec::with_capacity(3);
            for face_idx in 0..3 {
                let mut face_colors: Vec<f32> = Vec::with_capacity(4);
                for channel_idx in 0..4 {
                    let index = face_idx * 4 + channel_idx;
                    face_colors.push(f32::from_bits(gs.colors[index].load(Ordering::Relaxed)));
                }
                colors.push(face_colors);
            }
            dict.set_item("colors", colors)?;

            // State fields
            dict.set_item("phase", gs.phase.load(Ordering::Relaxed))?;
            dict.set_item("frame_number", gs.frame_number.load(Ordering::Relaxed))?;
            dict.set_item("elapsed_secs", f32::from_bits(gs.elapsed_secs.load(Ordering::Relaxed)))?;
            dict.set_item("camera_radius", f32::from_bits(gs.camera_radius.load(Ordering::Relaxed)))?;
            dict.set_item("camera_position", vec![
                f32::from_bits(gs.camera_x.load(Ordering::Relaxed)),
                f32::from_bits(gs.camera_y.load(Ordering::Relaxed)),
                f32::from_bits(gs.camera_z.load(Ordering::Relaxed)),
            ])?;
            dict.set_item("pyramid_yaw_rad", f32::from_bits(gs.pyramid_yaw.load(Ordering::Relaxed)))?;
            dict.set_item("nr_attempts", gs.attempts.load(Ordering::Relaxed))?;

            let align_bits = gs.alignment.load(Ordering::Relaxed);
            let align = f32::from_bits(align_bits);
            if align > 1.5 {
                // Sentinel check - 2.0 means None
                dict.set_item("cosine_alignment", py.None())?;
            } else {
                dict.set_item("cosine_alignment", align)?;
            }

            dict.set_item("is_animating", gs.is_animating.load(Ordering::Relaxed))?;
            dict.set_item("has_won", gs.has_won.load(Ordering::Relaxed))?;

            let win_t = f32::from_bits(gs.win_time.load(Ordering::Relaxed));
            if win_t > 0.001 {
                dict.set_item("win_elapsed_secs", win_t)?;
            } else {
                dict.set_item("win_elapsed_secs", py.None())?;
            }

            Ok(dict.into())
        })
    }

    /// Write commands to shared memory.
    ///
    /// Commands:
    /// - rotate_left, rotate_right: Continuous rotation
    /// - zoom_in, zoom_out: Continuous zoom
    /// - check: Trigger alignment check
    /// - reset: Trigger game reset (reads config from game_structure)
    /// - blank_screen: Toggle blank screen overlay
    /// - stop_rendering: Pause rendering
    /// - resume_rendering: Resume rendering
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
    ) {
        let shm = self.inner.get();
        let cmd = &shm.commands;

        cmd.rotate_left.store(rotate_left, Ordering::Relaxed);
        cmd.rotate_right.store(rotate_right, Ordering::Relaxed);
        cmd.zoom_in.store(zoom_in, Ordering::Relaxed);
        cmd.zoom_out.store(zoom_out, Ordering::Relaxed);

        // Trigger commands - only set to true, game will clear them
        if check {
            cmd.check_alignment.store(true, Ordering::Relaxed);
        }
        if reset {
            cmd.reset.store(true, Ordering::Release);
        }
        if blank_screen {
            cmd.blank_screen.store(true, Ordering::Relaxed);
        }
        if stop_rendering {
            cmd.stop_rendering.store(true, Ordering::Relaxed);
        }
        if resume_rendering {
            cmd.resume_rendering.store(true, Ordering::Relaxed);
        }
    }

    /// Write game structure config fields to shared memory.
    /// These will be applied when the reset command is triggered.
    fn write_game_structure(
        &mut self,
        seed: u64,
        pyramid_type: u32,
        base_radius: f32,
        height: f32,
        start_orient: f32,
        target_door: u32,
        colors: Vec<Vec<f32>>,
    ) -> PyResult<()> {
        if colors.len() != 3 || colors.iter().any(|face| face.len() != 4) {
            return Err(PyErr::new::<PyValueError, _>(format!(
                "expected colors to be a 3x4 matrix, got {:?}",
                colors.iter().map(|face| face.len()).collect::<Vec<_>>()
            )));
        }

        let shm = self.inner.get();
        let gs = &shm.game_structure;

        gs.seed.store(seed, Ordering::Relaxed);
        gs.pyramid_type.store(pyramid_type, Ordering::Relaxed);
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

        Ok(())
    }

    // Legacy method name for backward compatibility
    fn read_game_state(&self) -> PyResult<PyObject> {
        self.read_game_structure()
    }

    // Legacy method name for backward compatibility
    fn write_reset_config(
        &mut self,
        seed: u64,
        pyramid_type: u32,
        base_radius: f32,
        height: f32,
        start_orient: f32,
        target_door: u32,
        colors: Vec<Vec<f32>>,
    ) -> PyResult<()> {
        self.write_game_structure(seed, pyramid_type, base_radius, height, start_orient, target_door, colors)
    }
}

#[pymodule]
#[pyo3(name = "monkey_shared")]
fn monkey_shared(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SharedMemoryWrapper>()?;
    Ok(())
}
