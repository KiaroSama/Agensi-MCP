use std::{
    ffi::c_void,
    fs::File,
    io::{self, Write},
    os::windows::io::{AsHandle, AsRawHandle},
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileType(handle: *mut c_void) -> u32;
    fn GetNamedPipeHandleStateW(
        handle: *mut c_void,
        state: *mut u32,
        instances: *mut u32,
        collection: *mut u32,
        timeout: *mut u32,
        user: *mut u16,
        user_length: u32,
    ) -> i32;
    fn SetNamedPipeHandleState(
        handle: *mut c_void,
        mode: *const u32,
        collection: *const u32,
        timeout: *const u32,
    ) -> i32;
}

pub struct Pipe {
    file: File,
    original: Option<u32>,
}
impl Pipe {
    pub fn stdout() -> io::Result<Option<Self>> {
        let stdout = io::stdout();
        let handle = stdout.as_raw_handle();
        if handle.is_null() || handle as isize == -1 {
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        }
        // SAFETY: GetFileType only observes the live standard handle.
        match unsafe { GetFileType(handle) } {
            3 => {},
            1 | 2 => return Ok(None),
            _ => return Err(io::ErrorKind::Unsupported.into()),
        }
        let file = File::from(stdout.as_handle().try_clone_to_owned()?);
        let mut original = 0;
        // SAFETY: state points to a writable u32; every optional unused output is null.
        if unsafe {
            GetNamedPipeHandleStateW(
                file.as_raw_handle(),
                &mut original,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pipe = Self { file, original: None };
        if original & 1 == 0 {
            let mode = original | 1; // PIPE_NOWAIT; preserve the existing read mode.
            pipe.set_mode(mode)?;
            pipe.original = Some(original);
        }
        Ok(Some(pipe))
    }
    fn set_mode(&self, mode: u32) -> io::Result<()> {
        // SAFETY: the handle is owned and live; mode is a valid state, optional buffering controls are null.
        if unsafe {
            SetNamedPipeHandleState(
                self.file.as_raw_handle(),
                &mode,
                std::ptr::null(),
                std::ptr::null(),
            )
        } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }
    pub fn restore(&mut self) -> io::Result<()> {
        if let Some(original) = self.original {
            self.set_mode(original)?;
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
