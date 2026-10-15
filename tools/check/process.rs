//! Own the complete command tree, including descendants after the direct parent exits.
use std::{
    ffi::OsString,
    io::{self, Write},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
#[path = "process_unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "process_windows.rs"]
mod platform;

pub static INTERRUPTED: AtomicBool = AtomicBool::new(false);
pub use platform::{InterruptGuard, read_pipe};

pub struct OwnedProcess {
    pub child: Child,
    #[cfg(windows)]
    job: Option<platform::Job>,
    stopped: bool,
}

impl OwnedProcess {
    pub fn spawn(root: &Path, args: &[OsString]) -> io::Result<Self> {
        Self::launch(root, args, None, None, None)
    }

    pub fn spawn_with_input(root: &Path, args: &[OsString], input: &[u8]) -> io::Result<Self> {
        // Menu input fits comfortably in both native pipe buffers, avoiding an unmonitored write.
        if input.len() > 1024 {
            return Err(io::Error::other("Owned input exceeds1024byte limit"));
        }
        Self::launch(root, args, Some(input), None, None)
    }

    /// Preserve fixture CWD, streams and a bounded input without re-entering a libtest executable.
    pub fn spawn_fixture_with_input(
        bootstrap_host: &Path,
        child_cwd: &Path,
        args: &[OsString],
        input: &[u8],
        environment: &[(OsString, OsString)],
    ) -> io::Result<Self> {
        if input.len() > 1024 {
            return Err(io::Error::other("Owned input exceeds1024byte limit"));
        }
        if !bootstrap_host.is_absolute() || !bootstrap_host.is_file() {
            return Err(io::Error::other(
                "Trusted built bootstrap host is unavailable",
            ));
        }
        Self::launch(
            child_cwd,
            args,
            Some(input),
            Some(bootstrap_host),
            Some(environment),
        )
    }

    fn launch(
        root: &Path,
        args: &[OsString],
        input: Option<&[u8]>,
        bootstrap_host: Option<&Path>,
        environment: Option<&[(OsString, OsString)]>,
    ) -> io::Result<Self> {
        let program = args
            .first()
            .ok_or_else(|| io::Error::other("Missing command"))?;
        #[cfg(windows)]
        let job = platform::Job::new()?;
        #[cfg(windows)]
        let mut command = {
            use std::os::windows::process::CommandExt;
            let executable = match bootstrap_host {
                Some(host) => host.to_owned(),
                None => std::env::current_exe()?,
            };
            let mut command = Command::new(executable);
            command
                .arg(if input.is_some() {
                    "--bootstrap-input"
                } else {
                    "--bootstrap"
                })
                .args(args);
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW.
            command.stdin(Stdio::piped());
            command
        };
        #[cfg(unix)]
        let mut command = {
            use std::os::unix::process::CommandExt;
            let mut command = Command::new(program);
            command.args(&args[1..]).stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
            // SAFETY: only async-signal-safe setsid and errno access run between fork and exec.
            unsafe { command.pre_exec(platform::new_session) };
            command
        };
        #[cfg(windows)]
        let _ = program;
        #[cfg(unix)]
        let _ = bootstrap_host;
        if let Some(environment) = environment {
            command
                .env_clear()
                .envs(environment.iter().map(|(key, value)| (key, value)));
        }
        command
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn()?;
        let mut owned = Self {
            child,
            #[cfg(windows)]
            job: Some(job),
            stopped: false,
        };
        #[cfg(windows)]
        {
            // The bootstrap cannot execute any command until job assignment succeeds.
            if let Err(error) = owned
                .job
                .as_ref()
                .ok_or_else(|| io::Error::other("Missing job"))?
                .assign(&owned.child)
            {
                let _ = owned.child.kill();
                let _ = owned.stop();
                return Err(error);
            }
        }
        if let Some(mut gate) = owned.child.stdin.take() {
            let write = (|| -> io::Result<()> {
                #[cfg(windows)]
                gate.write_all(b"start\n")?;
                if let Some(input) = input {
                    gate.write_all(input)?;
                }
                Ok(())
            })();
            drop(gate); // EOF follows the bounded input on every successful and failing path.
            if let Err(error) = write {
                let _ = owned.stop();
                return Err(error);
            }
        } else if input.is_some() || cfg!(windows) {
            let _ = owned.stop();
            return Err(io::Error::other("Missing owned input pipe"));
        }
        Ok(owned)
    }

    pub fn stop(&mut self) -> io::Result<()> {
        if self.stopped {
            return Ok(());
        }
        #[cfg(windows)]
        let termination = {
            let result = self.job.as_ref().map_or(Ok(()), platform::Job::stop);
            // Close before reaping/readers even if termination or accounting failed.
            drop(self.job.take());
            result
        };
        #[cfg(unix)]
        let termination = platform::stop_group(self.child.id());
        let reap = reap(&mut self.child, Duration::from_secs(5));
        self.stopped = termination.is_ok() && reap.is_ok();
        termination.and(reap.map(|_| ()))
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if !self.stopped {
            #[cfg(windows)]
            {
                if let Some(job) = self.job.take() {
                    let _ = job.stop();
                    drop(job);
                }
            }
            #[cfg(unix)]
            let _ = platform::stop_group(self.child.id());
            let _ = reap(&mut self.child, Duration::from_secs(5));
        }
    }
}

pub fn reap(child: &mut Child, limit: Duration) -> io::Result<ExitStatus> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Owned wait exceeded limit",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        // Match Python's signal return value; process::exit converts it to the OS exit status.
        -status.signal().unwrap_or(1)
    }
    #[cfg(not(unix))]
    1
}

pub fn bootstrap(args: Vec<OsString>) -> i32 {
    bootstrap_command(args, false)
}
pub fn bootstrap_input(args: Vec<OsString>) -> i32 {
    bootstrap_command(args, true)
}

fn bootstrap_command(args: Vec<OsString>, inherit_input: bool) -> i32 {
    // Read only six bytes from the raw handle: std::io::Stdin buffering could swallow menu input.
    if platform::read_gate().is_err() {
        return 1;
    }
    let Some(program) = args.first() else {
        return 1;
    };
    let mut command = Command::new(program);
    command.args(&args[1..]).stdin(if inherit_input {
        Stdio::inherit()
    } else {
        Stdio::null()
    });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        if platform::restrict_stdio_inheritance().is_err() {
            eprintln!(
                "[{} UTC] [ERROR] [check] Cannot restrict bootstrap handle inheritance; payload omitted.",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
            );
            return 1;
        }
    }
    match command.status() {
        Ok(status) => exit_code(status),
        Err(_) => {
            eprintln!(
                "[{} UTC] [ERROR] [check] Command launch failed (IoError); payload omitted.",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
            );
            1
        }
    }
}

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

/// Fixture observation only; zombies are no longer executing owned work.
pub fn alive(pid: u32) -> bool {
    platform::alive(pid)
}
