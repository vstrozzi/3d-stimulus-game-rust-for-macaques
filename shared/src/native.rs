use crate::SharedMemory;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

/// Wrapper for file-based shared memory on native platforms (UNIX).
pub struct NativeSharedMemory {
    ptr: *mut SharedMemory,
    #[allow(dead_code)]
    file: File,
    #[allow(dead_code)]
    path: PathBuf,
}

// Initialize shared memory region (by creating or opening existing)
impl NativeSharedMemory {
    pub fn new(name: &str, create: bool) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("monkey_shm_{}", name));
        let size = std::mem::size_of::<SharedMemory>();

        eprintln!("[shared] {} memory at: {:?} (size={})", 
            if create { "Creating" } else { "Opening" }, &path, size);

        let file = if create {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)?;
            let zeroes = vec![0u8; size];
            file.write_all(&zeroes)?;
            file.sync_all()?;
            file
        } else {
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)?
        };

        #[cfg(unix)]
        let ptr = unsafe {
            use std::os::unix::io::AsRawFd;
            let fd = file.as_raw_fd();
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            if ptr == libc::MAP_FAILED {
                return Err(std::io::Error::last_os_error());
            }
            ptr as *mut SharedMemory
        };

        if create {
            unsafe {
                std::ptr::write(ptr, SharedMemory::new());
            }
        }

        Ok(Self { ptr, file, path })
    }

    pub fn get(&self) -> &SharedMemory {
        unsafe { &*self.ptr }
    }

    pub fn get_mut(&mut self) -> &mut SharedMemory {
        unsafe { &mut *self.ptr }
    }
}

// Drop the shared memory mapping
impl Drop for NativeSharedMemory {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::munmap(
                self.ptr as *mut libc::c_void,
                std::mem::size_of::<SharedMemory>(),
            );
        }
    }
}

unsafe impl Send for NativeSharedMemory {}
unsafe impl Sync for NativeSharedMemory {}


// ToDo: Maybe Arc is not needed
pub type SharedMemoryHandle = Arc<NativeSharedMemory>;

pub fn create_shared_memory(name: &str) -> std::io::Result<SharedMemoryHandle> {
    Ok(Arc::new(NativeSharedMemory::new(name, true)?))
}

pub fn open_shared_memory(name: &str) -> std::io::Result<SharedMemoryHandle> {
    Ok(Arc::new(NativeSharedMemory::new(name, false)?))
}
