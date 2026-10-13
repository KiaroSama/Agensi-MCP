use std::{
    fs::File,
    io::{self, Write},
    os::{fd::{AsFd, AsRawFd}, unix::fs::FileTypeExt},
};

unsafe extern "C" {
    fn fcntl(fd: i32, command: i32, ...) -> i32;
}
#[cfg(target_os = "linux")]
const NONBLOCK: i32 = 0x800;
#[cfg(not(target_os = "linux"))]
const NONBLOCK: i32 = 0x4; // Supported non-Linux Unix targets are macOS.
const GET_FLAGS: i32 = 3;
const SET_FLAGS: i32 = 4;

pub struct Pipe {
    file: File,
    original: Option<i32>,
}
impl Pipe {
    pub fn stdout() -> io::Result<Option<Self>> {
        // The safe descriptor API creates an independently owned close-on-exec duplicate.
        let file = File::from(io::stdout().as_fd().try_clone_to_owned()?);
        let kind = file.metadata()?.file_type();
        if !kind.is_fifo() && !kind.is_socket() {
            return Ok(None);
        }
        // SAFETY: GET_FLAGS has no variadic argument and receives a live descriptor.
        let original = unsafe { fcntl(file.as_raw_fd(), GET_FLAGS) };
        if original < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pipe = Self { file, original: None };
        if original & NONBLOCK == 0 {
            // SAFETY: SET_FLAGS takes an int; retain all original flags and add only nonblocking mode.
            if unsafe { fcntl(pipe.file.as_raw_fd(), SET_FLAGS, original | NONBLOCK) } < 0 {
                return Err(io::Error::last_os_error());
            }
            pipe.original = Some(original);
        }
        Ok(Some(pipe))
    }
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }
    pub fn restore(&mut self) -> io::Result<()> {
        if let Some(original) = self.original {
            // SAFETY: restore the recorded flags on the still-owned descriptor. Duplicates share flags.
            if unsafe { fcntl(self.file.as_raw_fd(), SET_FLAGS, original) } < 0 {
                return Err(io::Error::last_os_error());
            }
            self.original = None;
        }
        Ok(())
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
