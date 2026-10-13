//! Self-executable synthetic child programs; no Python runtime or shell-string evaluation.
use crate::process;
use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub fn child(args: &[OsString]) -> i32 {
    let mode = args.first().and_then(|v| v.to_str()).unwrap_or("");
    let result = (|| -> io::Result<()> {
        match mode {
            "exit7" => return Ok(()),
            "empty" => {}
            "stdout-progress" | "stderr-progress" => {
                let mut output: Box<dyn Write> = if mode == "stdout-progress" {
                    Box::new(io::stdout())
                } else {
                    Box::new(io::stderr())
                };
                // This delay models output arriving over time, not a readiness wait.
                for _ in 0..20 {
                    output.write_all(b"x")?;
                    output.flush()?;
                    thread::sleep(Duration::from_millis(100));
                }
                output.write_all(b"END")?;
            }
            "stdout-bytes" => io::stdout().write_all(b"begin\xffend\n")?,
            "stderr-bytes" => io::stderr().write_all(b"begin\xffend\n")?,
            "split" => {
                io::stdout().write_all(b"\xe2")?;
                io::stdout().flush()?;
                thread::sleep(Duration::from_millis(100));
                io::stdout().write_all(b"\x82\xacEND")?;
            }
            "marker" => {
                fs::write(Path::new(&args[1]), b"")?;
            }
            "large" => {
                for _ in 0..256 {
                    io::stdout().write_all(&[b'x'; 8192])?;
                }
                io::stdout().write_all(b"END")?;
            }
            "idle" => thread::sleep(Duration::from_secs(6)),
            "wall" => {
                for _ in 0..100 {
                    io::stdout().write_all(b"x\n")?;
                    io::stdout().flush()?;
                    thread::sleep(Duration::from_millis(50));
                }
            }
            "payload" => {
                io::stdout().write_all(args[1].to_string_lossy().as_bytes())?;
                io::stdout().write_all(b"\n")?;
            }
            "parent-inherit" | "parent-null" => {
                let mut command = Command::new(std::env::current_exe()?);
                command
                    .arg("--fixture")
                    .arg("lease")
                    .arg(&args[1])
                    .stdin(Stdio::null());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                    prevent_unselected_stdio_inheritance()?;
                }
                if mode == "parent-null" {
                    command.stdout(Stdio::null()).stderr(Stdio::null());
                }
                let mut descendant = command.spawn()?;
                if let Err(error) =
                    fs::write(Path::new(&args[2]), descendant.id().to_string().as_bytes())
                {
                    let _ = descendant.kill();
                    let _ = process::reap(&mut descendant, Duration::from_secs(2));
                    return Err(error);
                }
                // Deliberately leave the direct parent; the runner owns the descendant.
            }
            "lease" => {
                let deadline = Instant::now() + Duration::from_secs(8);
                while Path::new(&args[1]).exists() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(20));
                }
            }
            _ => return Err(io::Error::other("Unknown fixture mode")),
        }
        Ok(())
    })();
    if result.is_err() {
        1
    } else if mode == "exit7" {
        7
    } else {
        0
    }
}

#[cfg(windows)]
fn prevent_unselected_stdio_inheritance() -> io::Result<()> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(kind: u32) -> *mut c_void;
        fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
    }
    // Python close_fds=True excludes the originals; Rust1.99 otherwise inherits unrelated handles.
    // Command duplicates explicitly selected inherited stdio, so parent-inherit keeps its pipes.
    for kind in [-10_i32, -11, -12] {
        // SAFETY: query this fixture process's own standard handle, without taking ownership.
        let handle = unsafe { GetStdHandle(kind as u32) };
        if handle.is_null() || handle as isize == -1 {
            continue;
        }
        // SAFETY: change only HANDLE_FLAG_INHERIT on the live handle owned by this fixture process.
        if unsafe { SetHandleInformation(handle, 1, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub root: PathBuf,
    pub runner: PathBuf,
}
impl Fixture {
    // Explicit construction allocates disposable filesystem resources; it is not a default value.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tmp");
        fs::create_dir_all(&scratch).expect("Create project-local scratch");
        let directory = tempfile::Builder::new()
            .prefix("check streams-")
            .tempdir_in(scratch)
            .expect("Create fixture");
        let root = directory.path().join("project with spaces");
        fs::create_dir(&root).expect("Create fixture project");
        // Runtime root discovery stops here instead of finding the enclosing real checkout.
        fs::write(
            root.join("Cargo.toml"),
            b"[package]\nname=\"runner-fixture\"\n",
        )
        .expect("Fixture root marker");
        fs::write(root.join("run.ps1"), b"").expect("Fixture launcher marker");
        let runner = root.join(if cfg!(windows) {
            "check-run.exe"
        } else {
            "check-run"
        });
        fs::copy(env!("CARGO_BIN_EXE_check-run"), &runner).expect("Copy built native runner");
        Self {
            directory,
            root,
            runner,
        }
    }
    pub fn program(&self, mode: &str, extra: &[OsString]) -> Vec<OsString> {
        let mut command = vec![
            std::env::current_exe()
                .expect("Fixture executable")
                .into_os_string(),
            "--fixture".into(),
            mode.into(),
        ];
        command.extend_from_slice(extra);
        command
    }
    pub fn invoke(&self, command: Vec<OsString>, wall: &str, idle: &str) -> Output {
        let mut args = vec![
            self.runner.clone().into_os_string(),
            format!("--wall={wall}").into(),
            format!("--idle={idle}").into(),
            "--".into(),
        ];
        args.extend(command);
        capture(self.directory.path(), &args)
    }
    pub fn run(&self, mode: &str) -> Output {
        self.invoke(self.program(mode, &[]), "6", "3")
    }
    pub fn logs(&self) -> Vec<PathBuf> {
        fs::read_dir(self.root.join("logs"))
            .expect("Log directory")
            .map(|entry| entry.expect("Log entry").path())
            .collect()
    }
}

pub fn capture(root: &Path, command: &[OsString]) -> Output {
    capture_owned(
        process::OwnedProcess::spawn(root, command).expect("Spawn bounded fixture invocation"),
        Duration::from_secs(15),
    )
}

pub fn capture_with_input(
    root: &Path,
    command: &[OsString],
    input: &[u8],
    limit: Duration,
) -> Output {
    capture_owned(
        process::OwnedProcess::spawn_with_input(root, command, input)
            .expect("Spawn bounded fixture with input"),
        limit,
    )
}

fn capture_owned(mut owned: process::OwnedProcess, limit: Duration) -> Output {
    let stop = Arc::new(AtomicBool::new(false));
    let stdout = owned.child.stdout.take().expect("Captured stdout");
    let stderr = owned.child.stderr.take().expect("Captured stderr");
    #[cfg(unix)]
    fn reader<R: Read + std::os::fd::AsRawFd + Send + 'static>(
        mut pipe: R,
        stop: Arc<AtomicBool>,
    ) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || collect(&mut pipe, &stop))
    }
    #[cfg(windows)]
    fn reader<R: Read + std::os::windows::io::AsRawHandle + Send + 'static>(
        mut pipe: R,
        stop: Arc<AtomicBool>,
    ) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || collect(&mut pipe, &stop))
    }
    let stdout = reader(stdout, stop.clone());
    let stderr = reader(stderr, stop.clone());
    let status = process::reap(&mut owned.child, limit);
    let cleanup = owned.stop();
    // Normally let EOF drain captures; cancellation is reserved for an exceeded outer bound.
    if status.is_err() || cleanup.is_err() {
        stop.store(true, Ordering::Relaxed);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !(stdout.is_finished() && stderr.is_finished()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let drained = stdout.is_finished() && stderr.is_finished();
    stop.store(true, Ordering::Relaxed);
    // On failure, give bounded native readiness loops one final cancellation window before unwind.
    let deadline = Instant::now() + Duration::from_secs(1);
    while !(stdout.is_finished() && stderr.is_finished()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        stdout.is_finished() && stderr.is_finished(),
        "Fixture capture cancellation failed"
    );
    let stdout = stdout.join().expect("Stdout reader");
    let stderr = stderr.join().expect("Stderr reader");
    assert!(drained, "Fixture capture readers failed bounded shutdown");
    cleanup.expect("Outer fixture tree cleanup");
    Output {
        status: status.expect("Fixture invocation exceeded outer wall bound"),
        stdout,
        stderr,
    }
}

#[cfg(unix)]
fn collect<R: Read + std::os::fd::AsRawFd>(pipe: &mut R, stop: &AtomicBool) -> Vec<u8> {
    collect_with(|buffer| process::read_pipe(pipe, buffer, stop))
}
#[cfg(windows)]
fn collect<R: Read + std::os::windows::io::AsRawHandle>(
    pipe: &mut R,
    stop: &AtomicBool,
) -> Vec<u8> {
    collect_with(|buffer| process::read_pipe(pipe, buffer, stop))
}
fn collect_with(mut read: impl FnMut(&mut [u8]) -> io::Result<usize>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 16384];
    loop {
        let length = read(&mut buffer).expect("Read fixture capture");
        if length == 0 {
            return bytes;
        }
        assert!(
            bytes.len() + length <= 3 * 1024 * 1024,
            "Fixture capture exceeded expected3MiB maximum"
        );
        bytes.extend_from_slice(&buffer[..length]);
    }
}

pub fn wait_dead(pid: u32, limit: Duration) {
    let deadline = Instant::now() + limit;
    while process::alive(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !process::alive(pid),
        "An owned descendant survived runner cleanup"
    );
}
