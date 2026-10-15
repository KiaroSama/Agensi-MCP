#[allow(dead_code)]
#[path = "../../tools/check/process.rs"]
mod process;
#[allow(dead_code)]
#[path = "../../tools/tests/runner_fixture.rs"]
mod runner_fixture;

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Output,
    time::{Duration, Instant},
};

struct Installation {
    directory: tempfile::TempDir,
    root: PathBuf,
    cwd: PathBuf,
    binary: PathBuf,
    bootstrap: PathBuf,
}
impl Installation {
    fn new() -> Self {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"));
        fs::create_dir_all(source.join(".tmp")).expect("Project scratch");
        let directory = tempfile::Builder::new()
            .prefix("entry isolated-é-")
            .tempdir_in(source.join(".tmp"))
            .expect("Owned fixture");
        let root = directory.path().join("installation with spaces-é");
        let cwd = directory.path().join("foreign cwd-é");
        fs::create_dir_all(root.join("bin")).expect("Fixture bin");
        fs::create_dir(&cwd).expect("Foreign CWD");
        for name in ["Cargo.toml", "run.ps1"] {
            fs::copy(source.join(name), root.join(name)).expect("Root markers");
        }
        let binary = root.join("bin").join(if cfg!(windows) {
            "agensi-mcp.exe"
        } else {
            "agensi-mcp"
        });
        let bootstrap = root.join(if cfg!(windows) {
            "check-run.exe"
        } else {
            "check-run"
        });
        fs::copy(env!("CARGO_BIN_EXE_agensi-mcp"), &binary).expect("Copy native executable");
        fs::copy(env!("CARGO_BIN_EXE_check-run"), &bootstrap).expect("Copy trusted bootstrap");
        Self {
            directory,
            root,
            cwd,
            binary,
            bootstrap,
        }
    }
    fn capture(&self, command: &[OsString], input: &[u8], limit: Duration) -> Output {
        runner_fixture::capture_fixture_with_input(
            &self.bootstrap,
            &self.cwd,
            command,
            input,
            &runner_fixture::fixture_environment(&self.root),
            limit,
        )
    }
}

#[test]
fn native_entry_status_from_foreign_cwd_and_failure_exit() {
    let installation = Installation::new();
    let output = installation.capture(
        &[
            installation.binary.clone().into_os_string(),
            "status".into(),
        ],
        b"",
        Duration::from_secs(3),
    );
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("Status JSON");
    assert_eq!(json["mode"], "disconnected");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("password"));
    let failed = installation.capture(
        &[
            installation.binary.clone().into_os_string(),
            "invalid-command".into(),
        ],
        b"",
        Duration::from_secs(3),
    );
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Use status"));
    assert!(!installation.root.join("config.local.json").exists());
    assert!(!installation.cwd.join("logs").exists());
    let logs = fs::read_dir(installation.root.join("logs")).expect("Copied installation logs");
    assert!(
        logs.count() >= 2,
        "Both commands must log inside the copied installation"
    );
    assert!(installation.directory.path().is_dir());
}

#[test]
#[cfg(windows)]
fn native_menu_has_colon_and_signin_without_profile_dump() {
    let installation = Installation::new();
    let command = vec![
        "pwsh".into(),
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-File".into(),
        installation.root.join("run.ps1").into_os_string(),
    ];
    let output = installation.capture(&command, b"exit\n", Duration::from_secs(6));
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("UTF8 menu");
    assert!(text.contains("quit=exit: "));
    assert!(text.contains("7. Sign in"));
    assert!(!text.contains("socks5://"));
    assert!(!installation.root.join("config.local.json").exists());
    assert!(!installation.cwd.join("logs").exists());
}

#[test]
fn stalled_fixture_capture_reaps_descendants_and_readers() {
    let installation = Installation::new();
    let pid_path = installation.root.join("descendant.pid");
    #[cfg(windows)]
    let command: Vec<OsString> = vec![
        "pwsh".into(), "-NoProfile".into(), "-Command".into(),
        "$s=[Diagnostics.ProcessStartInfo]::new('pwsh');foreach($v in @('-NoProfile','-Command','Start-Sleep 30')){$s.ArgumentList.Add($v)};$s.UseShellExecute=$false;$s.CreateNoWindow=$true;$p=[Diagnostics.Process]::Start($s);[IO.File]::WriteAllText($env:AGENSI_FIXTURE_PID,[string]$p.Id,[Text.UTF8Encoding]::new($false));$p.WaitForExit()".into(),
    ];
    #[cfg(unix)]
    let command: Vec<OsString> = vec![
        "/bin/sh".into(),
        "-c".into(),
        "sleep 30 & child=$!; printf '%s' \"$child\" > \"$1\"; wait \"$child\"".into(),
        "fixture".into(),
        pid_path.clone().into_os_string(),
    ];
    // This intentional stall is the timeout control, not a readiness sleep.
    let started = Instant::now();
    let mut environment = runner_fixture::fixture_environment(&installation.root);
    environment.push((
        "AGENSI_FIXTURE_PID".into(),
        pid_path.clone().into_os_string(),
    ));
    let owned = process::OwnedProcess::spawn_fixture_with_input(
        &installation.bootstrap,
        &installation.cwd,
        &command,
        b"",
        &environment,
    )
    .expect("Owned stalled fixture");
    // Readiness, not cold PowerShell startup, starts the deliberately short stall budget.
    let deadline = Instant::now() + Duration::from_secs(6);
    while !pid_path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        pid_path.exists(),
        "Stall fixture never reached child readiness"
    );
    let result = std::panic::catch_unwind(move || {
        runner_fixture::capture_owned(owned, Duration::from_secs(1))
    });
    assert!(
        result.is_err(),
        "A stalled child must fail its capture bound"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    let pid: u32 = fs::read_to_string(pid_path)
        .expect("Child readiness PID")
        .parse()
        .expect("PID");
    runner_fixture::wait_dead(pid, Duration::from_secs(2));
}
