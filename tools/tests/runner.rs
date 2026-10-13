//! Exact18 RunnerTests identities and load-bearing assertions from source309052d.
#[path = "../check/process.rs"]
pub mod process;
#[path = "../check/runner.rs"]
pub mod runner;
pub mod runner_fixture;

use runner_fixture::{Fixture, wait_dead};
use std::{
    ffi::OsString,
    fs,
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

fn test_nonzero_exit_is_preserved() {
    let fixture = Fixture::new();
    assert_eq!(fixture.run("exit7").status.code(), Some(7));
    for flag in ["--help", "-h"] {
        let help = runner_fixture::capture(
            fixture.directory.path(),
            &[fixture.runner.clone().into_os_string(), flag.into()],
        );
        assert!(help.status.success());
        assert!(help.stdout.starts_with(b"usage: check-run"));
        assert!(help.stderr.is_empty());
        // Help after the command boundary remains child argv, never runner help.
        let child = fixture.invoke(fixture.program("payload", &[flag.into()]), "6", "3");
        assert!(child.status.success());
        assert_eq!(child.stdout, format!("{flag}\n").as_bytes());
    }
}

fn progress(stream: &str) {
    let fixture = Fixture::new();
    let result = fixture.invoke(fixture.program(stream, &[]), "6", "1");
    assert!(
        result.status.success(),
        "Active byte output must not require newline framing"
    );
    assert_eq!(result.stdout, [vec![b'x'; 20], b"END".to_vec()].concat());
}
fn test_unterminated_stdout_is_progress() {
    progress("stdout-progress");
}
fn test_unterminated_stderr_is_progress() {
    progress("stderr-progress");
}
fn bytes(stream: &str) {
    let result = Fixture::new().run(stream);
    assert!(result.status.success());
    assert_eq!(result.stdout, b"begin\xffend\n");
    assert!(!result.stderr.windows(9).any(|v| v == b"Traceback"));
}
fn test_non_utf8_stdout_is_preserved() {
    bytes("stdout-bytes");
}
fn test_non_utf8_stderr_is_preserved() {
    bytes("stderr-bytes");
}
fn test_split_multibyte_sequence_is_preserved() {
    let result = Fixture::new().run("split");
    assert!(result.status.success());
    assert_eq!(result.stdout, b"\xe2\x82\xacEND");
}
fn test_nonfinite_and_nonpositive_limits_never_start_command() {
    let fixture = Fixture::new();
    let marker = fixture.directory.path().join("started");
    for field in ["wall", "idle"] {
        for value in ["nan", "inf", "-inf", "0", "-1"] {
            let command = fixture.program("marker", &[marker.clone().into_os_string()]);
            let result = fixture.invoke(
                command,
                if field == "wall" { value } else { "6" },
                if field == "idle" { value } else { "3" },
            );
            assert!(
                !marker.exists(),
                "Invalid limits must fail before process creation"
            );
            assert!(!result.status.success());
            assert!(
                !fixture.root.join("logs").exists(),
                "Invalid bounds must fail before logging"
            );
        }
    }
}
fn test_large_unterminated_output_finishes_without_truncation() {
    let result = Fixture::new().run("large");
    assert!(result.status.success());
    assert_eq!(result.stdout.len(), 2 * 1024 * 1024 + 3);
    assert!(result.stdout.ends_with(b"END"));
}
fn test_true_idle_check_fails_within_bound() {
    let fixture = Fixture::new();
    let started = Instant::now();
    let result = fixture.invoke(fixture.program("idle", &[]), "4", ".5");
    assert!(!result.status.success());
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(String::from_utf8_lossy(&result.stderr).contains("TimeoutError"));
}
fn test_wall_deadline_remains_independent_of_progress() {
    let fixture = Fixture::new();
    let started = Instant::now();
    let result = fixture.invoke(fixture.program("wall", &[]), ".8", "3");
    assert!(!result.status.success());
    assert!(started.elapsed() < Duration::from_secs(5));
}
fn descendant_case(inherit: bool) {
    let fixture = Fixture::new();
    let lease = fixture.directory.path().join("fixture.lease");
    let pid_file = fixture.directory.path().join("grandchild.pid");
    fs::write(&lease, b"").expect("Create unique cooperative lease");
    let result = fixture.invoke(
        fixture.program(
            if inherit {
                "parent-inherit"
            } else {
                "parent-null"
            },
            &[
                lease.clone().into_os_string(),
                pid_file.clone().into_os_string(),
            ],
        ),
        "4",
        if inherit { ".8" } else { "2" },
    );
    let pid: u32 = fs::read_to_string(&pid_file)
        .expect("Descendant pid file")
        .parse()
        .expect("Descendant pid");
    // Always release the cooperative fixture before an assertion can unwind.
    let deadline = Instant::now() + Duration::from_secs(3);
    while process::alive(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    let survived = process::alive(pid);
    fs::remove_file(lease).expect("Release fixture lease");
    wait_dead(pid, Duration::from_secs(3));
    assert!(!survived, "An owned descendant survived runner cleanup");
    let safe_status = String::from_utf8_lossy(&result.stderr)
        .lines()
        .filter(|line| line.starts_with('[') && line.contains("] [check] "))
        .map(|line| line.chars().take(256).collect::<String>())
        .take(20)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        result.status.success(),
        !inherit,
        "Safe runner status:\n{safe_status}"
    );
}
fn test_success_reaps_descendant_after_direct_parent_exit() {
    descendant_case(false);
}
fn test_idle_timeout_reaps_descendant_holding_pipes() {
    descendant_case(true);
}
fn test_linked_log_directory_is_not_written() {
    let fixture = Fixture::new();
    let outside = fixture.directory.path().join("outside log destination");
    fs::create_dir(&outside).expect("Create outside directory");
    let link = fixture.root.join("logs");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).expect("Create linked log directory");
    #[cfg(windows)]
    {
        // cmd is used only for the native junction operation, with separate trusted path argv.
        let result = runner_fixture::capture(
            fixture.directory.path(),
            &[
                "cmd".into(),
                "/d".into(),
                "/c".into(),
                "mklink".into(),
                "/J".into(),
                link.clone().into_os_string(),
                outside.clone().into_os_string(),
            ],
        );
        assert!(
            result.status.success(),
            "Create junction without privilege-dependent symlinks"
        );
    }
    let result = fixture.run("empty");
    #[cfg(unix)]
    fs::remove_file(link).expect("Remove owned link");
    #[cfg(windows)]
    fs::remove_dir(link).expect("Remove owned junction");
    assert!(
        result.status.success(),
        "Safe stderr fallback must remain usable"
    );
    assert_eq!(fs::read_dir(outside).expect("Outside directory").count(), 0);
    let status = String::from_utf8_lossy(&result.stderr);
    assert!(status.contains("[WARNING]") && status.contains("[INFO]"));
}
fn test_own_logs_omit_command_arguments_and_output_payload() {
    let fixture = Fixture::new();
    let sentinel = "fixture-private-payload-not-a-real-secret";
    let result = fixture.invoke(fixture.program("payload", &[sentinel.into()]), "6", "3");
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains(sentinel));
    let logs = fixture.logs();
    assert_eq!(logs.len(), 1);
    let text = fs::read_to_string(&logs[0]).expect("UTF8 log");
    assert!(!text.contains(sentinel));
    assert!(
        regex::Regex::new(r"\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} UTC\] \[INFO\]")
            .expect("Timestamp regex")
            .is_match(&text)
    );
}
fn test_full_queue_reader_stops_without_deadlock() {
    let pending = std::hint::black_box(runner::PENDING_CHUNKS);
    let chunk = std::hint::black_box(runner::CHUNK_BYTES);
    assert!(pending > 0);
    assert!(pending * chunk <= 1024 * 1024);
    let (events, receiver) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let mut stream = io::Cursor::new(vec![b'x'; runner::CHUNK_BYTES * 4]);
    let (full, ready) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut reads = 0;
        runner::consume_output(
            |buffer| {
                let result = stream.read(buffer);
                reads += 1;
                // On the second read the first event has filled the queue and no receiver drains it.
                if reads == 2 {
                    full.send(()).expect("Readiness signal");
                }
                result
            },
            &events,
            &worker_stop,
        );
    });
    let saturated = ready.recv_timeout(Duration::from_secs(2)).is_ok();
    stop.store(true, Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let stopped = worker.is_finished();
    drop(receiver);
    worker.join().expect("Owned queue reader");
    assert!(saturated);
    assert!(stopped, "A full output queue prevented owned cleanup");
}
fn test_reader_failure_is_distinct_from_eof_and_redacted() {
    let (events, receiver) = mpsc::sync_channel(4);
    runner::consume_output(
        |_| {
            Err(io::Error::other(
                "fixture exception payload must not escape",
            ))
        },
        &events,
        &AtomicBool::new(false),
    );
    assert!(matches!(
        receiver.try_recv(),
        Ok(runner::OutputEvent::Error(io::ErrorKind::Other))
    ));
    assert!(matches!(receiver.try_recv(), Ok(runner::OutputEvent::Eof)));
    assert!(receiver.try_recv().is_err());
}
fn test_logging_supports_all_four_primary_levels() {
    let fixture = Fixture::new();
    {
        let logger = runner::CheckLog::new(&fixture.root, true);
        for level in ["INFO", "WARNING", "ERROR", "DEBUG"] {
            logger.event(level, "Safe fixture status.");
        }
    }
    let logs = fixture.logs();
    assert_eq!(logs.len(), 1);
    let text = fs::read_to_string(&logs[0]).expect("UTF8 log");
    for level in ["INFO", "WARNING", "ERROR", "DEBUG"] {
        assert!(text.contains(&format!("[{level}]")));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&logs[0])
                .expect("Log metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
fn test_missing_executable_fails_without_traceback() {
    let fixture = Fixture::new();
    let result = fixture.invoke(
        vec![
            fixture
                .directory
                .path()
                .join("missing-command")
                .into_os_string(),
        ],
        "6",
        "3",
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("[ERROR]"));
    assert!(
        !String::from_utf8_lossy(&[result.stdout, result.stderr].concat()).contains("Traceback")
    );
}

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|v| v == "--bootstrap-input") {
        std::process::exit(process::bootstrap_input(args.into_iter().skip(1).collect()));
    }
    if args.first().is_some_and(|v| v == "--bootstrap") {
        std::process::exit(process::bootstrap(args.into_iter().skip(1).collect()));
    }
    if args.first().is_some_and(|v| v == "--fixture") {
        std::process::exit(runner_fixture::child(&args[1..]));
    }
    let tests: [(&str, fn()); 18] = [
        (
            "test_nonzero_exit_is_preserved",
            test_nonzero_exit_is_preserved,
        ),
        (
            "test_unterminated_stdout_is_progress",
            test_unterminated_stdout_is_progress,
        ),
        (
            "test_unterminated_stderr_is_progress",
            test_unterminated_stderr_is_progress,
        ),
        (
            "test_non_utf8_stdout_is_preserved",
            test_non_utf8_stdout_is_preserved,
        ),
        (
            "test_non_utf8_stderr_is_preserved",
            test_non_utf8_stderr_is_preserved,
        ),
        (
            "test_split_multibyte_sequence_is_preserved",
            test_split_multibyte_sequence_is_preserved,
        ),
        (
            "test_nonfinite_and_nonpositive_limits_never_start_command",
            test_nonfinite_and_nonpositive_limits_never_start_command,
        ),
        (
            "test_large_unterminated_output_finishes_without_truncation",
            test_large_unterminated_output_finishes_without_truncation,
        ),
        (
            "test_true_idle_check_fails_within_bound",
            test_true_idle_check_fails_within_bound,
        ),
        (
            "test_wall_deadline_remains_independent_of_progress",
            test_wall_deadline_remains_independent_of_progress,
        ),
        (
            "test_success_reaps_descendant_after_direct_parent_exit",
            test_success_reaps_descendant_after_direct_parent_exit,
        ),
        (
            "test_idle_timeout_reaps_descendant_holding_pipes",
            test_idle_timeout_reaps_descendant_holding_pipes,
        ),
        (
            "test_linked_log_directory_is_not_written",
            test_linked_log_directory_is_not_written,
        ),
        (
            "test_own_logs_omit_command_arguments_and_output_payload",
            test_own_logs_omit_command_arguments_and_output_payload,
        ),
        (
            "test_full_queue_reader_stops_without_deadlock",
            test_full_queue_reader_stops_without_deadlock,
        ),
        (
            "test_reader_failure_is_distinct_from_eof_and_redacted",
            test_reader_failure_is_distinct_from_eof_and_redacted,
        ),
        (
            "test_logging_supports_all_four_primary_levels",
            test_logging_supports_all_four_primary_levels,
        ),
        (
            "test_missing_executable_fails_without_traceback",
            test_missing_executable_fails_without_traceback,
        ),
    ];
    let selected = if args.is_empty() {
        None
    } else if args.len() == 1 {
        args[0].to_str().and_then(|arg| arg.strip_prefix("--case="))
    } else {
        None
    };
    if !args.is_empty()
        && !selected.is_some_and(|name| tests.iter().any(|(known, _)| *known == name))
    {
        eprintln!("Unknown focused runner case; no contracts executed.");
        std::process::exit(2);
    }
    let mut failures = 0;
    let mut executed = 0;
    for (name, test) in tests {
        if selected.is_some_and(|selected| selected != name) {
            continue;
        }
        executed += 1;
        eprintln!("Starting RunnerTests.{name}");
        let started = Instant::now();
        let passed = std::panic::catch_unwind(test).is_ok();
        eprintln!(
            "RunnerTests.{name}: {} duration_ms={}",
            if passed { "PASS" } else { "FAIL" },
            started.elapsed().as_millis()
        );
        failures += usize::from(!passed);
    }
    eprintln!("runner contracts:{executed} failures:{failures}");
    std::process::exit(i32::from(failures != 0));
}
