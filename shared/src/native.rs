use crate::SharedMemory;
use std::fs::OpenOptions;
use std::sync::Arc;

fn shm_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("monkey_shm_{}", name))
}

/// File-backed shared memory region on native UNIX platforms.
pub struct NativeSharedMemory {
    ptr: *mut SharedMemory,
}

impl NativeSharedMemory {
    /// Creates (or recreates) the SHM file, zeroes it, and writes the initial struct.
    /// Must only be called by the game node at startup.
    fn create(name: &str) -> std::io::Result<Self> {
        let size = std::mem::size_of::<SharedMemory>();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(shm_path(name))?;

        // Use set_len instead of truncate(true) + write_all to avoid
        // briefly setting file size to 0, which would SIGBUS any process
        // that still has the old file mapped.
        file.set_len(size as u64)?;
        file.sync_all()?;

        let ptr = unsafe { mmap_file(&file, size)? };
        unsafe { std::ptr::write(ptr, SharedMemory::new()) };
        Ok(Self { ptr })
    }

    /// Attaches to an existing SHM file without modifying its contents.
    /// Must only be called by the Python controller (or any late-joining process).
    fn open(name: &str) -> std::io::Result<Self> {
        let size = std::mem::size_of::<SharedMemory>();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(shm_path(name))?;

        let file_len = file.metadata()?.len() as usize;
        if file_len != size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("SHM size mismatch: file is {file_len} bytes, expected {size}. \
                         Rebuild the shared library and restart the game."),
            ));
        }

        let ptr = unsafe { mmap_file(&file, size)? };
        Ok(Self { ptr })
    }

    pub fn get(&self) -> &SharedMemory {
        unsafe { &*self.ptr }
    }

    pub fn get_mut(&mut self) -> &mut SharedMemory {
        unsafe { &mut *self.ptr }
    }
}

#[cfg(unix)]
unsafe fn mmap_file(file: &std::fs::File, size: usize) -> std::io::Result<*mut SharedMemory> {
    use std::os::unix::io::AsRawFd;
    let ptr = libc::mmap(
        std::ptr::null_mut(),
        size,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        file.as_raw_fd(),
        0,
    );
    if ptr == libc::MAP_FAILED {
        return Err(std::io::Error::last_os_error());
    }
    Ok(ptr as *mut SharedMemory)
}

impl Drop for NativeSharedMemory {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, std::mem::size_of::<SharedMemory>());
        }
    }
}

unsafe impl Send for NativeSharedMemory {}
unsafe impl Sync for NativeSharedMemory {}

pub type SharedMemoryHandle = Arc<NativeSharedMemory>;

/// Creates a fresh SHM segment. Call this from the game node only.
pub fn create_shared_memory(name: &str) -> std::io::Result<SharedMemoryHandle> {
    Ok(Arc::new(NativeSharedMemory::create(name)?))
}

/// Attaches to an existing SHM segment without truncating it.
/// Call this from the Python controller.
pub fn open_shared_memory(name: &str) -> std::io::Result<SharedMemoryHandle> {
    Ok(Arc::new(NativeSharedMemory::open(name)?))
}
