//! Bounded byte streaming; status records never contain command arguments or child payload.
use crate::process::{self, OwnedProcess};
use chrono::Utc;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const CHUNK_BYTES: usize = 16 * 1024;
pub const PENDING_CHUNKS: usize = 64;

pub struct CheckLog {
    file: Mutex<Option<File>>,
    debug: bool,
}
impl CheckLog {
    pub fn new(root: &Path, debug: bool) -> Self {
        let file = (|| -> anyhow::Result<File> {
            let directory = root.join("logs");
            // Reuse the existing cooperative link/reparse/nonregular refusal, including ancestors.
            agensi_mcp::storage::check_file(&directory.join(".check-path-probe"))?;
            fs::create_dir_all(&directory)?;
            agensi_mcp::storage::check_file(&directory.join(".check-path-probe"))?;
            let path = directory.join(format!(
                "check_{}_{}.log",
                Utc::now().format("%Y-%m-%d_%H-%M-%S_UTC"),
                uuid::Uuid::new_v4().simple()
            ));
            agensi_mcp::storage::check_file(&path)?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            Ok(options.open(path)?)
        })();
        let logger = Self {
            file: Mutex::new(file.ok()),
            debug,
        };
        if logger.file.lock().map(|v| v.is_none()).unwrap_or(true) {
            logger.event(
                "WARNING",
                "File logging unavailable; check status uses stderr.",
            );
        }
        logger
    }
    pub fn event(&self, level: &str, message: &str) {
        if level == "DEBUG" && !self.debug {
            return;
        }
        let level = match level {
            "INFO" | "WARNING" | "ERROR" | "DEBUG" => level,
            _ => "WARNING",
        };
        let message: String = message
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let entry = format!(
            "[{} UTC] [{level}] [check] {message}\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S")
        );
        // A broken status sink must not panic during cleanup; the file remains independent.
        let _ = io::stderr().lock().write_all(entry.as_bytes());
        if let Ok(mut guard) = self.file.lock()
            && let Some(file) = guard.as_mut()
            && file
                .write_all(entry.as_bytes())
                .and_then(|_| file.flush())
                .is_err()
        {
            *guard = None;
        }
    }
}

pub enum OutputEvent {
    Chunk(Vec<u8>),
    Error(io::ErrorKind),
    Eof,
}
fn enqueue(events: &SyncSender<OutputEvent>, stop: &AtomicBool, mut event: OutputEvent) -> bool {
    while !stop.load(Ordering::Relaxed) {
        match events.try_send(event) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(returned)) => event = returned,
        }
        // sync_channel has no stable send_timeout; a bounded cancellable retry avoids send deadlock.
        thread::sleep(Duration::from_millis(10));
    }
    false
}

/// The read boundary returns bytes, EOF or a typed error; exception payloads never cross it.
pub fn consume_output(
    mut read: impl FnMut(&mut [u8]) -> io::Result<usize>,
    events: &SyncSender<OutputEvent>,
    stop: &AtomicBool,
) {
    let mut buffer = [0; CHUNK_BYTES];
    while !stop.load(Ordering::Relaxed) {
        match read(&mut buffer) {
            Ok(0) => break,
            Ok(length) if length <= buffer.len() => {
                if !enqueue(events, stop, OutputEvent::Chunk(buffer[..length].to_vec())) {
                    return;
                }
            }
            Ok(_) => {
                enqueue(events, stop, OutputEvent::Error(io::ErrorKind::InvalidData));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                enqueue(events, stop, OutputEvent::Error(error.kind()));
                break;
            }
        }
    }
    enqueue(events, stop, OutputEvent::Eof);
}

#[cfg(unix)]
fn reader<R: io::Read + std::os::fd::AsRawFd + Send + 'static>(
    mut pipe: R,
    events: SyncSender<OutputEvent>,
    stop: Arc<AtomicBool>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("check-output".into())
        .spawn(move || {
            consume_output(
                |buffer| process::read_pipe(&mut pipe, buffer, &stop),
                &events,
                &stop,
            )
        })
}
#[cfg(windows)]
fn reader<R: io::Read + std::os::windows::io::AsRawHandle + Send + 'static>(
    mut pipe: R,
    events: SyncSender<OutputEvent>,
    stop: Arc<AtomicBool>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("check-output".into())
        .spawn(move || {
            consume_output(
                |buffer| process::read_pipe(&mut pipe, buffer, &stop),
                &events,
                &stop,
            )
        })
}

struct Options {
    wall: f64,
    idle: f64,
    debug: bool,
    command: Vec<OsString>,
}
fn positive_finite(value: &std::ffi::OsStr) -> Result<f64, &'static str> {
    let value = value
        .to_str()
        .ok_or("A finite positive bound is required")?;
    // Python float accepts surrounding whitespace. Underscores follow Python's digit grammar.
    let value = value.trim();
    if value.as_bytes().iter().enumerate().any(|(index, &c)| {
        c == b'_'
            && (index == 0
                || index + 1 == value.len()
                || !value.as_bytes()[index - 1].is_ascii_digit()
                || !value.as_bytes()[index + 1].is_ascii_digit())
    }) {
        return Err("A finite positive bound is required");
    }
    let number: f64 = value
        .replace('_', "")
        .parse()
        .map_err(|_| "A finite positive bound is required")?;
    if number.is_finite() && number > 0.0 {
        Ok(number)
    } else {
        Err("A finite positive bound is required")
    }
}
fn parse(args: Vec<OsString>) -> Result<Option<Options>, &'static str> {
    let mut wall = None;
    let mut idle = None;
    let mut debug = false;
    let mut command = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            command.extend(args);
            break;
        }
        if arg == "--help" || arg == "-h" {
            return Ok(None);
        }
        if arg == "--debug" {
            debug = true;
            continue;
        }
        if arg == "--wall" || arg == "--idle" {
            let value = args.next().ok_or("A finite positive bound is required")?;
            let number = positive_finite(&value)?;
            if arg == "--wall" {
                wall = Some(number);
            } else {
                idle = Some(number);
            }
        } else if let Some(value) = arg.to_str().and_then(|v| v.strip_prefix("--wall=")) {
            wall = Some(positive_finite(std::ffi::OsStr::new(value))?);
        } else if let Some(value) = arg.to_str().and_then(|v| v.strip_prefix("--idle=")) {
            idle = Some(positive_finite(std::ffi::OsStr::new(value))?);
        } else if arg.to_str().is_some_and(|v| v.starts_with('-')) {
            return Err("Unknown check option; arguments omitted");
        } else {
            command.push(arg);
            command.extend(args);
            break;
        }
    }
    let wall = wall.ok_or("--wall is required")?;
    let idle = idle.ok_or("--idle is required")?;
    if command.is_empty() {
        return Err("A command is required");
    }
    Ok(Some(Options {
        wall,
        idle,
        debug,
        command,
    }))
}

pub fn run(root: &Path, args: Vec<OsString>) -> i32 {
    let options = match parse(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            return if writeln!(io::stdout(), "usage: check-run --wall WALL --idle IDLE [--debug] [--] command [args...]\n\nBound and reap one CI check, including descendants surviving their direct parent.\n\n  -h, --help   show this help message and exit\n  --wall WALL  finite positive wall limit in seconds\n  --idle IDLE  finite positive output-idle limit in seconds\n  --debug      include DEBUG status records").is_ok() { 0 } else { 1 };
        }
        Err(message) => {
            let _ = writeln!(io::stderr(), "check-run: {message}");
            return 2;
        }
    };
    let log = CheckLog::new(root, options.debug);
    let stop = Arc::new(AtomicBool::new(false));
    let (events, receiver) = mpsc::sync_channel(PENDING_CHUNKS);
    let mut owned = None;
    let mut readers = Vec::new();
    let started = Instant::now();
    let interrupt = process::InterruptGuard::install();
    let result = (|| -> io::Result<i32> {
        interrupt
            .as_ref()
            .map_err(|_| io::Error::other("Cannot install interrupt handler"))?;
        let process = OwnedProcess::spawn(root, &options.command)?;
        let pid = process.child.id();
        owned = Some(process);
        let process = owned
            .as_mut()
            .ok_or_else(|| io::Error::other("Missing owned process"))?;
        log.event(
            "INFO",
            &format!(
                "Started pid={pid} wall={} idle={}; arguments omitted.",
                options.wall, options.idle
            ),
        );
        log.event(
            "DEBUG",
            &format!(
                "Output queue bounded to {PENDING_CHUNKS} chunks of at most {CHUNK_BYTES} bytes."
            ),
        );
        for stream in [
            process.child.stdout.take().map(Pipe::Out),
            process.child.stderr.take().map(Pipe::Err),
        ] {
            let stream = stream.ok_or_else(|| io::Error::other("Missing output pipe"))?;
            readers.push(match stream {
                Pipe::Out(pipe) => reader(pipe, events.clone(), stop.clone())?,
                Pipe::Err(pipe) => reader(pipe, events.clone(), stop.clone())?,
            });
        }
        let mut complete = 0;
        let mut progress = started;
        let mut status = None;
        while complete < 2 || status.is_none() {
            if process::interrupted() {
                log.event("WARNING", "Check interrupted; stopping owned processes.");
                return Ok(130);
            }
            if started.elapsed().as_secs_f64() > options.wall
                || progress.elapsed().as_secs_f64() > options.idle
            {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Check exceeded wall or idle limit",
                ));
            }
            if status.is_none() {
                status = process.child.try_wait()?;
            }
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(OutputEvent::Eof) => complete += 1,
                Ok(OutputEvent::Error(kind)) => {
                    log.event(
                        "ERROR",
                        &format!("Output reader failed ({kind:?}); payload omitted."),
                    );
                    return Err(io::Error::other("Cannot verify complete command output"));
                }
                Ok(OutputEvent::Chunk(bytes)) => {
                    progress = Instant::now();
                    let mut output = io::stdout().lock();
                    output.write_all(&bytes)?;
                    output.flush()?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("Output readers disconnected"));
                }
            }
        }
        status
            .map(process::exit_code)
            .ok_or_else(|| io::Error::other("Missing child status"))
    })();
    let mut code = match result {
        Ok(code) => code,
        Err(error) => {
            let label = if error.kind() == io::ErrorKind::TimedOut {
                "TimeoutError"
            } else {
                "IoError"
            };
            log.event(
                "ERROR",
                &format!("Check failed ({label}); payload omitted."),
            );
            1
        }
    };
    stop.store(true, Ordering::Relaxed);
    drop(events);
    drop(receiver);
    if let Some(process) = owned.as_mut()
        && process.stop().is_err()
    {
        log.event("ERROR", "Owned termination/wait failed; payload omitted.");
        code = 1;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    for reader in readers {
        while !reader.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !reader.is_finished() {
            log.event(
                "ERROR",
                "Owned output reader did not stop; no successful verification claimed.",
            );
            code = 1;
        } else if reader.join().is_err() {
            log.event("ERROR", "Owned output reader failed; payload omitted.");
            code = 1;
        }
    }
    log.event(
        if code == 0 { "INFO" } else { "ERROR" },
        &format!(
            "Completed exit={code} duration_ms={}.",
            started.elapsed().as_millis()
        ),
    );
    code
}

enum Pipe {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
}
