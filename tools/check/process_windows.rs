use super::INTERRUPTED;
use std::{
    ffi::c_void,
    io,
    os::windows::io::AsRawHandle,
    process::Child,
    ptr,
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    minimum_working_set: usize,
    maximum_working_set: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    basic: BasicLimits,
    io_counters: [u64; 6],
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}
#[repr(C)]
#[derive(Default)]
struct Accounting {
    total_user_time: i64,
    total_kernel_time: i64,
    this_period_user_time: i64,
    this_period_kernel_time: i64,
    total_page_fault_count: u32,
    total_processes: u32,
    active_processes: u32,
    total_terminated_processes: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, size: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn QueryInformationJobObject(
        job: Handle,
        class: i32,
        info: *mut c_void,
        size: u32,
        returned: *mut u32,
    ) -> i32;
    fn TerminateJobObject(job: Handle, code: u32) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn SetConsoleCtrlHandler(
        handler: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
    fn GetStdHandle(kind: u32) -> Handle;
    fn ReadFile(
        file: Handle,
        buffer: *mut c_void,
        size: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn PeekNamedPipe(
        pipe: Handle,
        buffer: *mut c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        remaining: *mut u32,
    ) -> i32;
}

pub fn read_gate() -> io::Result<()> {
    // SAFETY: STD_INPUT_HANDLE queries the process-owned inherited pipe; no handle ownership transfer.
    let input = unsafe { GetStdHandle(-10_i32 as u32) };
    if input.is_null() || input as isize == -1 {
        return Err(io::Error::other("Missing gate input"));
    }
    let mut gate = [0_u8; 6];
    let mut offset = 0;
    while offset < gate.len() {
        let mut count = 0;
        // SAFETY: writable remaining slice; synchronous inherited pipe; never read beyond the gate.
        if unsafe {
            ReadFile(
                input,
                gate[offset..].as_mut_ptr().cast(),
                (gate.len() - offset) as u32,
                &mut count,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
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

pub fn read_pipe<R: io::Read + AsRawHandle>(
    pipe: &mut R,
    buffer: &mut [u8],
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<usize> {
    while !stop.load(Ordering::Relaxed) {
        let mut available = 0;
        // SAFETY: query our pipe without consuming data; available is a writable DWORD.
        if unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                &mut available,
                ptr::null_mut(),
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(109 | 232)) {
                return Ok(0);
            }
            return Err(error);
        }
        if available > 0 {
            let length = buffer.len().min(available as usize);
            return pipe.read(&mut buffer[..length]);
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(0)
}

pub struct Job(Handle);
impl Job {
    pub fn new() -> io::Result<Self> {
        // SAFETY: null attributes/name request a new unnamed, noninheritable owned job.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self(handle);
        let info = ExtendedLimits {
            basic: BasicLimits {
                flags: 0x2000,
                ..BasicLimits::default()
            },
            ..ExtendedLimits::default()
        };
        // SAFETY: repr(C) matches JOBOBJECT_EXTENDED_LIMIT_INFORMATION; pointer and size agree.
        if unsafe {
            SetInformationJobObject(
                handle,
                9,
                (&info as *const ExtendedLimits).cast(),
                size_of::<ExtendedLimits>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        // SAFETY: Child owns a valid process handle; self owns the live job handle.
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    fn active(&self) -> io::Result<u32> {
        let mut info = Accounting::default();
        // SAFETY: repr(C) matches BASIC_ACCOUNTING_INFORMATION, with writable full-size storage.
        if unsafe {
            QueryInformationJobObject(
                self.0,
                1,
                (&mut info as *mut Accounting).cast(),
                size_of::<Accounting>() as u32,
                ptr::null_mut(),
            )
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(info.active_processes)
        }
    }
    pub fn stop(&self) -> io::Result<()> {
        // SAFETY: terminating only this owned job, never a job inherited from the host.
        if unsafe { TerminateJobObject(self.0, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.active()? != 0 {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Owned job did not empty",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: close our unique handle; KILL_ON_JOB_CLOSE is the failure backstop.
        unsafe { CloseHandle(self.0) };
    }
}

unsafe extern "system" fn interrupt(kind: u32) -> i32 {
    if kind == 0 || kind == 1 {
        INTERRUPTED.store(true, Ordering::Relaxed);
        1
    } else {
        0
    }
}
pub struct InterruptGuard;
impl InterruptGuard {
    pub fn install() -> io::Result<Self> {
        INTERRUPTED.store(false, Ordering::Relaxed);
        // SAFETY: permanent callback has the required ABI and only writes a lock-free atomic.
        if unsafe { SetConsoleCtrlHandler(Some(interrupt), 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self)
        }
    }
}
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        // SAFETY: removes only our callback, leaving the host's other handlers intact.
        unsafe { SetConsoleCtrlHandler(Some(interrupt), 0) };
    }
}

pub fn alive(pid: u32) -> bool {
    // SAFETY: request query-only access; failure to open is treated as not live, as source oracle.
    let handle = unsafe { OpenProcess(0x1000, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut code = 0;
    // SAFETY: valid owned process handle and writable DWORD; handle closed exactly once.
    let live = unsafe { GetExitCodeProcess(handle, &mut code) != 0 && code == 259 };
    // SAFETY: handle was returned by OpenProcess and remains ours.
    unsafe { CloseHandle(handle) };
    live
}
