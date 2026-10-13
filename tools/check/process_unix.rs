use super::INTERRUPTED;
use std::{io, sync::atomic::Ordering};

// POSIX pid_t is a signed int on the supported Linux/macOS targets; both define SIGKILL=9.
unsafe extern "C" {
    fn setsid() -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn signal(number: i32, handler: usize) -> usize;
    fn poll(descriptors: *mut PollFd, count: Nfds, timeout_ms: i32) -> i32;
    fn read(fd: i32, buffer: *mut std::ffi::c_void, count: usize) -> isize;
}

#[cfg(target_os = "linux")]
type Nfds = usize;
#[cfg(not(target_os = "linux"))]
type Nfds = u32;
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    returned: i16,
}

pub fn read_gate() -> io::Result<()> {
    let mut gate = [0_u8; 6];
    let mut offset = 0;
    while offset < gate.len() {
        // SAFETY: fd0 is inherited stdin; writable remaining slice; no userspace read-ahead.
        let count = unsafe { read(0, gate[offset..].as_mut_ptr().cast(), gate.len() - offset) };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Gate EOF"));
        }
        offset += count as usize;
    }
    if gate == *b"start\n" {
        Ok(())
    } else {
        Err(io::Error::other("Invalid ownership gate"))
    }
}

pub fn read_pipe<R: io::Read + std::os::fd::AsRawFd>(
    pipe: &mut R,
    buffer: &mut [u8],
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<usize> {
    while !stop.load(Ordering::Relaxed) {
        let mut descriptor = PollFd {
            fd: pipe.as_raw_fd(),
            events: 1,
            returned: 0,
        };
        // SAFETY: one valid repr(C) pollfd; a 50ms bound lets readers observe cancellation.
        let ready = unsafe { poll(&mut descriptor, 1, 50) };
        if ready > 0 {
            return pipe.read(buffer);
        }
        if ready == -1 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    Ok(0)
}

pub fn new_session() -> io::Result<()> {
    // SAFETY: setsid takes no pointers, is async-signal-safe and runs before child exec.
    if unsafe { setsid() } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn stop_group(pid: u32) -> io::Result<()> {
    let pid = i32::try_from(pid).map_err(|_| io::Error::other("Invalid owned process id"))?;
    if pid <= 1 {
        return Err(io::Error::other("Invalid owned process group"));
    }
    // SAFETY: the child called setsid, so -pid targets only its owned group, even after parent exit.
    if unsafe { kill(-pid, 9) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // ESRCH is 3 on Linux/macOS: an already-empty group is successful cleanup.
    if error.raw_os_error() == Some(3) {
        Ok(())
    } else {
        Err(error)
    }
}

extern "C" fn interrupt(_: i32) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

pub struct InterruptGuard(usize);
impl InterruptGuard {
    pub fn install() -> io::Result<Self> {
        INTERRUPTED.store(false, Ordering::Relaxed);
        // SAFETY: callback has the signal ABI, only stores a lock-free atomic, and lives forever.
        let previous = unsafe { signal(2, interrupt as *const () as usize) };
        if previous == usize::MAX {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(previous))
        }
    }
}
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        // SAFETY: restore the exact prior SIGINT disposition while callback remains valid.
        unsafe { signal(2, self.0) };
    }
}

pub fn alive(pid: u32) -> bool {
    let Ok(pid_number) = i32::try_from(pid) else {
        return false;
    };
    if pid_number <= 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    if let Ok(state) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        && state
            .split_once(") ")
            .is_some_and(|(_, rest)| rest.starts_with('Z'))
    {
        return false;
    }
    // SAFETY: signal zero queries existence without delivering a signal to the observed PID.
    unsafe { kill(pid_number, 0) == 0 }
}
